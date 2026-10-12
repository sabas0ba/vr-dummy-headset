// Virtual generic tracker. Has no input components in protocol version 1.
#pragma once

#include "tracked_device.h"

namespace vrdummy {

class TrackerDevice : public TrackedDevice {
public:
    TrackerDevice(uint16_t device_id, std::string serial, std::string model_number);

protected:
    void SetupProperties(vr::PropertyContainerHandle_t container) override;
};

}  // namespace vrdummy
