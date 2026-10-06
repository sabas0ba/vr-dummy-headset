#include "protocol.h"

#include <cstring>

namespace vrdummy::protocol {

namespace {

class Writer {
public:
    void U8(uint8_t v) { buf_.push_back(v); }
    void U16(uint16_t v) { Raw(&v, 2); }
    void U32(uint32_t v) { Raw(&v, 4); }
    void U64(uint64_t v) { Raw(&v, 8); }
    void F32(float v) { Raw(&v, 4); }
    void F64(double v) { Raw(&v, 8); }
    void Zeros(size_t n) { buf_.resize(buf_.size() + n, 0); }
    void Bytes(const void* p, size_t n) {
        const auto* b = static_cast<const uint8_t*>(p);
        buf_.insert(buf_.end(), b, b + n);
    }
    std::vector<uint8_t>& buf() { return buf_; }

private:
    // Little-endian is assumed; the driver only targets x86-64 Windows.
    void Raw(const void* p, size_t n) { Bytes(p, n); }
    std::vector<uint8_t> buf_;
};

class Reader {
public:
    Reader(const uint8_t* data, size_t len) : data_(data), len_(len) {}
    bool Take(size_t n, const uint8_t** out) {
        if (n > len_ - pos_) return false;
        *out = data_ + pos_;
        pos_ += n;
        return true;
    }
    bool U8(uint8_t* v) { return Copy(v, 1); }
    bool U16(uint16_t* v) { return Copy(v, 2); }
    bool U32(uint32_t* v) { return Copy(v, 4); }
    bool U64(uint64_t* v) { return Copy(v, 8); }
    bool F32(float* v) { return Copy(v, 4); }
    bool F64(double* v) { return Copy(v, 8); }
    template <size_t N>
    bool F32Array(std::array<float, N>* a) {
        for (auto& v : *a)
            if (!F32(&v)) return false;
        return true;
    }
    template <size_t N>
    bool F64Array(std::array<double, N>* a) {
        for (auto& v : *a)
            if (!F64(&v)) return false;
        return true;
    }
    bool Skip(size_t n) {
        const uint8_t* unused = nullptr;
        return Take(n, &unused);
    }
    const uint8_t* remaining_data() const { return data_ + pos_; }
    size_t remaining() const { return len_ - pos_; }

private:
    bool Copy(void* v, size_t n) {
        const uint8_t* p = nullptr;
        if (!Take(n, &p)) return false;
        std::memcpy(v, p, n);
        return true;
    }
    const uint8_t* data_;
    size_t len_;
    size_t pos_ = 0;
};

struct Header {
    uint32_t payload_len = 0;
    uint16_t msg_type = 0;
    uint16_t version = 0;
};

bool ReadHeader(const uint8_t* data, size_t len, Header* h) {
    if (len < kHeaderLen) return false;
    std::memcpy(&h->payload_len, data, 4);
    std::memcpy(&h->msg_type, data + 4, 2);
    std::memcpy(&h->version, data + 6, 2);
    return true;
}

DecodeResult Fail(DecodeResult r, DecodeStatus status, const char* detail) {
    r.status = status;
    r.detail = detail;
    r.message.reset();
    return r;
}

bool DecodeHelloAck(Reader& r, size_t payload_len, HelloAck* out) {
    uint16_t count = 0;
    if (!r.U32(&out->protocol_version) || !r.U16(&count) || !r.Skip(2)) return false;
    if (payload_len != sizes::kHelloAckHead + static_cast<size_t>(count) * sizes::kDeviceInfo) return false;
    out->devices.reserve(count);
    for (uint16_t i = 0; i < count; ++i) {
        DeviceInfo d;
        uint8_t cls = 0;
        const uint8_t* raw = nullptr;
        if (!r.U16(&d.device_id) || !r.U8(&cls) || !r.Skip(1) || !r.Take(sizes::kSerial, &raw)) return false;
        if (cls < 1 || cls > 4) return false;
        d.device_class = static_cast<DeviceClass>(cls);
        size_t n = 0;
        while (n < sizes::kSerial && raw[n] != 0) ++n;
        d.serial.assign(reinterpret_cast<const char*>(raw), n);
        out->devices.push_back(std::move(d));
    }
    return true;
}

bool DecodePose(Reader& r, Pose* p) {
    uint8_t result = 0;
    if (!r.U16(&p->device_id) || !r.U8(&p->flags.bits) || !r.U8(&result) || !r.Skip(4)) return false;
    if (result > 4) return false;
    p->tracking_result = static_cast<TrackingResult>(result);
    return r.U64(&p->timestamp_us) && r.F64Array(&p->position) && r.F64Array(&p->rotation) &&
           r.F64Array(&p->linear_velocity) && r.F64Array(&p->angular_velocity);
}

bool DecodeInput(Reader& r, Input* in) {
    uint8_t kind = 0;
    if (!r.U16(&in->device_id) || !r.U16(&in->component_id) || !r.U8(&kind) || !r.Skip(3)) return false;
    if (kind > 1) return false;
    in->kind = static_cast<InputKind>(kind);
    return r.F32(&in->value) && r.F64(&in->time_offset_s);
}

bool DecodeDisplay(Reader& r, Display* d) {
    uint8_t swap = 0;
    if (!r.U32(&d->flags.bits) || !r.F32(&d->ipd_m) || !r.F32Array(&d->projection_left) ||
        !r.F32Array(&d->projection_right) || !r.U8(&swap) || !r.Skip(3))
        return false;
    d->swap_eyes = swap != 0;
    return true;
}

bool DecodeLog(Reader& r, size_t payload_len, LogRecord* l) {
    if (payload_len < sizes::kLogHead) return false;
    uint8_t level = 0;
    if (!r.U8(&level) || !r.Skip(3) || !r.U64(&l->timestamp_us)) return false;
    if (level > 4) return false;
    l->level = static_cast<LogLevel>(level);
    l->text.assign(reinterpret_cast<const char*>(r.remaining_data()), r.remaining());
    return true;
}

}  // namespace

MsgType TypeOf(const Message& message) {
    return std::visit(
        [](const auto& m) -> MsgType {
            using T = std::decay_t<decltype(m)>;
            if constexpr (std::is_same_v<T, Hello>) return MsgType::Hello;
            else if constexpr (std::is_same_v<T, HelloAck>) return MsgType::HelloAck;
            else if constexpr (std::is_same_v<T, Pose>) return MsgType::PoseUpdate;
            else if constexpr (std::is_same_v<T, Input>) return MsgType::InputUpdate;
            else if constexpr (std::is_same_v<T, Display>) return MsgType::DisplayUpdate;
            else if constexpr (std::is_same_v<T, LogRecord>) return MsgType::LogRecord;
            else return MsgType::Goodbye;
        },
        message);
}

std::vector<uint8_t> Encode(const Message& message) {
    Writer w;
    w.Zeros(kHeaderLen);
    bool ok = true;
    std::visit(
        [&](const auto& m) {
            using T = std::decay_t<decltype(m)>;
            if constexpr (std::is_same_v<T, Hello>) {
                w.U32(m.protocol_version);
                w.U32(m.client_flags);
            } else if constexpr (std::is_same_v<T, HelloAck>) {
                if (m.devices.size() > 0xFFFF) {
                    ok = false;
                    return;
                }
                w.U32(m.protocol_version);
                w.U16(static_cast<uint16_t>(m.devices.size()));
                w.U16(0);
                for (const auto& d : m.devices) {
                    if (d.serial.size() >= sizes::kSerial) {
                        ok = false;
                        return;
                    }
                    w.U16(d.device_id);
                    w.U8(static_cast<uint8_t>(d.device_class));
                    w.U8(0);
                    w.Bytes(d.serial.data(), d.serial.size());
                    w.Zeros(sizes::kSerial - d.serial.size());
                }
            } else if constexpr (std::is_same_v<T, Pose>) {
                w.U16(m.device_id);
                w.U8(m.flags.bits);
                w.U8(static_cast<uint8_t>(m.tracking_result));
                w.U32(0);
                w.U64(m.timestamp_us);
                for (double v : m.position) w.F64(v);
                for (double v : m.rotation) w.F64(v);
                for (double v : m.linear_velocity) w.F64(v);
                for (double v : m.angular_velocity) w.F64(v);
            } else if constexpr (std::is_same_v<T, Input>) {
                w.U16(m.device_id);
                w.U16(m.component_id);
                w.U8(static_cast<uint8_t>(m.kind));
                w.Zeros(3);
                w.F32(m.value);
                w.F64(m.time_offset_s);
            } else if constexpr (std::is_same_v<T, Display>) {
                w.U32(m.flags.bits);
                w.F32(m.ipd_m);
                for (float v : m.projection_left) w.F32(v);
                for (float v : m.projection_right) w.F32(v);
                w.U8(m.swap_eyes ? 1 : 0);
                w.Zeros(3);
            } else if constexpr (std::is_same_v<T, LogRecord>) {
                w.U8(static_cast<uint8_t>(m.level));
                w.Zeros(3);
                w.U64(m.timestamp_us);
                w.Bytes(m.text.data(), m.text.size());
            }
        },
        message);
    if (!ok) return {};
    auto& buf = w.buf();
    const size_t payload_len = buf.size() - kHeaderLen;
    if (payload_len > kMaxPayloadLen) return {};
    const auto len = static_cast<uint32_t>(payload_len);
    const auto type = static_cast<uint16_t>(TypeOf(message));
    const uint16_t version = kProtocolVersion;
    std::memcpy(buf.data(), &len, 4);
    std::memcpy(buf.data() + 4, &type, 2);
    std::memcpy(buf.data() + 6, &version, 2);
    return buf;
}

DecodeResult Decode(const uint8_t* data, size_t len) {
    DecodeResult r;
    Header h;
    if (!ReadHeader(data, len, &h)) return r;
    r.raw_type = h.msg_type;
    r.raw_version = h.version;
    if (h.payload_len > kMaxPayloadLen) return Fail(r, DecodeStatus::PayloadTooLarge, "payload exceeds limit");
    const size_t frame_len = kHeaderLen + h.payload_len;
    if (len < frame_len) return r;
    r.consumed = frame_len;
    if (h.version != kProtocolVersion) return Fail(r, DecodeStatus::VersionMismatch, "version mismatch");
    if (h.msg_type < 1 || h.msg_type > 7) return Fail(r, DecodeStatus::UnknownType, "unknown type");

    Reader reader(data + kHeaderLen, h.payload_len);
    const size_t n = h.payload_len;
    bool ok = false;
    switch (static_cast<MsgType>(h.msg_type)) {
        case MsgType::Hello: {
            Hello m;
            ok = n == sizes::kHello && reader.U32(&m.protocol_version) && reader.U32(&m.client_flags);
            if (ok) r.message = m;
            break;
        }
        case MsgType::HelloAck: {
            HelloAck m;
            ok = DecodeHelloAck(reader, n, &m);
            if (ok) r.message = std::move(m);
            break;
        }
        case MsgType::PoseUpdate: {
            Pose m;
            ok = n == sizes::kPose && DecodePose(reader, &m);
            if (ok) r.message = m;
            break;
        }
        case MsgType::InputUpdate: {
            Input m;
            ok = n == sizes::kInput && DecodeInput(reader, &m);
            if (ok) r.message = m;
            break;
        }
        case MsgType::DisplayUpdate: {
            Display m;
            ok = n == sizes::kDisplay && DecodeDisplay(reader, &m);
            if (ok) r.message = m;
            break;
        }
        case MsgType::LogRecord: {
            LogRecord m;
            ok = DecodeLog(reader, n, &m);
            if (ok) r.message = std::move(m);
            break;
        }
        case MsgType::Goodbye:
            ok = n == 0;
            if (ok) r.message = Goodbye{};
            break;
    }
    if (!ok) return Fail(r, DecodeStatus::Malformed, "payload does not match layout");
    r.status = DecodeStatus::Ok;
    return r;
}

void Decoder::Push(const uint8_t* data, size_t len) { buffer_.insert(buffer_.end(), data, data + len); }

std::optional<DecodeResult> Decoder::NextFrame() {
    DecodeResult r = Decode(buffer_.data(), buffer_.size());
    if (r.status == DecodeStatus::Incomplete) return std::nullopt;
    if (r.status == DecodeStatus::PayloadTooLarge) return r;
    buffer_.erase(buffer_.begin(), buffer_.begin() + static_cast<std::ptrdiff_t>(r.consumed));
    return r;
}

const char* ToString(DecodeStatus status) {
    switch (status) {
        case DecodeStatus::Ok: return "ok";
        case DecodeStatus::Incomplete: return "incomplete";
        case DecodeStatus::VersionMismatch: return "version mismatch";
        case DecodeStatus::UnknownType: return "unknown type";
        case DecodeStatus::PayloadTooLarge: return "payload too large";
        case DecodeStatus::Malformed: return "malformed";
    }
    return "unknown";
}

}  // namespace vrdummy::protocol
