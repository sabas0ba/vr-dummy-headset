// Virtual hand controller with the component set of docs/protocol.md 3.4.1.
#pragma once

#include "tracked_device.h"

namespace vrdummy {

class ControllerDevice : public TrackedDevice {
public:
    ControllerDevice(uint16_t device_id, bool left, std::string serial, std::string model_number);

    bool left() const { return left_; }

protected:
    void SetupProperties(vr::PropertyContainerHandle_t container) override;
    void SetupInputs(vr::PropertyContainerHandle_t container) override;

private:
    const bool left_;
    vr::VRInputComponentHandle_t haptic_ = vr::k_ulInvalidInputComponentHandle;
};

}  // namespace vrdummy
