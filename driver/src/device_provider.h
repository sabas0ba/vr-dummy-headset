// IServerTrackedDeviceProvider: owns the devices, the pipe server and the
// pose submission thread.
#pragma once

#include <atomic>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

#include "controller_device.h"
#include "hmd_device.h"
#include "openvr_driver.h"
#include "pipe_server.h"
#include "tracker_device.h"

namespace vrdummy {

struct DriverSettings {
    bool hmd_enable = true;
    bool controllers_enable = true;
    int32_t tracker_count = 3;
    int32_t pose_rate_hz = 100;
    std::string pipe_name = protocol::kDefaultPipeName;
    std::string serial_prefix = "VRDUMMY";
    std::string log_level = "info";
    DisplayConfig display;
};

class DeviceProvider : public vr::IServerTrackedDeviceProvider {
public:
    vr::EVRInitError Init(vr::IVRDriverContext* driver_context) override;
    void Cleanup() override;
    const char* const* GetInterfaceVersions() override;
    void RunFrame() override;
    bool ShouldBlockStandbyMode() override;
    void EnterStandby() override;
    void LeaveStandby() override;

private:
    DriverSettings ReadSettings();
    void CreateDevices(const DriverSettings& settings);
    TrackedDevice* FindDevice(uint16_t device_id);
    void OnFeederConnected();
    void OnFeederDisconnected();
    void OnMessage(const protocol::Message& message);
    void SendHelloAck();
    void PoseThread();
    void ProcessEvent(const vr::VREvent_t& event);

    DriverSettings settings_;
    std::vector<std::unique_ptr<TrackedDevice>> devices_;  // all devices including the HMD
    HmdDevice* hmd_ = nullptr;                              // owned by devices_
    PipeServer pipe_;
    std::atomic<bool> handshake_done_{false};
    std::atomic<bool> running_{false};
    std::thread pose_thread_;
};

}  // namespace vrdummy
