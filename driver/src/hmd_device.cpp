#include "hmd_device.h"

#include <cstring>

#include "logging.h"
#include "pose_convert.h"

namespace vrdummy {

DisplayComponent::DisplayComponent(const DisplayConfig& config) : config_(config) {}

bool DisplayComponent::IsDisplayOnDesktop() { return true; }

bool DisplayComponent::IsDisplayRealDisplay() { return false; }

void DisplayComponent::GetRecommendedRenderTargetSize(uint32_t* width, uint32_t* height) {
    *width = static_cast<uint32_t>(config_.render_width);
    *height = static_cast<uint32_t>(config_.render_height);
}

vr::EVREye DisplayComponent::Physical(vr::EVREye eye) {
    if (!swap_eyes_) return eye;
    return eye == vr::Eye_Left ? vr::Eye_Right : vr::Eye_Left;
}

void DisplayComponent::GetEyeOutputViewport(vr::EVREye eye, uint32_t* x, uint32_t* y, uint32_t* width,
                                            uint32_t* height) {
    std::lock_guard<std::mutex> lock(mutex_);
    const auto half = static_cast<uint32_t>(config_.window_width / 2);
    *y = 0;
    *width = half;
    *height = static_cast<uint32_t>(config_.window_height);
    *x = Physical(eye) == vr::Eye_Left ? 0 : half;
}

void DisplayComponent::GetProjectionRaw(vr::EVREye eye, float* left, float* right, float* top, float* bottom) {
    const std::array<float, 4> p = projection(eye);
    *left = p[0];
    *right = p[1];
    *top = p[2];
    *bottom = p[3];
}

std::array<float, 4> DisplayComponent::projection(vr::EVREye eye) {
    std::lock_guard<std::mutex> lock(mutex_);
    return Physical(eye) == vr::Eye_Left ? projection_left_ : projection_right_;
}

bool DisplayComponent::swap_eyes() {
    std::lock_guard<std::mutex> lock(mutex_);
    return swap_eyes_;
}

vr::DistortionCoordinates_t DisplayComponent::ComputeDistortion(vr::EVREye /*eye*/, float u, float v) {
    vr::DistortionCoordinates_t c{};
    c.rfRed[0] = c.rfGreen[0] = c.rfBlue[0] = u;
    c.rfRed[1] = c.rfGreen[1] = c.rfBlue[1] = v;
    return c;
}

void DisplayComponent::GetWindowBounds(int32_t* x, int32_t* y, uint32_t* width, uint32_t* height) {
    *x = config_.window_x;
    *y = config_.window_y;
    *width = static_cast<uint32_t>(config_.window_width);
    *height = static_cast<uint32_t>(config_.window_height);
}

bool DisplayComponent::ComputeInverseDistortion(vr::HmdVector2_t* /*result*/, vr::EVREye /*eye*/,
                                                uint32_t /*channel*/, float /*u*/, float /*v*/) {
    // Let SteamVR derive the inverse from ComputeDistortion.
    return false;
}

void DisplayComponent::Apply(const protocol::Display& display) {
    std::lock_guard<std::mutex> lock(mutex_);
    if (display.flags.has(protocol::DisplayFlags::kProjection)) {
        projection_left_ = display.projection_left;
        projection_right_ = display.projection_right;
    }
    if (display.flags.has(protocol::DisplayFlags::kSwapEyes)) {
        swap_eyes_ = display.swap_eyes;
    }
}

HmdDevice::HmdDevice(uint16_t device_id, std::string serial, std::string model_number, const DisplayConfig& config)
    : TrackedDevice(device_id, protocol::DeviceClass::Hmd, std::move(serial), std::move(model_number)),
      config_(config),
      display_(config) {}

void* HmdDevice::GetComponent(const char* component_name_and_version) {
    if (std::strcmp(component_name_and_version, vr::IVRDisplayComponent_Version) == 0) return &display_;
    return nullptr;
}

vr::DriverPose_t HmdDevice::InitialPose() const {
    // A valid standing-height pose keeps SteamVR usable without the feeder.
    vr::DriverPose_t pose = MakeInitialPose();
    pose.vecPosition[1] = 1.6;
    pose.poseIsValid = true;
    pose.result = vr::TrackingResult_Running_OK;
    return pose;
}

void HmdDevice::SetupProperties(vr::PropertyContainerHandle_t container) {
    auto* props = vr::VRProperties();
    props->SetFloatProperty(container, vr::Prop_UserIpdMeters_Float, config_.ipd_m);
    props->SetFloatProperty(container, vr::Prop_DisplayFrequency_Float, config_.display_frequency);
    props->SetFloatProperty(container, vr::Prop_UserHeadToEyeDepthMeters_Float, 0.0f);
    props->SetFloatProperty(container, vr::Prop_SecondsFromVsyncToPhotons_Float, config_.seconds_from_vsync_to_photons);
    props->SetBoolProperty(container, vr::Prop_IsOnDesktop_Bool, false);
    props->SetBoolProperty(container, vr::Prop_DisplayDebugMode_Bool, true);
    props->SetBoolProperty(container, vr::Prop_ContainsProximitySensor_Bool, true);
    props->SetUint64Property(container, vr::Prop_CurrentUniverseId_Uint64, config_.universe_id);
    props->SetStringProperty(container, vr::Prop_ControllerType_String, "vrdummy_hmd");
    props->SetStringProperty(container, vr::Prop_InputProfilePath_String, "{vrdummy}/input/vrdummy_hmd_profile.json");
}

void HmdDevice::SetupInputs(vr::PropertyContainerHandle_t container) {
    CreateBool(container, 0, "/input/system/click");
    CreateBool(container, 1, "/proximity");
    // Report the headset as worn so that applications do not pause.
    protocol::Input worn;
    worn.device_id = device_id();
    worn.component_id = 1;
    worn.kind = protocol::InputKind::Bool;
    worn.value = 1.0f;
    ApplyInput(worn);
}

void HmdDevice::ApplyDisplay(const protocol::Display& display) {
    display_.Apply(display);
    if (display.flags.has(protocol::DisplayFlags::kIpd) && active()) {
        vr::VRProperties()->SetFloatProperty(container(), vr::Prop_UserIpdMeters_Float, display.ipd_m);
        VRDUMMY_LOG_INFO("hmd: ipd set to %.4f m", static_cast<double>(display.ipd_m));
    }
    if (display.flags.has(protocol::DisplayFlags::kProjection)) {
        VRDUMMY_LOG_INFO("hmd: projection L(%.3f %.3f %.3f %.3f) R(%.3f %.3f %.3f %.3f)",
                         static_cast<double>(display.projection_left[0]), static_cast<double>(display.projection_left[1]),
                         static_cast<double>(display.projection_left[2]), static_cast<double>(display.projection_left[3]),
                         static_cast<double>(display.projection_right[0]), static_cast<double>(display.projection_right[1]),
                         static_cast<double>(display.projection_right[2]), static_cast<double>(display.projection_right[3]));
    }
    if (display.flags.has(protocol::DisplayFlags::kSwapEyes)) {
        VRDUMMY_LOG_INFO("hmd: swap_eyes=%d", display.swap_eyes ? 1 : 0);
    }
}

}  // namespace vrdummy
