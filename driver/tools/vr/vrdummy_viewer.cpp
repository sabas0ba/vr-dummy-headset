// VR scene application that shows every tracked device, the hand inputs and
// the floor grid in the headset (visible through SteamVR's VR View), and
// prints the same information to the console.
//
//   vrdummy_viewer [--interval-ms N] [--frames N]
//
// Standing and seated universes are set to the raw tracking space on start
// (docs/coordinates.md). The action manifest (actions.json) next to the
// executable provides the vrdummy_controller bindings.
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

#include "openvr.h"
#include "renderer.h"
#include "scene.h"

using namespace vrscene;

namespace {

volatile bool g_stop = false;

BOOL WINAPI OnCtrlC(DWORD) {
    g_stop = true;
    return TRUE;
}

std::string ExeDir() {
    char exe[MAX_PATH] = {};
    GetModuleFileNameA(nullptr, exe, MAX_PATH);
    std::string s = exe;
    return s.substr(0, s.find_last_of("\\/") + 1);
}

void PrintState(vr::IVRSystem* sys, const vr::TrackedDevicePose_t* poses, const HandInput& left, const HandInput& right,
                uint64_t frame) {
    std::printf("---- frame %llu  (pos m, euler deg: yaw pitch roll)\n", static_cast<unsigned long long>(frame));
    for (uint32_t i = 0; i < vr::k_unMaxTrackedDeviceCount; ++i) {
        const auto cls = sys->GetTrackedDeviceClass(i);
        if (cls == vr::TrackedDeviceClass_Invalid) continue;
        const auto& p = poses[i];
        const Vec3 pos = Mat4::From34(p.mDeviceToAbsoluteTracking).Position();
        const Euler e = EulerDeg(p.mDeviceToAbsoluteTracking);
        std::printf("%2u %-4s %-16s %-8s pos=[%7.3f %7.3f %7.3f] euler=[%7.1f %7.1f %7.1f]\n", i, ClassName(cls),
                    StringProp(sys, i, vr::Prop_SerialNumber_String).c_str(), p.bPoseIsValid ? ResultName(p.eTrackingResult) : "invalid",
                    static_cast<double>(pos.x), static_cast<double>(pos.y), static_cast<double>(pos.z), e.yaw, e.pitch, e.roll);
    }
    for (const auto& [name, h] : {std::pair{"left ", left}, std::pair{"right", right}}) {
        std::printf("   %s active=%d a=%d b=%d js_click=%d trigger=%.2f grip=%.2f joystick=[%5.2f %5.2f]\n", name, h.active ? 1 : 0,
                    h.a ? 1 : 0, h.b ? 1 : 0, h.joystick_click ? 1 : 0, static_cast<double>(h.trigger), static_cast<double>(h.grip),
                    static_cast<double>(h.joystick_x), static_cast<double>(h.joystick_y));
    }
    const auto l = sys->GetEyeToHeadTransform(vr::Eye_Left);
    const auto r = sys->GetEyeToHeadTransform(vr::Eye_Right);
    float pl[4], pr[4];
    sys->GetProjectionRaw(vr::Eye_Left, &pl[0], &pl[1], &pl[2], &pl[3]);
    sys->GetProjectionRaw(vr::Eye_Right, &pr[0], &pr[1], &pr[2], &pr[3]);
    std::printf("   eyes: ipd=%.4f  projL=[%.2f %.2f %.2f %.2f] projR=[%.2f %.2f %.2f %.2f] dashboard=%d input=%d\n",
                static_cast<double>(r.m[0][3] - l.m[0][3]), static_cast<double>(pl[0]), static_cast<double>(pl[1]), static_cast<double>(pl[2]),
                static_cast<double>(pl[3]), static_cast<double>(pr[0]), static_cast<double>(pr[1]), static_cast<double>(pr[2]),
                static_cast<double>(pr[3]), vr::VROverlay()->IsDashboardVisible() ? 1 : 0, sys->IsInputAvailable() ? 1 : 0);
    std::fflush(stdout);
}

}  // namespace

int main(int argc, char** argv) {
    int interval_ms = 500;
    long long max_frames = -1;
    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], "--interval-ms") == 0 && i + 1 < argc) interval_ms = std::atoi(argv[++i]);
        else if (std::strcmp(argv[i], "--frames") == 0 && i + 1 < argc) max_frames = std::atoll(argv[++i]);
        else {
            std::fprintf(stderr, "usage: vrdummy_viewer [--interval-ms N] [--frames N]\n");
            return 2;
        }
    }
    SetConsoleCtrlHandler(OnCtrlC, TRUE);

    vr::EVRInitError err = vr::VRInitError_None;
    vr::IVRSystem* sys = vr::VR_Init(&err, vr::VRApplication_Scene);
    if (err != vr::VRInitError_None) {
        std::fprintf(stderr, "VR_Init failed: %s\n", vr::VR_GetVRInitErrorAsEnglishDescription(err));
        return 1;
    }
    if (!SetupChaperoneIdentity()) std::fprintf(stderr, "warning: chaperone setup not committed\n");
    vr::VRCompositor()->SetTrackingSpace(vr::TrackingUniverseStanding);

    ActionSet actions;
    if (!actions.Init(ExeDir() + "actions.json")) std::fprintf(stderr, "warning: action manifest not loaded\n");

    Renderer renderer;
    if (!renderer.Init(sys)) {
        vr::VR_Shutdown();
        return 1;
    }
    std::printf("vrdummy_viewer: rendering %ux%u per eye; Ctrl-C to stop\n", renderer.width(), renderer.height());

    vr::TrackedDevicePose_t poses[vr::k_unMaxTrackedDeviceCount];
    Lines lines;
    uint64_t frame = 0;
    auto last_print = std::chrono::steady_clock::now() - std::chrono::seconds(10);
    while (!g_stop && (max_frames < 0 || static_cast<long long>(frame) < max_frames)) {
        vr::VRCompositor()->WaitGetPoses(poses, vr::k_unMaxTrackedDeviceCount, nullptr, 0);
        actions.Update();
        const HandInput left = actions.Read(actions.left);
        const HandInput right = actions.Read(actions.right);
        vr::VREvent_t ev{};
        while (sys->PollNextEvent(&ev, sizeof(ev))) {
            if (ev.eventType == vr::VREvent_Quit) {
                sys->AcknowledgeQuit_Exiting();
                g_stop = true;
            }
        }
        lines.v.clear();
        BuildScene(lines, sys, poses, left, right, vr::k_unTrackedDeviceIndex_Hmd);
        renderer.RenderAndSubmit(sys, poses[vr::k_unTrackedDeviceIndex_Hmd].mDeviceToAbsoluteTracking, lines);
        ++frame;
        const auto now = std::chrono::steady_clock::now();
        if (now - last_print >= std::chrono::milliseconds(interval_ms)) {
            PrintState(sys, poses, left, right, frame);
            last_print = now;
        }
    }
    vr::VR_Shutdown();
    return 0;
}
