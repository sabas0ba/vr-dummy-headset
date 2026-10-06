// Loads driver_vrdummy.dll the way vrserver does, with fake OpenVR
// interfaces, and drives it over the real named pipe. No SteamVR needed.
#define DOCTEST_CONFIG_IMPLEMENT_WITH_MAIN
#include "doctest.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <chrono>
#include <cstdio>
#include <functional>
#include <string>
#include <thread>
#include <vector>

#include "fake_openvr.h"
#include "openvr_driver.h"
#include "protocol.h"

using namespace std::chrono_literals;
namespace proto = vrdummy::protocol;

namespace {

bool WaitFor(const std::function<bool()>& cond, std::chrono::milliseconds timeout) {
    const auto deadline = std::chrono::steady_clock::now() + timeout;
    while (std::chrono::steady_clock::now() < deadline) {
        if (cond()) return true;
        std::this_thread::sleep_for(5ms);
    }
    return cond();
}

// Synchronous pipe client used by the test. Reads only bytes that
// PeekNamedPipe reports so that nothing blocks.
class PipeClient {
public:
    ~PipeClient() { Close(); }

    bool Open(const std::string& pipe_name, std::chrono::milliseconds timeout) {
        const std::string path = "\\\\.\\pipe\\" + pipe_name;
        const auto deadline = std::chrono::steady_clock::now() + timeout;
        while (std::chrono::steady_clock::now() < deadline) {
            handle_ = CreateFileA(path.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr, OPEN_EXISTING, 0, nullptr);
            if (handle_ != INVALID_HANDLE_VALUE) return true;
            std::this_thread::sleep_for(20ms);
        }
        return false;
    }
    void Close() {
        if (handle_ != INVALID_HANDLE_VALUE) {
            CloseHandle(handle_);
            handle_ = INVALID_HANDLE_VALUE;
        }
    }
    bool Send(const proto::Message& m) {
        const std::vector<uint8_t> frame = proto::Encode(m);
        DWORD written = 0;
        return WriteFile(handle_, frame.data(), static_cast<DWORD>(frame.size()), &written, nullptr) &&
               written == frame.size();
    }
    // Waits for a message satisfying pred; other messages are kept in `others`.
    std::optional<proto::Message> WaitMessage(const std::function<bool(const proto::Message&)>& pred,
                                              std::chrono::milliseconds timeout) {
        const auto deadline = std::chrono::steady_clock::now() + timeout;
        while (std::chrono::steady_clock::now() < deadline) {
            // Frames already buffered from a previous read come first.
            while (auto frame = decoder_.NextFrame()) {
                if (frame->status != proto::DecodeStatus::Ok || !frame->message) continue;
                if (pred(*frame->message)) return frame->message;
                others.push_back(*frame->message);
            }
            DWORD avail = 0;
            if (!PeekNamedPipe(handle_, nullptr, 0, nullptr, &avail, nullptr)) return std::nullopt;
            if (avail > 0) {
                std::vector<uint8_t> buf(avail);
                DWORD read = 0;
                if (!ReadFile(handle_, buf.data(), avail, &read, nullptr)) return std::nullopt;
                decoder_.Push(buf.data(), read);
                continue;
            }
            std::this_thread::sleep_for(5ms);
        }
        return std::nullopt;
    }

    std::vector<proto::Message> others;

private:
    HANDLE handle_ = INVALID_HANDLE_VALUE;
    proto::Decoder decoder_;
};

struct LoadedDriver {
    HMODULE module = nullptr;
    vr::IServerTrackedDeviceProvider* provider = nullptr;

    ~LoadedDriver() {
        if (module) FreeLibrary(module);
    }
    bool Load() {
        module = LoadLibraryA(VRDUMMY_DRIVER_DLL);
        if (!module) {
            std::fprintf(stderr, "LoadLibrary(%s) failed: %lu\n", VRDUMMY_DRIVER_DLL, GetLastError());
            return false;
        }
        using Factory = void* (*)(const char*, int*);
        auto factory = reinterpret_cast<Factory>(GetProcAddress(module, "HmdDriverFactory"));
        if (!factory) return false;
        int code = 0;
        provider = static_cast<vr::IServerTrackedDeviceProvider*>(factory(vr::IServerTrackedDeviceProvider_Version, &code));
        return provider != nullptr && code == 0;
    }
};

std::string UniquePipeName() { return "vrdummy-host-test-" + std::to_string(GetCurrentProcessId()); }

void ConfigureSettings(fake::Settings& s, const std::string& pipe_name, int32_t trackers) {
    s.Set("driver_vrdummy", "hmd_enable", true);
    s.Set("driver_vrdummy", "controllers_enable", true);
    s.Set("driver_vrdummy", "tracker_count", trackers);
    s.Set("driver_vrdummy", "pose_rate_hz", 200);
    s.Set("driver_vrdummy", "pipe_name", pipe_name);
    s.Set("driver_vrdummy", "serial_prefix", std::string("TEST"));
    s.Set("driver_vrdummy", "log_level", std::string("debug"));
    s.Set("vrdummy_display", "ipd_meters", 0.064f);
    s.Set("vrdummy_display", "render_width", 1440);
    s.Set("vrdummy_display", "render_height", 1600);
}

}  // namespace

TEST_CASE("driver loads, registers devices and serves the feeder pipe") {
    LoadedDriver driver;
    REQUIRE(driver.Load());

    fake::DriverContext ctx;
    const std::string pipe_name = UniquePipeName();
    ConfigureSettings(ctx.settings, pipe_name, 2);

    REQUIRE(driver.provider->Init(&ctx) == vr::VRInitError_None);
    CHECK(ctx.log.Contains("driver_vrdummy starting"));

    SUBCASE("devices and properties") {
        REQUIRE(ctx.host.devices.size() == 5);
        CHECK(ctx.host.devices[0].serial == "TEST-HMD-0");
        CHECK(ctx.host.devices[0].device_class == vr::TrackedDeviceClass_HMD);
        CHECK(ctx.host.devices[1].serial == "TEST-CTRL-L");
        CHECK(ctx.host.devices[1].device_class == vr::TrackedDeviceClass_Controller);
        CHECK(ctx.host.devices[2].serial == "TEST-CTRL-R");
        CHECK(ctx.host.devices[3].serial == "TEST-TRK-3");
        CHECK(ctx.host.devices[3].device_class == vr::TrackedDeviceClass_GenericTracker);
        CHECK(ctx.host.devices[4].serial == "TEST-TRK-4");

        CHECK(ctx.properties.GetString(0, vr::Prop_SerialNumber_String) == "TEST-HMD-0");
        CHECK(ctx.properties.GetFloat(0, vr::Prop_UserIpdMeters_Float) == doctest::Approx(0.064f));
        CHECK(ctx.properties.GetBool(0, vr::Prop_ContainsProximitySensor_Bool));
        CHECK(ctx.properties.GetString(0, vr::Prop_InputProfilePath_String) == "{vrdummy}/input/vrdummy_hmd_profile.json");
        CHECK(ctx.properties.GetInt32(1, vr::Prop_ControllerRoleHint_Int32) == vr::TrackedControllerRole_LeftHand);
        CHECK(ctx.properties.GetInt32(2, vr::Prop_ControllerRoleHint_Int32) == vr::TrackedControllerRole_RightHand);
        CHECK(ctx.properties.GetString(2, vr::Prop_ControllerType_String) == "vrdummy_controller");
        CHECK(ctx.properties.GetString(3, vr::Prop_ControllerType_String) == "vrdummy_tracker");

        // 15 input components plus the haptic output on each controller.
        CHECK(ctx.input.CountFor(1) == 16);
        CHECK(ctx.input.CountFor(2) == 16);
        CHECK(ctx.input.Find(2, "/input/joystick/x").kind == fake::DriverInput::Component::Scalar);
        CHECK(ctx.input.Find(0, "/proximity").bool_value);  // reported as worn

        // The HMD exposes a display component.
        void* display = ctx.host.devices[0].driver->GetComponent(vr::IVRDisplayComponent_Version);
        REQUIRE(display != nullptr);
        uint32_t w = 0, h = 0;
        static_cast<vr::IVRDisplayComponent*>(display)->GetRecommendedRenderTargetSize(&w, &h);
        CHECK(w == 1440);
        CHECK(h == 1600);
    }

    SUBCASE("pose thread submits poses without a feeder") {
        REQUIRE(WaitFor([&] { return ctx.host.Updates(0) >= 3 && ctx.host.Updates(4) >= 3; }, 1000ms));
        vr::DriverPose_t hmd{};
        REQUIRE(ctx.host.GetPose(0, &hmd));
        CHECK(hmd.poseIsValid);
        CHECK(hmd.vecPosition[1] == doctest::Approx(1.6));
        CHECK(hmd.result == vr::TrackingResult_Running_OK);
        vr::DriverPose_t trk{};
        REQUIRE(ctx.host.GetPose(3, &trk));
        CHECK_FALSE(trk.poseIsValid);
        CHECK(trk.result == vr::TrackingResult_Running_OutOfRange);
    }

    SUBCASE("feeder session over the pipe") {
        PipeClient client;
        REQUIRE(client.Open(pipe_name, 2000ms));
        REQUIRE(client.Send(proto::Message{proto::Hello{}}));
        auto ack = client.WaitMessage([](const proto::Message& m) { return std::holds_alternative<proto::HelloAck>(m); }, 2000ms);
        REQUIRE(ack.has_value());
        const auto& devices = std::get<proto::HelloAck>(*ack).devices;
        REQUIRE(devices.size() == 5);
        CHECK(devices[0].device_id == 0);
        CHECK(devices[0].device_class == proto::DeviceClass::Hmd);
        CHECK(devices[3].device_id == 3);
        CHECK(devices[3].serial == "TEST-TRK-3");
        REQUIRE(WaitFor([&] { return ctx.log.Contains("feeder handshake complete"); }, 1000ms));

        // Log records are forwarded to the feeder.
        auto log = client.WaitMessage([](const proto::Message& m) {
            return std::holds_alternative<proto::LogRecord>(m) &&
                   std::get<proto::LogRecord>(m).text.find("handshake complete") != std::string::npos;
        }, 1000ms);
        CHECK(log.has_value());

        // Pose for tracker 0 (device id 3, OpenVR index 3).
        proto::Pose pose;
        pose.device_id = 3;
        pose.flags.bits = proto::PoseFlags::kPoseValid | proto::PoseFlags::kConnected;
        pose.tracking_result = proto::TrackingResult::RunningOk;
        pose.position = {0.1, 0.95, -0.2};
        pose.rotation = {0.9238795, 0.0, 0.3826834, 0.0};
        REQUIRE(client.Send(proto::Message{pose}));
        vr::DriverPose_t got{};
        REQUIRE(WaitFor([&] { return ctx.host.GetPose(3, &got) && got.poseIsValid; }, 1000ms));
        CHECK(got.vecPosition[0] == doctest::Approx(0.1));
        CHECK(got.vecPosition[1] == doctest::Approx(0.95));
        CHECK(got.vecPosition[2] == doctest::Approx(-0.2));
        CHECK(got.qRotation.y == doctest::Approx(0.3826834));
        CHECK(got.result == vr::TrackingResult_Running_OK);
        CHECK_FALSE(got.shouldApplyHeadModel);

        // Input on the right controller: a/click (bool id 1), joystick/x (scalar id 11).
        proto::Input in;
        in.device_id = 2;
        in.component_id = 1;
        in.kind = proto::InputKind::Bool;
        in.value = 1.0f;
        REQUIRE(client.Send(proto::Message{in}));
        in.component_id = 11;
        in.kind = proto::InputKind::Scalar;
        in.value = -0.5f;
        REQUIRE(client.Send(proto::Message{in}));
        REQUIRE(WaitFor([&] { return ctx.input.Find(2, "/input/joystick/x").updates > 0; }, 1000ms));
        CHECK(ctx.input.Find(2, "/input/a/click").bool_value);
        CHECK(ctx.input.Find(2, "/input/joystick/x").scalar_value == doctest::Approx(-0.5f));

        // Display: IPD property and projection with swapped eyes.
        proto::Display d;
        d.flags.bits = proto::DisplayFlags::kIpd | proto::DisplayFlags::kProjection | proto::DisplayFlags::kSwapEyes;
        d.ipd_m = 0.07f;
        d.projection_left = {-1.2f, 0.8f, -1.0f, 1.0f};
        d.projection_right = {-0.8f, 1.2f, -1.0f, 1.0f};
        d.swap_eyes = true;
        REQUIRE(client.Send(proto::Message{d}));
        REQUIRE(WaitFor([&] { return ctx.properties.GetFloat(0, vr::Prop_UserIpdMeters_Float) > 0.069f; }, 1000ms));
        auto* display = static_cast<vr::IVRDisplayComponent*>(
            ctx.host.devices[0].driver->GetComponent(vr::IVRDisplayComponent_Version));
        float l = 0, r = 0, t = 0, b = 0;
        display->GetProjectionRaw(vr::Eye_Left, &l, &r, &t, &b);
        CHECK(l == doctest::Approx(-0.8f));  // swapped: left eye shows the right projection
        CHECK(r == doctest::Approx(1.2f));
        uint32_t x = 0, y = 0, w = 0, h = 0;
        display->GetEyeOutputViewport(vr::Eye_Left, &x, &y, &w, &h);
        CHECK(x == w);  // left eye drawn on the right half

        // Disconnect: trackers lose tracking, the HMD keeps its pose.
        client.Close();
        REQUIRE(WaitFor([&] { return ctx.host.GetPose(3, &got) && got.result == vr::TrackingResult_Running_OutOfRange; },
                        2000ms));
        CHECK(got.vecPosition[1] == doctest::Approx(0.95));  // last position kept
        vr::DriverPose_t hmd{};
        REQUIRE(ctx.host.GetPose(0, &hmd));
        CHECK(hmd.poseIsValid);
        REQUIRE(WaitFor([&] { return ctx.log.Contains("client disconnected"); }, 1000ms));

        // A second feeder can connect again.
        PipeClient again;
        REQUIRE(again.Open(pipe_name, 2000ms));
        REQUIRE(again.Send(proto::Message{proto::Hello{}}));
        auto ack2 = again.WaitMessage([](const proto::Message& m) { return std::holds_alternative<proto::HelloAck>(m); }, 2000ms);
        CHECK(ack2.has_value());
        REQUIRE(again.Send(proto::Message{proto::Goodbye{}}));
        REQUIRE(WaitFor([&] { return ctx.log.Contains("feeder sent goodbye"); }, 1000ms));
    }

    driver.provider->Cleanup();
    CHECK(ctx.log.Contains("cleanup"));
}
