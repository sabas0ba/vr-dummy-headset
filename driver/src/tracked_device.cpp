#include "tracked_device.h"

#include <cstring>

#include "logging.h"
#include "pose_convert.h"

namespace vrdummy {

TrackedDevice::TrackedDevice(uint16_t device_id, protocol::DeviceClass device_class, std::string serial,
                             std::string model_number)
    : model_number_(std::move(model_number)),
      device_id_(device_id),
      device_class_(device_class),
      serial_(std::move(serial)),
      pose_(MakeInitialPose()) {}

vr::ETrackedDeviceClass TrackedDevice::openvr_class() const {
    switch (device_class_) {
        case protocol::DeviceClass::Hmd: return vr::TrackedDeviceClass_HMD;
        case protocol::DeviceClass::ControllerLeft:
        case protocol::DeviceClass::ControllerRight: return vr::TrackedDeviceClass_Controller;
        case protocol::DeviceClass::Tracker: return vr::TrackedDeviceClass_GenericTracker;
    }
    return vr::TrackedDeviceClass_Invalid;
}

vr::DriverPose_t TrackedDevice::InitialPose() const { return MarkOutOfRange(MakeInitialPose()); }

vr::EVRInitError TrackedDevice::Activate(uint32_t object_id) {
    object_id_.store(object_id);
    container_ = vr::VRProperties()->TrackedDeviceToPropertyContainer(object_id);
    vr::VRProperties()->SetStringProperty(container_, vr::Prop_SerialNumber_String, serial_.c_str());
    vr::VRProperties()->SetStringProperty(container_, vr::Prop_ModelNumber_String, model_number_.c_str());
    vr::VRProperties()->SetStringProperty(container_, vr::Prop_ManufacturerName_String, "vr-dummy-headset");
    vr::VRProperties()->SetBoolProperty(container_, vr::Prop_Identifiable_Bool, false);
    vr::VRProperties()->SetBoolProperty(container_, vr::Prop_DeviceProvidesBatteryStatus_Bool, false);
    SetupProperties(container_);
    SetupInputs(container_);
    {
        std::lock_guard<std::mutex> lock(mutex_);
        pose_ = InitialPose();
        pose_.shouldApplyHeadModel = should_apply_head_model();
    }
    VRDUMMY_LOG_INFO("device %u (%s) activated as index %u", device_id_, serial_.c_str(), object_id);
    return vr::VRInitError_None;
}

void TrackedDevice::Deactivate() {
    VRDUMMY_LOG_INFO("device %u (%s) deactivated", device_id_, serial_.c_str());
    object_id_.store(vr::k_unTrackedDeviceIndexInvalid);
}

void TrackedDevice::EnterStandby() { VRDUMMY_LOG_DEBUG("device %u entered standby", device_id_); }

void* TrackedDevice::GetComponent(const char* /*component_name_and_version*/) { return nullptr; }

void TrackedDevice::DebugRequest(const char* /*request*/, char* response_buffer, uint32_t response_buffer_size) {
    if (response_buffer_size >= 1) response_buffer[0] = 0;
}

vr::DriverPose_t TrackedDevice::GetPose() {
    std::lock_guard<std::mutex> lock(mutex_);
    return pose_;
}

void TrackedDevice::ApplyPose(const protocol::Pose& pose) {
    std::lock_guard<std::mutex> lock(mutex_);
    pose_ = ToDriverPose(pose, should_apply_head_model());
}

void TrackedDevice::ApplyInput(const protocol::Input& input) {
    if (!active()) return;
    if (input.component_id >= components_.size() || !components_[input.component_id].valid) {
        VRDUMMY_LOG_WARN("device %u: unknown component id %u", device_id_, input.component_id);
        return;
    }
    const Component& c = components_[input.component_id];
    if (c.kind != input.kind) {
        VRDUMMY_LOG_WARN("device %u: component %u kind mismatch", device_id_, input.component_id);
        return;
    }
    if (c.kind == protocol::InputKind::Bool) {
        vr::VRDriverInput()->UpdateBooleanComponent(c.handle, input.value != 0.0f, input.time_offset_s);
    } else {
        vr::VRDriverInput()->UpdateScalarComponent(c.handle, input.value, input.time_offset_s);
    }
}

void TrackedDevice::SetFeederConnected(bool connected) {
    std::lock_guard<std::mutex> lock(mutex_);
    feeder_connected_ = connected;
    if (!connected && !keep_pose_when_disconnected()) {
        pose_ = MarkOutOfRange(pose_);
    }
}

void TrackedDevice::SubmitPose() {
    const uint32_t id = object_id_.load();
    if (id == vr::k_unTrackedDeviceIndexInvalid) return;
    vr::DriverPose_t pose;
    {
        std::lock_guard<std::mutex> lock(mutex_);
        pose = pose_;
    }
    vr::VRServerDriverHost()->TrackedDevicePoseUpdated(id, pose, sizeof(vr::DriverPose_t));
}

void TrackedDevice::CreateBool(vr::PropertyContainerHandle_t container, uint16_t component_id, const char* path) {
    if (components_.size() <= component_id) components_.resize(component_id + 1u);
    Component& c = components_[component_id];
    c.kind = protocol::InputKind::Bool;
    const auto err = vr::VRDriverInput()->CreateBooleanComponent(container, path, &c.handle);
    c.valid = err == vr::VRInputError_None;
    if (!c.valid) VRDUMMY_LOG_ERROR("device %u: CreateBooleanComponent(%s) failed (%d)", device_id_, path, err);
}

void TrackedDevice::CreateScalar(vr::PropertyContainerHandle_t container, uint16_t component_id, const char* path,
                                 vr::EVRScalarUnits units) {
    if (components_.size() <= component_id) components_.resize(component_id + 1u);
    Component& c = components_[component_id];
    c.kind = protocol::InputKind::Scalar;
    const auto err = vr::VRDriverInput()->CreateScalarComponent(container, path, &c.handle, vr::VRScalarType_Absolute, units);
    c.valid = err == vr::VRInputError_None;
    if (!c.valid) VRDUMMY_LOG_ERROR("device %u: CreateScalarComponent(%s) failed (%d)", device_id_, path, err);
}

}  // namespace vrdummy
