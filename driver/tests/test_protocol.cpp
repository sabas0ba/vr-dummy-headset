// Protocol tests. The golden fixtures in tests/fixtures are produced by the
// Rust implementation; the values below must match
// feeder/crates/protocol/tests/fixtures.rs.
#include <cstring>
#include <fstream>
#include <iterator>
#include <string>
#include <vector>

#include "doctest.h"
#include "protocol.h"

using namespace vrdummy::protocol;

namespace {

std::vector<uint8_t> ReadFixture(const std::string& name) {
    const std::string path = std::string(VRDUMMY_FIXTURE_DIR) + "/" + name + ".bin";
    std::ifstream in(path, std::ios::binary);
    REQUIRE_MESSAGE(in.good(), "fixture missing: " << path);
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

Message DecodeOk(const std::vector<uint8_t>& bytes) {
    DecodeResult r = Decode(bytes.data(), bytes.size());
    REQUIRE_MESSAGE(r.status == DecodeStatus::Ok, ToString(r.status) << ": " << r.detail);
    REQUIRE(r.consumed == bytes.size());
    REQUIRE(r.message.has_value());
    return *r.message;
}

void CheckRoundTrip(const std::string& fixture, const Message& expected) {
    const std::vector<uint8_t> golden = ReadFixture(fixture);
    const std::vector<uint8_t> encoded = Encode(expected);
    CHECK_MESSAGE(encoded == golden, fixture << ": encoding differs from fixture");
    const Message decoded = DecodeOk(golden);
    CHECK(Encode(decoded) == golden);
}

}  // namespace

TEST_CASE("fixed layout sizes") {
    CHECK(Encode(Message{Hello{}}).size() == kHeaderLen + sizes::kHello);
    CHECK(Encode(Message{Pose{}}).size() == kHeaderLen + sizes::kPose);
    CHECK(Encode(Message{Input{}}).size() == kHeaderLen + sizes::kInput);
    CHECK(Encode(Message{Display{}}).size() == kHeaderLen + sizes::kDisplay);
    CHECK(Encode(Message{LogRecord{}}).size() == kHeaderLen + sizes::kLogHead);
    CHECK(Encode(Message{Goodbye{}}).size() == kHeaderLen);
    HelloAck ack;
    ack.devices.push_back(DeviceInfo{0, DeviceClass::Hmd, ""});
    CHECK(Encode(Message{ack}).size() == kHeaderLen + sizes::kHelloAckHead + sizes::kDeviceInfo);
}

TEST_CASE("fixture hello_v1") {
    Hello h;
    h.protocol_version = 1;
    h.client_flags = 0;
    CheckRoundTrip("hello_v1", Message{h});
    const auto m = std::get<Hello>(DecodeOk(ReadFixture("hello_v1")));
    CHECK(m.protocol_version == 1);
    CHECK(m.client_flags == 0);
}

TEST_CASE("fixture hello_ack_v1") {
    HelloAck ack;
    ack.protocol_version = 1;
    ack.devices = {
        DeviceInfo{0, DeviceClass::Hmd, "VRDUMMY-HMD-0"},
        DeviceInfo{1, DeviceClass::ControllerLeft, "VRDUMMY-CTRL-L"},
        DeviceInfo{3, DeviceClass::Tracker, "VRDUMMY-TRK-3"},
    };
    CheckRoundTrip("hello_ack_v1", Message{ack});
    const auto m = std::get<HelloAck>(DecodeOk(ReadFixture("hello_ack_v1")));
    REQUIRE(m.devices.size() == 3);
    CHECK(m.devices[1].device_id == 1);
    CHECK(m.devices[1].device_class == DeviceClass::ControllerLeft);
    CHECK(m.devices[1].serial == "VRDUMMY-CTRL-L");
    CHECK(m.devices[2].serial == "VRDUMMY-TRK-3");
}

TEST_CASE("fixture pose_update_v1") {
    Pose p;
    p.device_id = 3;
    p.flags.bits = PoseFlags::kPoseValid | PoseFlags::kConnected;
    p.tracking_result = TrackingResult::RunningOk;
    p.timestamp_us = 123456789;
    p.position = {0.1, 1.6, -0.25};
    p.rotation = {0.9238795, 0.0, 0.3826834, 0.0};
    p.linear_velocity = {0.5, 0.0, 0.0};
    p.angular_velocity = {0.0, 0.25, 0.0};
    CheckRoundTrip("pose_update_v1", Message{p});
    const auto m = std::get<Pose>(DecodeOk(ReadFixture("pose_update_v1")));
    CHECK(m.device_id == 3);
    CHECK(m.flags.pose_valid());
    CHECK(m.flags.connected());
    CHECK(m.tracking_result == TrackingResult::RunningOk);
    CHECK(m.timestamp_us == 123456789);
    CHECK(m.position[1] == doctest::Approx(1.6));
    CHECK(m.rotation[2] == doctest::Approx(0.3826834));
    CHECK(m.angular_velocity[1] == doctest::Approx(0.25));
}

TEST_CASE("fixture input_update_v1") {
    Input in;
    in.device_id = 2;
    in.component_id = 11;
    in.kind = InputKind::Scalar;
    in.value = -0.5f;
    in.time_offset_s = 0.0;
    CheckRoundTrip("input_update_v1", Message{in});
    const auto m = std::get<Input>(DecodeOk(ReadFixture("input_update_v1")));
    CHECK(m.device_id == 2);
    CHECK(m.component_id == 11);
    CHECK(m.kind == InputKind::Scalar);
    CHECK(m.value == doctest::Approx(-0.5f));
}

TEST_CASE("fixture display_update_v1") {
    Display d;
    d.flags.bits = DisplayFlags::kIpd | DisplayFlags::kProjection | DisplayFlags::kSwapEyes;
    d.ipd_m = 0.07f;
    d.projection_left = {-1.2f, 0.8f, -1.0f, 1.0f};
    d.projection_right = {-0.8f, 1.2f, -1.0f, 1.0f};
    d.swap_eyes = true;
    CheckRoundTrip("display_update_v1", Message{d});
    const auto m = std::get<Display>(DecodeOk(ReadFixture("display_update_v1")));
    CHECK(m.flags.has(DisplayFlags::kIpd));
    CHECK(m.ipd_m == doctest::Approx(0.07f));
    CHECK(m.projection_left[0] == doctest::Approx(-1.2f));
    CHECK(m.projection_right[1] == doctest::Approx(1.2f));
    CHECK(m.swap_eyes);
}

TEST_CASE("fixture log_record_v1") {
    LogRecord l;
    l.level = LogLevel::Info;
    l.timestamp_us = 42;
    l.text = "driver ready";
    CheckRoundTrip("log_record_v1", Message{l});
    const auto m = std::get<LogRecord>(DecodeOk(ReadFixture("log_record_v1")));
    CHECK(m.level == LogLevel::Info);
    CHECK(m.timestamp_us == 42);
    CHECK(m.text == "driver ready");
}

TEST_CASE("fixture goodbye_v1") {
    CheckRoundTrip("goodbye_v1", Message{Goodbye{}});
    CHECK(std::holds_alternative<Goodbye>(DecodeOk(ReadFixture("goodbye_v1"))));
}

TEST_CASE("decoder resynchronises after bad frames") {
    const std::vector<uint8_t> good = Encode(Message{Goodbye{}});
    std::vector<uint8_t> bad_version = good;
    bad_version[6] = 9;
    std::vector<uint8_t> unknown_type = good;
    unknown_type[4] = 200;
    std::vector<uint8_t> malformed = Encode(Message{Hello{}});
    malformed[0] = 7;
    malformed.pop_back();

    Decoder d;
    d.Push(bad_version.data(), bad_version.size());
    d.Push(unknown_type.data(), unknown_type.size());
    d.Push(malformed.data(), malformed.size());
    d.Push(good.data(), 5);

    auto f = d.NextFrame();
    REQUIRE(f.has_value());
    CHECK(f->status == DecodeStatus::VersionMismatch);
    CHECK(f->raw_version == 9);
    f = d.NextFrame();
    REQUIRE(f.has_value());
    CHECK(f->status == DecodeStatus::UnknownType);
    CHECK(f->raw_type == 200);
    f = d.NextFrame();
    REQUIRE(f.has_value());
    CHECK(f->status == DecodeStatus::Malformed);
    CHECK_FALSE(d.NextFrame().has_value());
    d.Push(good.data() + 5, good.size() - 5);
    f = d.NextFrame();
    REQUIRE(f.has_value());
    CHECK(f->status == DecodeStatus::Ok);
    CHECK(std::holds_alternative<Goodbye>(*f->message));
    CHECK_FALSE(d.NextFrame().has_value());
    CHECK(d.buffered() == 0);
}

TEST_CASE("oversized payload is fatal") {
    std::vector<uint8_t> frame(kHeaderLen, 0);
    const uint32_t len = 70000;
    std::memcpy(frame.data(), &len, 4);
    frame[4] = 7;
    frame[6] = 1;
    const DecodeResult r = Decode(frame.data(), frame.size());
    CHECK(r.status == DecodeStatus::PayloadTooLarge);
    Decoder d;
    d.Push(frame.data(), frame.size());
    const auto f = d.NextFrame();
    REQUIRE(f.has_value());
    CHECK(f->status == DecodeStatus::PayloadTooLarge);
}

TEST_CASE("encode rejects oversized serial") {
    HelloAck ack;
    ack.devices.push_back(DeviceInfo{0, DeviceClass::Hmd, std::string(sizes::kSerial, 'x')});
    CHECK(Encode(Message{ack}).empty());
}
