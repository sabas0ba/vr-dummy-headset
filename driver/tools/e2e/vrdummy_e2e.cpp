// End-to-end scenarios against a running SteamVR with driver_vrdummy loaded
// and the feeder (`vrdummy-cli run`) serving its control pipe.
//
// The program is a VR scene application: it initialises OpenVR, loads an
// action manifest whose bindings target vrdummy_controller, drives the feeder
// through the control pipe, and checks what OpenVR reports back (poses,
// controller roles, action states, eye transforms).
//
// Environment: VRDUMMY_CONTROL_PIPE (default vrdummy-control).
#define DOCTEST_CONFIG_IMPLEMENT
#include "doctest.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <functional>
#include <string>
#include <thread>
#include <vector>

#include "openvr.h"
#include "renderer.h"
#include "scene.h"

using namespace std::chrono_literals;

namespace {

// ---------------------------------------------------------------------------
// Control pipe client (one connection per command, like vrdummy-cli set).

std::string ControlPipePath() {
    char name[256] = {};
    const DWORD n = GetEnvironmentVariableA("VRDUMMY_CONTROL_PIPE", name, sizeof(name));
    return std::string("\\\\.\\pipe\\") + (n > 0 && n < sizeof(name) ? name : "vrdummy-control");
}

// Sends one command line and returns the reply line ("ok ...", "err ...").
std::string Control(const std::string& line) {
    const std::string path = ControlPipePath();
    HANDLE h = INVALID_HANDLE_VALUE;
    const auto deadline = std::chrono::steady_clock::now() + 3s;
    while (std::chrono::steady_clock::now() < deadline) {
        h = CreateFileA(path.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr, OPEN_EXISTING, 0, nullptr);
        if (h != INVALID_HANDLE_VALUE) break;
        const DWORD err = GetLastError();
        if (err == ERROR_PIPE_BUSY) {
            WaitNamedPipeA(path.c_str(), 200);
        } else {
            std::this_thread::sleep_for(50ms);
        }
    }
    if (h == INVALID_HANDLE_VALUE) return "err control pipe not available";
    const std::string out = line + "\n";
    DWORD written = 0;
    WriteFile(h, out.data(), static_cast<DWORD>(out.size()), &written, nullptr);
    std::string reply;
    char c = 0;
    DWORD read = 0;
    while (ReadFile(h, &c, 1, &read, nullptr) && read == 1) {
        if (c == '\n') break;
        reply.push_back(c);
    }
    CloseHandle(h);
    return reply;
}

bool Cmd(const std::string& line) {
    const std::string reply = Control(line);
    const bool ok = reply.rfind("ok", 0) == 0;
    if (!ok) MESSAGE("control: ", line, " -> ", reply);
    return ok;
}

// ---------------------------------------------------------------------------
// OpenVR session shared by all scenarios.

struct Vec3 {
    double x = 0, y = 0, z = 0;
};

Vec3 Position(const vr::HmdMatrix34_t& m) { return {m.m[0][3], m.m[1][3], m.m[2][3]}; }

// Yaw in degrees about +Y for a rotation matrix (SteamVR convention: -Z forward).
double YawDeg(const vr::HmdMatrix34_t& m) { return std::atan2(m.m[0][2], m.m[2][2]) * 180.0 / 3.14159265358979323846; }

struct Session {
    vr::IVRSystem* sys = nullptr;
    vrscene::ActionSet actions;
    vrscene::Renderer renderer;
    vr::TrackedDevicePose_t poses[vr::k_unMaxTrackedDeviceCount] = {};
    vrscene::Lines lines;

    bool Init() {
        vr::EVRInitError err = vr::VRInitError_None;
        sys = vr::VR_Init(&err, vr::VRApplication_Scene);
        if (err != vr::VRInitError_None) {
            std::fprintf(stderr, "VR_Init failed: %s\n", vr::VR_GetVRInitErrorAsEnglishDescription(err));
            return false;
        }
        if (!vrscene::SetupChaperoneIdentity()) std::fprintf(stderr, "warning: chaperone setup not committed\n");
        vr::VRCompositor()->SetTrackingSpace(vr::TrackingUniverseStanding);
        char exe[MAX_PATH] = {};
        GetModuleFileNameA(nullptr, exe, MAX_PATH);
        std::string manifest = exe;
        manifest = manifest.substr(0, manifest.find_last_of("\\/") + 1) + "actions.json";
        if (!actions.Init(manifest)) {
            std::fprintf(stderr, "SetActionManifestPath failed: %s\n", manifest.c_str());
            return false;
        }
        if (!renderer.Init(sys)) return false;
        return true;
    }
    void Shutdown() {
        if (sys) vr::VR_Shutdown();
        sys = nullptr;
    }

    std::string Serial(uint32_t i) { return vrscene::StringProp(sys, i, vr::Prop_SerialNumber_String); }
    int Find(const std::string& serial) {
        for (uint32_t i = 0; i < vr::k_unMaxTrackedDeviceCount; ++i) {
            if (sys->GetTrackedDeviceClass(i) != vr::TrackedDeviceClass_Invalid && Serial(i) == serial) return static_cast<int>(i);
        }
        return -1;
    }
    vr::TrackedDevicePose_t Pose(uint32_t index) {
        vr::TrackedDevicePose_t all[vr::k_unMaxTrackedDeviceCount];
        sys->GetDeviceToAbsoluteTrackingPose(vr::TrackingUniverseStanding, 0.0f, all, vr::k_unMaxTrackedDeviceCount);
        return all[index];
    }
    // One application frame: poses, input, scene rendering and submission.
    // Rendering keeps this process the focused scene application.
    void Update() {
        vr::VRCompositor()->WaitGetPoses(poses, vr::k_unMaxTrackedDeviceCount, nullptr, 0);
        actions.Update();
        vr::VREvent_t ev{};
        while (sys->PollNextEvent(&ev, sizeof(ev))) {
        }
        lines.v.clear();
        vrscene::BuildScene(lines, sys, poses, actions.Read(actions.left), actions.Read(actions.right), vr::k_unTrackedDeviceIndex_Hmd);
        renderer.RenderAndSubmit(sys, poses[vr::k_unTrackedDeviceIndex_Hmd].mDeviceToAbsoluteTracking, lines);
    }
    vr::InputDigitalActionData_t Digital(vr::VRActionHandle_t action, vr::VRInputValueHandle_t device) {
        vr::InputDigitalActionData_t d{};
        vr::VRInput()->GetDigitalActionData(action, &d, sizeof(d), device);
        return d;
    }
    vr::InputAnalogActionData_t Analog(vr::VRActionHandle_t action, vr::VRInputValueHandle_t device) {
        vr::InputAnalogActionData_t d{};
        vr::VRInput()->GetAnalogActionData(action, &d, sizeof(d), device);
        return d;
    }
    // Closes the dashboard with the right controller's system button so that
    // input reaches the scene application.
    bool EnsureInputFocus();
};

Session g_session;

bool WaitFor(const std::function<bool()>& cond, std::chrono::milliseconds timeout = 2000ms) {
    const auto deadline = std::chrono::steady_clock::now() + timeout;
    while (std::chrono::steady_clock::now() < deadline) {
        g_session.Update();
        if (cond()) return true;
        std::this_thread::sleep_for(20ms);
    }
    g_session.Update();
    return cond();
}

bool Session::EnsureInputFocus() {
    for (int attempt = 0; attempt < 3 && !sys->IsInputAvailable(); ++attempt) {
        if (vr::VROverlay()->IsDashboardVisible()) {
            Cmd("input ctrl-r system/click 1");
            std::this_thread::sleep_for(150ms);
            Cmd("input ctrl-r system/click 0");
        }
        WaitFor([this] { return sys->IsInputAvailable(); }, 3000ms);
    }
    return sys->IsInputAvailable();
}

bool Near(const Vec3& p, double x, double y, double z, double tol = 0.02) {
    return std::fabs(p.x - x) <= tol && std::fabs(p.y - y) <= tol && std::fabs(p.z - z) <= tol;
}

bool PoseAt(uint32_t index, double x, double y, double z, double tol = 0.02) {
    return WaitFor([&] {
        const auto p = g_session.Pose(index);
        return p.bPoseIsValid && p.eTrackingResult == vr::TrackingResult_Running_OK &&
               Near(Position(p.mDeviceToAbsoluteTracking), x, y, z, tol);
    });
}

struct Devices {
    int hmd = -1, left = -1, right = -1, trk0 = -1, trk1 = -1, trk2 = -1;
};

Devices FindDevices() {
    Devices d;
    d.hmd = g_session.Find("VRDUMMY-HMD-0");
    d.left = g_session.Find("VRDUMMY-CTRL-L");
    d.right = g_session.Find("VRDUMMY-CTRL-R");
    d.trk0 = g_session.Find("VRDUMMY-TRK-3");
    d.trk1 = g_session.Find("VRDUMMY-TRK-4");
    d.trk2 = g_session.Find("VRDUMMY-TRK-5");
    return d;
}

void ResetScene() {
    Cmd("preset stand 1.7");
    for (const char* dev : {"hmd", "ctrl-l", "ctrl-r", "trk0", "trk1", "trk2"}) Cmd(std::string("euler ") + dev + " 0 0 0");
    for (const char* hand : {"ctrl-l", "ctrl-r"}) {
        for (const char* comp : {"a/click", "b/click", "trigger/click", "grip/click", "joystick/click"}) {
            Cmd(std::string("input ") + hand + " " + comp + " 0");
        }
        for (const char* comp : {"trigger/value", "grip/value", "joystick/x", "joystick/y"}) {
            Cmd(std::string("input ") + hand + " " + comp + " 0");
        }
    }
    Cmd("ipd 0.064");
    Cmd("proj -1 1 -1 1 -1 1 -1 1");
    Cmd("swap 0");
}

}  // namespace

// ---------------------------------------------------------------------------

TEST_CASE("S1 enumeration: devices, classes and hand roles") {
    REQUIRE(WaitFor([] { return FindDevices().hmd >= 0; }, 5000ms));
    const Devices d = FindDevices();
    REQUIRE(d.hmd >= 0);
    REQUIRE(d.left >= 0);
    REQUIRE(d.right >= 0);
    REQUIRE(d.trk0 >= 0);
    CHECK(d.trk1 >= 0);
    CHECK(d.trk2 >= 0);
    auto* sys = g_session.sys;
    CHECK(sys->GetTrackedDeviceClass(static_cast<uint32_t>(d.hmd)) == vr::TrackedDeviceClass_HMD);
    CHECK(sys->GetTrackedDeviceClass(static_cast<uint32_t>(d.left)) == vr::TrackedDeviceClass_Controller);
    CHECK(sys->GetTrackedDeviceClass(static_cast<uint32_t>(d.trk0)) == vr::TrackedDeviceClass_GenericTracker);
    CHECK(sys->GetTrackedDeviceIndexForControllerRole(vr::TrackedControllerRole_LeftHand) == static_cast<uint32_t>(d.left));
    CHECK(sys->GetTrackedDeviceIndexForControllerRole(vr::TrackedControllerRole_RightHand) == static_cast<uint32_t>(d.right));
    CHECK(Control("status").rfind("ok connected=true", 0) == 0);
}

TEST_CASE("S2 static poses: position and yaw of every device") {
    ResetScene();
    const Devices d = FindDevices();
    REQUIRE(d.hmd >= 0);
    REQUIRE(Cmd("pos hmd 0.25 1.7 -0.5"));
    REQUIRE(Cmd("euler hmd 90 0 0"));
    REQUIRE(Cmd("pos ctrl-l -0.3 1.1 -0.4"));
    REQUIRE(Cmd("pos ctrl-r 0.3 1.2 -0.4"));
    REQUIRE(Cmd("pos trk0 0.0 0.9 0.1"));
    REQUIRE(Cmd("pos trk1 -0.1 0.05 0.0"));
    REQUIRE(Cmd("pos trk2 0.1 0.05 0.0"));
    CHECK(PoseAt(static_cast<uint32_t>(d.hmd), 0.25, 1.7, -0.5));
    CHECK(PoseAt(static_cast<uint32_t>(d.left), -0.3, 1.1, -0.4));
    CHECK(PoseAt(static_cast<uint32_t>(d.right), 0.3, 1.2, -0.4));
    CHECK(PoseAt(static_cast<uint32_t>(d.trk0), 0.0, 0.9, 0.1));
    CHECK(PoseAt(static_cast<uint32_t>(d.trk1), -0.1, 0.05, 0.0));
    CHECK(PoseAt(static_cast<uint32_t>(d.trk2), 0.1, 0.05, 0.0));
    const double yaw = YawDeg(g_session.Pose(static_cast<uint32_t>(d.hmd)).mDeviceToAbsoluteTracking);
    CHECK(yaw == doctest::Approx(90.0).epsilon(0.02));
}

TEST_CASE("S3 motion: the HMD follows a continuous path") {
    ResetScene();
    const Devices d = FindDevices();
    REQUIRE(d.hmd >= 0);
    const auto hmd = static_cast<uint32_t>(d.hmd);
    REQUIRE(PoseAt(hmd, 0.0, 1.581, 0.0, 0.03));
    std::vector<double> samples;
    for (int step = 1; step <= 20; ++step) {
        const double x = 0.05 * step;
        REQUIRE(Cmd("pos hmd " + std::to_string(x) + " 1.6 0"));
        std::this_thread::sleep_for(50ms);
        samples.push_back(Position(g_session.Pose(hmd).mDeviceToAbsoluteTracking).x);
    }
    CHECK(PoseAt(hmd, 1.0, 1.6, 0.0));
    // Samples never move backwards and cover the path.
    double prev = -1.0;
    int advances = 0;
    for (double x : samples) {
        CHECK(x >= prev - 0.001);
        if (x > prev + 0.01) ++advances;
        prev = x;
    }
    CHECK(advances >= 10);
}

TEST_CASE("S4 input: buttons, triggers and joystick reach the action system") {
    ResetScene();
    const Devices d = FindDevices();
    REQUIRE(d.right >= 0);
    CHECK_MESSAGE(g_session.EnsureInputFocus(), "scene application has input focus (dashboard closed)");
    const bool active = WaitFor([] { return g_session.Digital(g_session.actions.a, g_session.actions.right).bActive; }, 5000ms);
    if (!active) {
        vr::InputDigitalActionData_t any{};
        const auto e_any = vr::VRInput()->GetDigitalActionData(g_session.actions.a, &any, sizeof(any), vr::k_ulInvalidInputValueHandle);
        vr::InputDigitalActionData_t r{};
        const auto e_right = vr::VRInput()->GetDigitalActionData(g_session.actions.a, &r, sizeof(r), g_session.actions.right);
        vr::VRInputValueHandle_t origins[8] = {};
        const auto e_orig = vr::VRInput()->GetActionOrigins(g_session.actions.set, g_session.actions.a, origins, 8);
        char scene_key[256] = {};
        vr::VRApplications()->GetApplicationKeyByProcessId(vr::VRApplications()->GetCurrentSceneProcessId(), scene_key,
                                                           sizeof(scene_key));
        MESSAGE("dashboard=", vr::VROverlay()->IsDashboardVisible(), " scene_pid=",
                vr::VRApplications()->GetCurrentSceneProcessId(), " scene_key=", scene_key, " my_pid=",
                GetCurrentProcessId(), " scene_state=", static_cast<int>(vr::VRApplications()->GetSceneApplicationState()));
        MESSAGE("IsInputAvailable=", g_session.sys->IsInputAvailable(), " set=", g_session.actions.set, " action=", g_session.actions.a,
                " right=", g_session.actions.right, " any: err=", static_cast<int>(e_any), " active=", any.bActive,
                " restricted: err=", static_cast<int>(e_right), " active=", r.bActive, " origins: err=",
                static_cast<int>(e_orig), " first=", origins[0]);
    }
    REQUIRE(active);

    REQUIRE(Cmd("input ctrl-r a/click 1"));
    CHECK(WaitFor([] { return g_session.Digital(g_session.actions.a, g_session.actions.right).bState; }));
    CHECK_FALSE(g_session.Digital(g_session.actions.a, g_session.actions.left).bState);
    REQUIRE(Cmd("input ctrl-r a/click 0"));
    CHECK(WaitFor([] { return !g_session.Digital(g_session.actions.a, g_session.actions.right).bState; }));

    REQUIRE(Cmd("input ctrl-l b/click 1"));
    CHECK(WaitFor([] { return g_session.Digital(g_session.actions.b, g_session.actions.left).bState; }));
    REQUIRE(Cmd("input ctrl-l b/click 0"));

    REQUIRE(Cmd("input ctrl-r trigger/value 0.75"));
    CHECK(WaitFor([] { return std::fabs(g_session.Analog(g_session.actions.trigger, g_session.actions.right).x - 0.75f) < 0.02f; }));
    REQUIRE(Cmd("input ctrl-l grip/value 0.4"));
    CHECK(WaitFor([] { return std::fabs(g_session.Analog(g_session.actions.grip, g_session.actions.left).x - 0.4f) < 0.02f; }));

    REQUIRE(Cmd("input ctrl-r joystick/x -0.5"));
    REQUIRE(Cmd("input ctrl-r joystick/y 0.25"));
    CHECK(WaitFor([] {
        const auto j = g_session.Analog(g_session.actions.joystick, g_session.actions.right);
        return std::fabs(j.x + 0.5f) < 0.02f && std::fabs(j.y - 0.25f) < 0.02f;
    }));
    REQUIRE(Cmd("input ctrl-r joystick/click 1"));
    CHECK(WaitFor([] { return g_session.Digital(g_session.actions.joystick_click, g_session.actions.right).bState; }));
    REQUIRE(Cmd("input ctrl-r joystick/click 0"));
    REQUIRE(Cmd("input ctrl-r joystick/x 0"));
    REQUIRE(Cmd("input ctrl-r joystick/y 0"));
    REQUIRE(Cmd("input ctrl-r trigger/value 0"));
    REQUIRE(Cmd("input ctrl-l grip/value 0"));
}

TEST_CASE("S5 display: IPD changes the eye transforms") {
    ResetScene();
    auto* sys = g_session.sys;
    REQUIRE(Cmd("ipd 0.07"));
    CHECK(WaitFor([&] {
        const auto l = sys->GetEyeToHeadTransform(vr::Eye_Left);
        const auto r = sys->GetEyeToHeadTransform(vr::Eye_Right);
        return std::fabs(l.m[0][3] + 0.035f) < 0.001f && std::fabs(r.m[0][3] - 0.035f) < 0.001f;
    }));
    REQUIRE(Cmd("ipd 0.064"));
    CHECK(WaitFor([&] { return std::fabs(sys->GetEyeToHeadTransform(vr::Eye_Left).m[0][3] + 0.032f) < 0.001f; }));
}

TEST_CASE("S6 tracking loss and recovery of a tracker") {
    ResetScene();
    const Devices d = FindDevices();
    REQUIRE(d.trk0 >= 0);
    const auto trk = static_cast<uint32_t>(d.trk0);
    REQUIRE(Cmd("pos trk0 0.2 0.9 0.0"));
    CHECK(PoseAt(trk, 0.2, 0.9, 0.0));
    REQUIRE(Cmd("valid trk0 0"));
    CHECK(WaitFor([&] {
        const auto p = g_session.Pose(trk);
        return !p.bPoseIsValid && p.eTrackingResult == vr::TrackingResult_Running_OutOfRange;
    }));
    REQUIRE(Cmd("valid trk0 1"));
    CHECK(PoseAt(trk, 0.2, 0.9, 0.0));
}

TEST_CASE("S7 scenario file replayed by the feeder") {
    ResetScene();
    const Devices d = FindDevices();
    REQUIRE(d.hmd >= 0);
    // The feeder's `scenario` subcommand is exercised by the wrapper script;
    // here the same timed steps are issued through the control pipe.
    REQUIRE(Cmd("pos hmd 0 1.6 0"));
    REQUIRE(Cmd("euler hmd 0 0 0"));
    REQUIRE(Cmd("move hmd 0 0 -1"));
    CHECK(PoseAt(static_cast<uint32_t>(d.hmd), 0.0, 1.6, -1.0));
    REQUIRE(Cmd("euler hmd -45 0 0"));
    CHECK(WaitFor([&] {
        return std::fabs(YawDeg(g_session.Pose(static_cast<uint32_t>(d.hmd)).mDeviceToAbsoluteTracking) + 45.0) < 1.0;
    }));
    REQUIRE(Cmd("preset sit 1.7"));
    CHECK(WaitFor([&] { return Position(g_session.Pose(static_cast<uint32_t>(d.hmd)).mDeviceToAbsoluteTracking).y < 1.3; }));
}

int main(int argc, char** argv) {
    doctest::Context context;
    context.applyCommandLine(argc, argv);
    if (context.shouldExit()) return context.run();
    if (!g_session.Init()) return 3;
    const int rc = context.run();
    ResetScene();
    g_session.Shutdown();
    return rc;
}
