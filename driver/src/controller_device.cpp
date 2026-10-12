#include "controller_device.h"

#include "logging.h"

namespace vrdummy {

ControllerDevice::ControllerDevice(uint16_t device_id, bool left, std::string serial, std::string model_number)
    : TrackedDevice(device_id, left ? protocol::DeviceClass::ControllerLeft : protocol::DeviceClass::ControllerRight,
                    std::move(serial), std::move(model_number)),
      left_(left) {}

void ControllerDevice::SetupProperties(vr::PropertyContainerHandle_t container) {
    auto* props = vr::VRProperties();
    props->SetInt32Property(container, vr::Prop_ControllerRoleHint_Int32,
                            left_ ? vr::TrackedControllerRole_LeftHand : vr::TrackedControllerRole_RightHand);
    props->SetStringProperty(container, vr::Prop_ControllerType_String, "vrdummy_controller");
    props->SetStringProperty(container, vr::Prop_InputProfilePath_String,
                             "{vrdummy}/input/vrdummy_controller_profile.json");
}

void ControllerDevice::SetupInputs(vr::PropertyContainerHandle_t container) {
    // Ids follow docs/protocol.md 3.4.1 and feeder/crates/protocol controller_components.
    CreateBool(container, 0, "/input/system/click");
    CreateBool(container, 1, "/input/a/click");
    CreateBool(container, 2, "/input/a/touch");
    CreateBool(container, 3, "/input/b/click");
    CreateBool(container, 4, "/input/b/touch");
    CreateScalar(container, 5, "/input/trigger/value", vr::VRScalarUnits_NormalizedOneSided);
    CreateBool(container, 6, "/input/trigger/click");
    CreateBool(container, 7, "/input/trigger/touch");
    CreateScalar(container, 8, "/input/grip/value", vr::VRScalarUnits_NormalizedOneSided);
    CreateBool(container, 9, "/input/grip/click");
    CreateBool(container, 10, "/input/grip/touch");
    CreateScalar(container, 11, "/input/joystick/x", vr::VRScalarUnits_NormalizedTwoSided);
    CreateScalar(container, 12, "/input/joystick/y", vr::VRScalarUnits_NormalizedTwoSided);
    CreateBool(container, 13, "/input/joystick/click");
    CreateBool(container, 14, "/input/joystick/touch");
    const auto err = vr::VRDriverInput()->CreateHapticComponent(container, "/output/haptic", &haptic_);
    if (err != vr::VRInputError_None) {
        VRDUMMY_LOG_ERROR("device %u: CreateHapticComponent failed (%d)", device_id(), err);
    }
}

}  // namespace vrdummy
