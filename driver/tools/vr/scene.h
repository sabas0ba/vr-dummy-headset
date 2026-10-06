// Shared helpers for the OpenVR client tools (viewer, e2e): matrix math,
// chaperone setup, device table, input readout and a line-based scene.
#pragma once

#include <array>
#include <cmath>
#include <cstdint>
#include <string>
#include <vector>

#include "openvr.h"

namespace vrscene {

constexpr double kPi = 3.14159265358979323846;

struct Vec3 {
    float x = 0, y = 0, z = 0;
};

struct Color {
    float r = 1, g = 1, b = 1, a = 1;
};

// Column-vector 4x4 matrix: v' = M v. Memory is row-major (m[row][col]).
struct Mat4 {
    float m[4][4] = {{1, 0, 0, 0}, {0, 1, 0, 0}, {0, 0, 1, 0}, {0, 0, 0, 1}};

    static Mat4 Identity() { return Mat4{}; }
    static Mat4 From34(const vr::HmdMatrix34_t& a) {
        Mat4 r;
        for (int i = 0; i < 3; ++i)
            for (int j = 0; j < 4; ++j) r.m[i][j] = a.m[i][j];
        return r;
    }
    static Mat4 From44(const vr::HmdMatrix44_t& a) {
        Mat4 r;
        for (int i = 0; i < 4; ++i)
            for (int j = 0; j < 4; ++j) r.m[i][j] = a.m[i][j];
        return r;
    }
    Mat4 operator*(const Mat4& o) const {
        Mat4 r;
        for (int i = 0; i < 4; ++i)
            for (int j = 0; j < 4; ++j) {
                float s = 0;
                for (int k = 0; k < 4; ++k) s += m[i][k] * o.m[k][j];
                r.m[i][j] = s;
            }
        return r;
    }
    Vec3 TransformPoint(const Vec3& p) const {
        return {m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3], m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3],
                m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3]};
    }
    Vec3 TransformDir(const Vec3& d) const {
        return {m[0][0] * d.x + m[0][1] * d.y + m[0][2] * d.z, m[1][0] * d.x + m[1][1] * d.y + m[1][2] * d.z,
                m[2][0] * d.x + m[2][1] * d.y + m[2][2] * d.z};
    }
    // Inverse of a rigid transform (rotation + translation).
    Mat4 RigidInverse() const {
        Mat4 r;
        for (int i = 0; i < 3; ++i)
            for (int j = 0; j < 3; ++j) r.m[i][j] = m[j][i];
        for (int i = 0; i < 3; ++i) r.m[i][3] = -(r.m[i][0] * m[0][3] + r.m[i][1] * m[1][3] + r.m[i][2] * m[2][3]);
        return r;
    }
    Vec3 Position() const { return {m[0][3], m[1][3], m[2][3]}; }
};

// Euler angles (degrees) of the Y-X-Z intrinsic rotation stored in a pose
// matrix: yaw about +Y, pitch about +X, roll about +Z (docs/coordinates.md).
struct Euler {
    double yaw = 0, pitch = 0, roll = 0;
};

inline Euler EulerDeg(const vr::HmdMatrix34_t& a) {
    const double r2d = 180.0 / kPi;
    Euler e;
    e.yaw = std::atan2(a.m[0][2], a.m[2][2]) * r2d;
    e.pitch = std::asin(std::fmax(-1.0, std::fmin(1.0, -static_cast<double>(a.m[1][2])))) * r2d;
    e.roll = std::atan2(a.m[1][0], a.m[1][1]) * r2d;
    return e;
}

inline const char* ResultName(vr::ETrackingResult r) {
    switch (r) {
        case vr::TrackingResult_Uninitialized: return "uninit";
        case vr::TrackingResult_Calibrating_InProgress: return "calib";
        case vr::TrackingResult_Calibrating_OutOfRange: return "calib_oor";
        case vr::TrackingResult_Running_OK: return "ok";
        case vr::TrackingResult_Running_OutOfRange: return "oor";
        case vr::TrackingResult_Fallback_RotationOnly: return "rot_only";
        default: return "?";
    }
}

inline const char* ClassName(vr::ETrackedDeviceClass c) {
    switch (c) {
        case vr::TrackedDeviceClass_HMD: return "hmd";
        case vr::TrackedDeviceClass_Controller: return "ctrl";
        case vr::TrackedDeviceClass_GenericTracker: return "trk";
        case vr::TrackedDeviceClass_TrackingReference: return "ref";
        default: return "?";
    }
}

inline std::string StringProp(vr::IVRSystem* sys, uint32_t index, vr::ETrackedDeviceProperty prop) {
    char buf[256] = {};
    vr::ETrackedPropertyError err = vr::TrackedProp_Success;
    sys->GetStringTrackedDeviceProperty(index, prop, buf, sizeof(buf), &err);
    return err == vr::TrackedProp_Success ? buf : "";
}

// Makes the standing and seated universes coincide with the raw tracking
// space and defines a square play area (docs/coordinates.md). SteamVR
// persists the result under the driver's universe id.
inline bool SetupChaperoneIdentity(float size_m = 3.0f) {
    auto* setup = vr::VRChaperoneSetup();
    if (!setup) return false;
    setup->RevertWorkingCopy();
    vr::HmdMatrix34_t identity{};
    identity.m[0][0] = identity.m[1][1] = identity.m[2][2] = 1.0f;
    setup->SetWorkingStandingZeroPoseToRawTrackingPose(&identity);
    setup->SetWorkingSeatedZeroPoseToRawTrackingPose(&identity);
    setup->SetWorkingPlayAreaSize(size_m, size_m);
    const float h = size_m / 2.0f;
    vr::HmdQuad_t walls[4] = {};
    const float corners[5][2] = {{-h, -h}, {h, -h}, {h, h}, {-h, h}, {-h, -h}};
    for (int i = 0; i < 4; ++i) {
        walls[i].vCorners[0] = {corners[i][0], 0.0f, corners[i][1]};
        walls[i].vCorners[1] = {corners[i][0], 2.5f, corners[i][1]};
        walls[i].vCorners[2] = {corners[i + 1][0], 2.5f, corners[i + 1][1]};
        walls[i].vCorners[3] = {corners[i + 1][0], 0.0f, corners[i + 1][1]};
    }
    setup->SetWorkingCollisionBoundsInfo(walls, 4);
    return setup->CommitWorkingCopy(vr::EChaperoneConfigFile_Live);
}

// Input state of one hand as read through the e2e/viewer action manifest.
struct HandInput {
    bool active = false;
    bool a = false, b = false, joystick_click = false;
    float trigger = 0, grip = 0, joystick_x = 0, joystick_y = 0;
};

struct ActionSet {
    vr::VRActionSetHandle_t set = vr::k_ulInvalidActionSetHandle;
    vr::VRActionHandle_t a = 0, b = 0, trigger = 0, grip = 0, joystick = 0, joystick_click = 0, hand = 0;
    vr::VRInputValueHandle_t left = 0, right = 0;

    // Loads actions.json next to the executable.
    bool Init(const std::string& manifest_path) {
        auto* in = vr::VRInput();
        if (in->SetActionManifestPath(manifest_path.c_str()) != vr::VRInputError_None) return false;
        in->GetActionSetHandle("/actions/main", &set);
        in->GetActionHandle("/actions/main/in/a", &a);
        in->GetActionHandle("/actions/main/in/b", &b);
        in->GetActionHandle("/actions/main/in/trigger", &trigger);
        in->GetActionHandle("/actions/main/in/grip", &grip);
        in->GetActionHandle("/actions/main/in/joystick", &joystick);
        in->GetActionHandle("/actions/main/in/joystick_click", &joystick_click);
        in->GetActionHandle("/actions/main/in/hand", &hand);
        in->GetInputSourceHandle("/user/hand/left", &left);
        in->GetInputSourceHandle("/user/hand/right", &right);
        return true;
    }
    void Update() {
        vr::VRActiveActionSet_t active{};
        active.ulActionSet = set;
        active.ulRestrictedToDevice = vr::k_ulInvalidInputValueHandle;
        vr::VRInput()->UpdateActionState(&active, sizeof(active), 1);
    }
    HandInput Read(vr::VRInputValueHandle_t device) const {
        HandInput h;
        vr::InputDigitalActionData_t d{};
        vr::InputAnalogActionData_t an{};
        auto* in = vr::VRInput();
        in->GetDigitalActionData(a, &d, sizeof(d), device);
        h.active = d.bActive;
        h.a = d.bState;
        in->GetDigitalActionData(b, &d, sizeof(d), device);
        h.b = d.bState;
        in->GetDigitalActionData(joystick_click, &d, sizeof(d), device);
        h.joystick_click = d.bState;
        in->GetAnalogActionData(trigger, &an, sizeof(an), device);
        h.trigger = an.x;
        in->GetAnalogActionData(grip, &an, sizeof(an), device);
        h.grip = an.x;
        in->GetAnalogActionData(joystick, &an, sizeof(an), device);
        h.joystick_x = an.x;
        h.joystick_y = an.y;
        return h;
    }
};

// Line list in world space, rebuilt every frame.
struct Lines {
    struct Vertex {
        Vec3 p;
        Color c;
    };
    std::vector<Vertex> v;

    void Line(Vec3 a, Vec3 b, Color c) {
        v.push_back({a, c});
        v.push_back({b, c});
    }
    void Grid(float half, float step, Color c) {
        for (float x = -half; x <= half + 1e-4f; x += step) {
            Line({x, 0, -half}, {x, 0, half}, c);
            Line({-half, 0, x}, {half, 0, x}, c);
        }
    }
    // RGB axes of a pose (X red, Y green, Z blue), length in meters.
    void Axes(const Mat4& pose, float len) {
        const Vec3 o = pose.Position();
        const Vec3 x = pose.TransformPoint({len, 0, 0});
        const Vec3 y = pose.TransformPoint({0, len, 0});
        const Vec3 z = pose.TransformPoint({0, 0, len});
        Line(o, x, {1, 0.2f, 0.2f, 1});
        Line(o, y, {0.2f, 1, 0.2f, 1});
        Line(o, z, {0.3f, 0.5f, 1, 1});
    }
    // Wireframe box centred on the pose.
    void Box(const Mat4& pose, Vec3 half, Color c) {
        Vec3 corners[8];
        for (int i = 0; i < 8; ++i) {
            corners[i] = pose.TransformPoint({(i & 1) ? half.x : -half.x, (i & 2) ? half.y : -half.y, (i & 4) ? half.z : -half.z});
        }
        static const int edges[12][2] = {{0, 1}, {2, 3}, {4, 5}, {6, 7}, {0, 2}, {1, 3}, {4, 6}, {5, 7}, {0, 4}, {1, 5}, {2, 6}, {3, 7}};
        for (const auto& e : edges) Line(corners[e[0]], corners[e[1]], c);
    }
    // Horizontal bar of a value in [0,1] above a pose: grey outline, filled part coloured.
    void Bar(const Mat4& pose, float y_offset, float value, Color on) {
        const float w = 0.08f;
        const Vec3 a = pose.TransformPoint({-w / 2, y_offset, 0});
        const Vec3 b = pose.TransformPoint({w / 2, y_offset, 0});
        Line(a, b, {0.4f, 0.4f, 0.4f, 1});
        if (value > 0.001f) {
            const Vec3 f = pose.TransformPoint({-w / 2 + w * value, y_offset + 0.005f, 0});
            const Vec3 s = pose.TransformPoint({-w / 2, y_offset + 0.005f, 0});
            Line(s, f, on);
        }
    }
    // Small square indicator; filled (bright) when on.
    void Indicator(const Mat4& pose, Vec3 offset, bool on, Color c) {
        const float s = 0.012f;
        const Color col = on ? c : Color{c.r * 0.25f, c.g * 0.25f, c.b * 0.25f, 1};
        const Vec3 p[4] = {pose.TransformPoint({offset.x - s, offset.y - s, offset.z}), pose.TransformPoint({offset.x + s, offset.y - s, offset.z}),
                           pose.TransformPoint({offset.x + s, offset.y + s, offset.z}), pose.TransformPoint({offset.x - s, offset.y + s, offset.z})};
        for (int i = 0; i < 4; ++i) Line(p[i], p[(i + 1) % 4], col);
        if (on) Line(p[0], p[2], col), Line(p[1], p[3], col);
    }
};

// Draws the floor, the world axes, every tracked device and the hand inputs.
inline void BuildScene(Lines& lines, vr::IVRSystem* sys, const vr::TrackedDevicePose_t* poses, const HandInput& left,
                       const HandInput& right, uint32_t hmd_index) {
    lines.Grid(3.0f, 0.5f, {0.25f, 0.25f, 0.3f, 1});
    lines.Grid(3.0f, 1.0f, {0.4f, 0.4f, 0.45f, 1});
    lines.Axes(Mat4::Identity(), 0.5f);
    for (uint32_t i = 0; i < vr::k_unMaxTrackedDeviceCount; ++i) {
        const auto cls = sys->GetTrackedDeviceClass(i);
        if (cls == vr::TrackedDeviceClass_Invalid || !poses[i].bPoseIsValid) continue;
        const Mat4 pose = Mat4::From34(poses[i].mDeviceToAbsoluteTracking);
        lines.Axes(pose, 0.1f);
        if (cls == vr::TrackedDeviceClass_HMD) {
            if (i != hmd_index) lines.Box(pose, {0.09f, 0.05f, 0.06f}, {0.9f, 0.9f, 0.9f, 1});
        } else if (cls == vr::TrackedDeviceClass_Controller) {
            const auto role = sys->GetControllerRoleForTrackedDeviceIndex(i);
            const bool is_left = role == vr::TrackedControllerRole_LeftHand;
            const HandInput& in = is_left ? left : right;
            const Color body = is_left ? Color{0.3f, 0.8f, 1, 1} : Color{1, 0.6f, 0.3f, 1};
            lines.Box(pose, {0.03f, 0.02f, 0.08f}, body);
            lines.Indicator(pose, {-0.03f, 0.06f, 0}, in.a, {0.2f, 1, 0.2f, 1});
            lines.Indicator(pose, {0.03f, 0.06f, 0}, in.b, {1, 0.3f, 0.3f, 1});
            lines.Indicator(pose, {0, 0.06f, -0.04f}, in.joystick_click, {1, 1, 0.3f, 1});
            lines.Bar(pose, 0.10f, in.trigger, {0.2f, 1, 0.2f, 1});
            lines.Bar(pose, 0.125f, in.grip, {1, 0.6f, 0.2f, 1});
            // Joystick: a square and the stick position inside it.
            const float r = 0.03f;
            const Vec3 c = {0, 0.17f, 0};
            lines.Indicator(pose, c, false, {0.6f, 0.6f, 0.6f, 1});
            lines.Line(pose.TransformPoint({c.x - r, c.y, 0}), pose.TransformPoint({c.x + r, c.y, 0}), {0.5f, 0.5f, 0.5f, 1});
            lines.Line(pose.TransformPoint({c.x, c.y - r, 0}), pose.TransformPoint({c.x, c.y + r, 0}), {0.5f, 0.5f, 0.5f, 1});
            lines.Indicator(pose, {c.x + r * in.joystick_x, c.y + r * in.joystick_y, 0}, true, {1, 1, 0.3f, 1});
        } else if (cls == vr::TrackedDeviceClass_GenericTracker) {
            lines.Box(pose, {0.04f, 0.015f, 0.04f}, {0.9f, 0.5f, 1, 1});
        }
    }
}

}  // namespace vrscene
