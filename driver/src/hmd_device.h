// Virtual HMD with an extended-mode display component whose optical
// parameters can be changed at runtime from the feeder.
#pragma once

#include <array>
#include <mutex>

#include "openvr_driver.h"
#include "tracked_device.h"

namespace vrdummy {

struct DisplayConfig {
    int32_t window_x = 0;
    int32_t window_y = 0;
    int32_t window_width = 1920;
    int32_t window_height = 1080;
    int32_t render_width = 1440;
    int32_t render_height = 1600;
    float display_frequency = 90.0f;
    float ipd_m = 0.064f;
    float seconds_from_vsync_to_photons = 0.011f;
    // Tracking universe id. SteamVR keys chaperone (room setup) data by it;
    // without one the HMD stays "uncalibrated" and nothing is persisted.
    uint64_t universe_id = 0x5652444D;  // "VRDM"
};

class DisplayComponent : public vr::IVRDisplayComponent {
public:
    explicit DisplayComponent(const DisplayConfig& config);

    // vr::IVRDisplayComponent
    bool IsDisplayOnDesktop() override;
    bool IsDisplayRealDisplay() override;
    void GetRecommendedRenderTargetSize(uint32_t* width, uint32_t* height) override;
    void GetEyeOutputViewport(vr::EVREye eye, uint32_t* x, uint32_t* y, uint32_t* width, uint32_t* height) override;
    void GetProjectionRaw(vr::EVREye eye, float* left, float* right, float* top, float* bottom) override;
    vr::DistortionCoordinates_t ComputeDistortion(vr::EVREye eye, float u, float v) override;
    void GetWindowBounds(int32_t* x, int32_t* y, uint32_t* width, uint32_t* height) override;
    bool ComputeInverseDistortion(vr::HmdVector2_t* result, vr::EVREye eye, uint32_t channel, float u,
                                  float v) override;

    // Applies projection and swap flags. The IPD is handled by HmdDevice.
    void Apply(const protocol::Display& display);
    std::array<float, 4> projection(vr::EVREye eye);
    bool swap_eyes();

private:
    vr::EVREye Physical(vr::EVREye eye);

    const DisplayConfig config_;
    std::mutex mutex_;
    std::array<float, 4> projection_left_{-1.0f, 1.0f, -1.0f, 1.0f};
    std::array<float, 4> projection_right_{-1.0f, 1.0f, -1.0f, 1.0f};
    bool swap_eyes_ = false;
};

class HmdDevice : public TrackedDevice {
public:
    HmdDevice(uint16_t device_id, std::string serial, std::string model_number, const DisplayConfig& config);

    void* GetComponent(const char* component_name_and_version) override;
    void ApplyDisplay(const protocol::Display& display);

protected:
    void SetupProperties(vr::PropertyContainerHandle_t container) override;
    void SetupInputs(vr::PropertyContainerHandle_t container) override;
    // With the head model enabled vrserver offsets the reported HMD position
    // by a neck model that depends on orientation, so commanded positions
    // would no longer be reproduced exactly. A debugging device reports what
    // it was told.
    bool should_apply_head_model() const override { return false; }
    bool keep_pose_when_disconnected() const override { return true; }
    vr::DriverPose_t InitialPose() const override;

private:
    DisplayConfig config_;
    DisplayComponent display_;
};

}  // namespace vrdummy
