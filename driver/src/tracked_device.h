// Base class of every device exposed by the driver.
// Holds the latest pose and input values received from the feeder and
// submits them to vrserver. Subclasses only set properties and components.
#pragma once

#include <atomic>
#include <mutex>
#include <string>
#include <vector>

#include "openvr_driver.h"
#include "protocol.h"

namespace vrdummy {

class TrackedDevice : public vr::ITrackedDeviceServerDriver {
public:
    TrackedDevice(uint16_t device_id, protocol::DeviceClass device_class, std::string serial, std::string model_number);
    // ITrackedDeviceServerDriver has no virtual destructor; devices are owned
    // by the provider and deleted through this class.
    virtual ~TrackedDevice() = default;

    // vr::ITrackedDeviceServerDriver
    vr::EVRInitError Activate(uint32_t object_id) override;
    void Deactivate() override;
    void EnterStandby() override;
    void* GetComponent(const char* component_name_and_version) override;
    void DebugRequest(const char* request, char* response_buffer, uint32_t response_buffer_size) override;
    vr::DriverPose_t GetPose() override;

    uint16_t device_id() const { return device_id_; }
    protocol::DeviceClass device_class() const { return device_class_; }
    const std::string& serial() const { return serial_; }
    vr::ETrackedDeviceClass openvr_class() const;
    bool active() const { return object_id_.load() != vr::k_unTrackedDeviceIndexInvalid; }

    // Called from the pipe thread.
    void ApplyPose(const protocol::Pose& pose);
    void ApplyInput(const protocol::Input& input);
    void SetFeederConnected(bool connected);

    // Called from the pose thread at a fixed rate.
    void SubmitPose();

protected:
    // Set device properties. Called once from Activate.
    virtual void SetupProperties(vr::PropertyContainerHandle_t container) = 0;
    // Create input components. Called once from Activate after SetupProperties.
    virtual void SetupInputs(vr::PropertyContainerHandle_t /*container*/) {}
    virtual bool should_apply_head_model() const { return false; }
    // HMD keeps its last pose without the feeder so that SteamVR stays usable.
    virtual bool keep_pose_when_disconnected() const { return false; }
    virtual vr::DriverPose_t InitialPose() const;

    void CreateBool(vr::PropertyContainerHandle_t container, uint16_t component_id, const char* path);
    void CreateScalar(vr::PropertyContainerHandle_t container, uint16_t component_id, const char* path,
                      vr::EVRScalarUnits units);
    vr::PropertyContainerHandle_t container() const { return container_; }

    const std::string model_number_;

private:
    struct Component {
        vr::VRInputComponentHandle_t handle = vr::k_ulInvalidInputComponentHandle;
        protocol::InputKind kind = protocol::InputKind::Bool;
        bool valid = false;
    };

    const uint16_t device_id_;
    const protocol::DeviceClass device_class_;
    const std::string serial_;
    std::atomic<uint32_t> object_id_{vr::k_unTrackedDeviceIndexInvalid};
    vr::PropertyContainerHandle_t container_ = vr::k_ulInvalidPropertyContainer;
    std::vector<Component> components_;

    std::mutex mutex_;
    vr::DriverPose_t pose_;
    bool feeder_connected_ = false;
};

}  // namespace vrdummy
