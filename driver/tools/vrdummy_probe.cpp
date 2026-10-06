// OpenVR client probe for integration tests against a running SteamVR.
//
//   vrdummy_probe list                       print every tracked device
//   vrdummy_probe pose <serial>              print the pose of one device
//   vrdummy_probe expect <serial> x y z [tol]  exit 0 if the device is tracking
//                                            within tol (default 0.01 m)
//   vrdummy_probe wait <serial> [timeout_s]  wait until the device exists
//
// Runs as a background application, so SteamVR must already be running.
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <thread>

#include "openvr.h"

namespace {

const char* ClassName(vr::ETrackedDeviceClass c) {
    switch (c) {
        case vr::TrackedDeviceClass_HMD: return "hmd";
        case vr::TrackedDeviceClass_Controller: return "controller";
        case vr::TrackedDeviceClass_GenericTracker: return "tracker";
        case vr::TrackedDeviceClass_TrackingReference: return "reference";
        case vr::TrackedDeviceClass_DisplayRedirect: return "display_redirect";
        default: return "invalid";
    }
}

const char* ResultName(vr::ETrackingResult r) {
    switch (r) {
        case vr::TrackingResult_Uninitialized: return "uninitialized";
        case vr::TrackingResult_Calibrating_InProgress: return "calibrating";
        case vr::TrackingResult_Calibrating_OutOfRange: return "calibrating_out_of_range";
        case vr::TrackingResult_Running_OK: return "ok";
        case vr::TrackingResult_Running_OutOfRange: return "out_of_range";
        case vr::TrackingResult_Fallback_RotationOnly: return "rotation_only";
        default: return "?";
    }
}

std::string StringProp(vr::IVRSystem* sys, uint32_t index, vr::ETrackedDeviceProperty prop) {
    char buf[256] = {};
    vr::ETrackedPropertyError err = vr::TrackedProp_Success;
    sys->GetStringTrackedDeviceProperty(index, prop, buf, sizeof(buf), &err);
    return err == vr::TrackedProp_Success ? buf : "";
}

int FindBySerial(vr::IVRSystem* sys, const std::string& serial) {
    for (uint32_t i = 0; i < vr::k_unMaxTrackedDeviceCount; ++i) {
        if (sys->GetTrackedDeviceClass(i) == vr::TrackedDeviceClass_Invalid) continue;
        if (StringProp(sys, i, vr::Prop_SerialNumber_String) == serial) return static_cast<int>(i);
    }
    return -1;
}

void PrintDevice(vr::IVRSystem* sys, uint32_t i, const vr::TrackedDevicePose_t& pose) {
    const vr::HmdMatrix34_t& m = pose.mDeviceToAbsoluteTracking;
    std::printf("index=%u class=%s serial=%s type=%s connected=%d valid=%d result=%s pos=[%.4f %.4f %.4f]\n", i,
                ClassName(sys->GetTrackedDeviceClass(i)), StringProp(sys, i, vr::Prop_SerialNumber_String).c_str(),
                StringProp(sys, i, vr::Prop_ControllerType_String).c_str(), pose.bDeviceIsConnected ? 1 : 0,
                pose.bPoseIsValid ? 1 : 0, ResultName(pose.eTrackingResult), static_cast<double>(m.m[0][3]),
                static_cast<double>(m.m[1][3]), static_cast<double>(m.m[2][3]));
}

struct Session {
    vr::IVRSystem* sys = nullptr;
    ~Session() {
        if (sys) vr::VR_Shutdown();
    }
    bool Init() {
        vr::EVRInitError err = vr::VRInitError_None;
        sys = vr::VR_Init(&err, vr::VRApplication_Background);
        if (err != vr::VRInitError_None) {
            std::fprintf(stderr, "VR_Init failed: %s\n", vr::VR_GetVRInitErrorAsEnglishDescription(err));
            sys = nullptr;
            return false;
        }
        return true;
    }
    void Poses(vr::TrackedDevicePose_t* out) {
        sys->GetDeviceToAbsoluteTrackingPose(vr::TrackingUniverseStanding, 0.0f, out, vr::k_unMaxTrackedDeviceCount);
    }
};

int Usage() {
    std::fprintf(stderr, "usage: vrdummy_probe list | pose <serial> | expect <serial> x y z [tol] | wait <serial> [timeout_s]\n");
    return 2;
}

}  // namespace

int main(int argc, char** argv) {
    if (argc < 2) return Usage();
    const std::string cmd = argv[1];

    if (cmd == "wait") {
        if (argc < 3) return Usage();
        const double timeout = argc >= 4 ? std::atof(argv[3]) : 30.0;
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::duration<double>(timeout);
        while (std::chrono::steady_clock::now() < deadline) {
            Session s;
            if (s.Init() && FindBySerial(s.sys, argv[2]) >= 0) {
                std::printf("found %s\n", argv[2]);
                return 0;
            }
            std::this_thread::sleep_for(std::chrono::seconds(1));
        }
        std::fprintf(stderr, "timeout waiting for %s\n", argv[2]);
        return 1;
    }

    Session s;
    if (!s.Init()) return 1;
    vr::TrackedDevicePose_t poses[vr::k_unMaxTrackedDeviceCount];
    s.Poses(poses);

    if (cmd == "list") {
        for (uint32_t i = 0; i < vr::k_unMaxTrackedDeviceCount; ++i) {
            if (s.sys->GetTrackedDeviceClass(i) == vr::TrackedDeviceClass_Invalid) continue;
            PrintDevice(s.sys, i, poses[i]);
        }
        return 0;
    }
    if (argc < 3) return Usage();
    const int index = FindBySerial(s.sys, argv[2]);
    if (index < 0) {
        std::fprintf(stderr, "device not found: %s\n", argv[2]);
        return 1;
    }
    const auto ui = static_cast<uint32_t>(index);
    if (cmd == "pose") {
        PrintDevice(s.sys, ui, poses[ui]);
        return 0;
    }
    if (cmd == "expect") {
        if (argc < 6) return Usage();
        const double ex = std::atof(argv[3]), ey = std::atof(argv[4]), ez = std::atof(argv[5]);
        const double tol = argc >= 7 ? std::atof(argv[6]) : 0.01;
        PrintDevice(s.sys, ui, poses[ui]);
        const vr::HmdMatrix34_t& m = poses[ui].mDeviceToAbsoluteTracking;
        const double dx = std::fabs(m.m[0][3] - ex), dy = std::fabs(m.m[1][3] - ey), dz = std::fabs(m.m[2][3] - ez);
        if (!poses[ui].bPoseIsValid || poses[ui].eTrackingResult != vr::TrackingResult_Running_OK) {
            std::fprintf(stderr, "pose is not valid\n");
            return 1;
        }
        if (dx > tol || dy > tol || dz > tol) {
            std::fprintf(stderr, "position differs: d=[%.4f %.4f %.4f] tol=%.4f\n", dx, dy, dz, tol);
            return 1;
        }
        std::printf("match\n");
        return 0;
    }
    return Usage();
}
