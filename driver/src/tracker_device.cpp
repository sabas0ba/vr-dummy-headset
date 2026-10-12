#include "tracker_device.h"

namespace vrdummy {

TrackerDevice::TrackerDevice(uint16_t device_id, std::string serial, std::string model_number)
    : TrackedDevice(device_id, protocol::DeviceClass::Tracker, std::move(serial), std::move(model_number)) {}

void TrackerDevice::SetupProperties(vr::PropertyContainerHandle_t container) {
    auto* props = vr::VRProperties();
    props->SetStringProperty(container, vr::Prop_ControllerType_String, "vrdummy_tracker");
    props->SetStringProperty(container, vr::Prop_InputProfilePath_String, "{vrdummy}/input/vrdummy_tracker_profile.json");
    // Roles are assigned by the user in SteamVR (Manage Trackers) or by the
    // application (VRChat assigns by position during calibration).
    props->SetInt32Property(container, vr::Prop_ControllerRoleHint_Int32, vr::TrackedControllerRole_Invalid);
}

}  // namespace vrdummy
