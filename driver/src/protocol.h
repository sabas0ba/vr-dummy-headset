// IPC wire format between driver_vrdummy and the feeder.
// Specification: docs/protocol.md. Mirrors feeder/crates/protocol.
// This file has no OpenVR dependency so that it can be unit tested.
#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <variant>
#include <vector>

namespace vrdummy::protocol {

constexpr uint16_t kProtocolVersion = 1;
constexpr size_t kHeaderLen = 8;
constexpr size_t kMaxPayloadLen = 65536;
constexpr const char* kDefaultPipeName = "vrdummy-driver";

namespace sizes {
constexpr size_t kHello = 8;
constexpr size_t kHelloAckHead = 8;
constexpr size_t kDeviceInfo = 40;
constexpr size_t kSerial = 36;
constexpr size_t kPose = 120;
constexpr size_t kInput = 20;
constexpr size_t kDisplay = 44;
constexpr size_t kLogHead = 12;
}  // namespace sizes

namespace device_ids {
constexpr uint16_t kHmd = 0;
constexpr uint16_t kControllerLeft = 1;
constexpr uint16_t kControllerRight = 2;
constexpr uint16_t kTrackerBase = 3;
}  // namespace device_ids

enum class MsgType : uint16_t {
    Hello = 1,
    HelloAck = 2,
    PoseUpdate = 3,
    InputUpdate = 4,
    DisplayUpdate = 5,
    LogRecord = 6,
    Goodbye = 7,
};

enum class DeviceClass : uint8_t {
    Hmd = 1,
    ControllerLeft = 2,
    ControllerRight = 3,
    Tracker = 4,
};

enum class TrackingResult : uint8_t {
    Uninitialized = 0,
    RunningOk = 1,
    RunningOutOfRange = 2,
    Calibrating = 3,
    FallbackRotationOnly = 4,
};

enum class InputKind : uint8_t {
    Bool = 0,
    Scalar = 1,
};

enum class LogLevel : uint8_t {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
};

struct Hello {
    uint32_t protocol_version = kProtocolVersion;
    uint32_t client_flags = 0;
};

struct DeviceInfo {
    uint16_t device_id = 0;
    DeviceClass device_class = DeviceClass::Tracker;
    std::string serial;  // at most sizes::kSerial - 1 bytes
};

struct HelloAck {
    uint32_t protocol_version = kProtocolVersion;
    std::vector<DeviceInfo> devices;
};

struct PoseFlags {
    static constexpr uint8_t kPoseValid = 1u << 0;
    static constexpr uint8_t kConnected = 1u << 1;
    uint8_t bits = 0;
    bool pose_valid() const { return (bits & kPoseValid) != 0; }
    bool connected() const { return (bits & kConnected) != 0; }
};

struct Pose {
    uint16_t device_id = 0;
    PoseFlags flags{};
    TrackingResult tracking_result = TrackingResult::Uninitialized;
    uint64_t timestamp_us = 0;
    std::array<double, 3> position{};
    std::array<double, 4> rotation{1.0, 0.0, 0.0, 0.0};  // w, x, y, z
    std::array<double, 3> linear_velocity{};
    std::array<double, 3> angular_velocity{};
};

struct Input {
    uint16_t device_id = 0;
    uint16_t component_id = 0;
    InputKind kind = InputKind::Bool;
    float value = 0.0f;
    double time_offset_s = 0.0;
};

struct DisplayFlags {
    static constexpr uint32_t kIpd = 1u << 0;
    static constexpr uint32_t kProjection = 1u << 1;
    static constexpr uint32_t kSwapEyes = 1u << 2;
    uint32_t bits = 0;
    bool has(uint32_t flag) const { return (bits & flag) != 0; }
};

struct Display {
    DisplayFlags flags{};
    float ipd_m = 0.0f;
    std::array<float, 4> projection_left{};   // left, right, top, bottom tangents
    std::array<float, 4> projection_right{};
    bool swap_eyes = false;
};

struct LogRecord {
    LogLevel level = LogLevel::Info;
    uint64_t timestamp_us = 0;
    std::string text;
};

struct Goodbye {};

using Message = std::variant<Hello, HelloAck, Pose, Input, Display, LogRecord, Goodbye>;

MsgType TypeOf(const Message& message);

// Encodes a message as a complete frame (header plus payload).
// Returns an empty vector if the message violates a size limit.
std::vector<uint8_t> Encode(const Message& message);

enum class DecodeStatus {
    Ok,
    Incomplete,       // more bytes are needed
    VersionMismatch,  // frame complete, skip it
    UnknownType,      // frame complete, skip it
    PayloadTooLarge,  // fatal, close the connection
    Malformed,        // frame complete, skip it
};

struct DecodeResult {
    DecodeStatus status = DecodeStatus::Incomplete;
    size_t consumed = 0;  // bytes of the frame, valid unless Incomplete or PayloadTooLarge
    uint16_t raw_type = 0;
    uint16_t raw_version = 0;
    std::optional<Message> message;
    const char* detail = "";
};

// Decodes one frame from the start of the buffer.
DecodeResult Decode(const uint8_t* data, size_t len);

// Incremental frame reader over a byte stream.
class Decoder {
public:
    void Push(const uint8_t* data, size_t len);
    // Returns the next complete frame. Bad frames are consumed and reported
    // so that the stream stays in sync. PayloadTooLarge is not recoverable.
    std::optional<DecodeResult> NextFrame();
    size_t buffered() const { return buffer_.size(); }

private:
    std::vector<uint8_t> buffer_;
};

const char* ToString(DecodeStatus status);

}  // namespace vrdummy::protocol
