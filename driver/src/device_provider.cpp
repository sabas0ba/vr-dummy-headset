#include "device_provider.h"

#include <chrono>

#include "logging.h"

namespace vrdummy {

namespace {

constexpr const char* kSettingsSection = "driver_vrdummy";
constexpr const char* kDisplaySection = "vrdummy_display";

std::string GetStringSetting(const char* section, const char* key, const std::string& fallback) {
    char buf[1024] = {};
    vr::EVRSettingsError err = vr::VRSettingsError_None;
    vr::VRSettings()->GetString(section, key, buf, sizeof(buf), &err);
    if (err != vr::VRSettingsError_None || buf[0] == 0) return fallback;
    return buf;
}

int32_t GetIntSetting(const char* section, const char* key, int32_t fallback) {
    vr::EVRSettingsError err = vr::VRSettingsError_None;
    const int32_t v = vr::VRSettings()->GetInt32(section, key, &err);
    return err == vr::VRSettingsError_None ? v : fallback;
}

float GetFloatSetting(const char* section, const char* key, float fallback) {
    vr::EVRSettingsError err = vr::VRSettingsError_None;
    const float v = vr::VRSettings()->GetFloat(section, key, &err);
    return err == vr::VRSettingsError_None ? v : fallback;
}

bool GetBoolSetting(const char* section, const char* key, bool fallback) {
    vr::EVRSettingsError err = vr::VRSettingsError_None;
    const bool v = vr::VRSettings()->GetBool(section, key, &err);
    return err == vr::VRSettingsError_None ? v : fallback;
}

}  // namespace

vr::EVRInitError DeviceProvider::Init(vr::IVRDriverContext* driver_context) {
    VR_INIT_SERVER_DRIVER_CONTEXT(driver_context);
    settings_ = ReadSettings();
    SetLogThreshold(ParseLogLevel(settings_.log_level, LogLevel::Info));
    VRDUMMY_LOG_INFO("driver_vrdummy starting (hmd=%d controllers=%d trackers=%d pipe=%s rate=%dHz)",
                     settings_.hmd_enable ? 1 : 0, settings_.controllers_enable ? 1 : 0, settings_.tracker_count,
                     settings_.pipe_name.c_str(), settings_.pose_rate_hz);

    CreateDevices(settings_);
    for (auto& device : devices_) {
        if (!vr::VRServerDriverHost()->TrackedDeviceAdded(device->serial().c_str(), device->openvr_class(),
                                                          device.get())) {
            VRDUMMY_LOG_ERROR("TrackedDeviceAdded failed for %s", device->serial().c_str());
            return vr::VRInitError_Driver_Failed;
        }
    }

    running_.store(true);
    pose_thread_ = std::thread(&DeviceProvider::PoseThread, this);

    PipeHandler handler;
    handler.on_connected = [this] { OnFeederConnected(); };
    handler.on_disconnected = [this] { OnFeederDisconnected(); };
    handler.on_message = [this](const protocol::Message& m) { OnMessage(m); };
    if (!pipe_.Start(settings_.pipe_name, std::move(handler))) {
        VRDUMMY_LOG_ERROR("pipe server failed to start; devices stay without feeder");
    }
    return vr::VRInitError_None;
}

void DeviceProvider::Cleanup() {
    VRDUMMY_LOG_INFO("driver_vrdummy cleanup");
    SetLogSink({});
    pipe_.Stop();
    running_.store(false);
    if (pose_thread_.joinable()) pose_thread_.join();
    devices_.clear();
    hmd_ = nullptr;
    VR_CLEANUP_SERVER_DRIVER_CONTEXT();
}

const char* const* DeviceProvider::GetInterfaceVersions() { return vr::k_InterfaceVersions; }

void DeviceProvider::RunFrame() {
    vr::VREvent_t event{};
    while (vr::VRServerDriverHost()->PollNextEvent(&event, sizeof(event))) {
        ProcessEvent(event);
    }
}

bool DeviceProvider::ShouldBlockStandbyMode() { return false; }

void DeviceProvider::EnterStandby() {}

void DeviceProvider::LeaveStandby() {}

DriverSettings DeviceProvider::ReadSettings() {
    DriverSettings s;
    s.hmd_enable = GetBoolSetting(kSettingsSection, "hmd_enable", s.hmd_enable);
    s.controllers_enable = GetBoolSetting(kSettingsSection, "controllers_enable", s.controllers_enable);
    s.tracker_count = GetIntSetting(kSettingsSection, "tracker_count", s.tracker_count);
    if (s.tracker_count < 0) s.tracker_count = 0;
    if (s.tracker_count > 32) s.tracker_count = 32;
    s.pose_rate_hz = GetIntSetting(kSettingsSection, "pose_rate_hz", s.pose_rate_hz);
    if (s.pose_rate_hz < 1) s.pose_rate_hz = 1;
    if (s.pose_rate_hz > 1000) s.pose_rate_hz = 1000;
    s.pipe_name = GetStringSetting(kSettingsSection, "pipe_name", s.pipe_name);
    s.serial_prefix = GetStringSetting(kSettingsSection, "serial_prefix", s.serial_prefix);
    s.log_level = GetStringSetting(kSettingsSection, "log_level", s.log_level);

    DisplayConfig& d = s.display;
    d.window_x = GetIntSetting(kDisplaySection, "window_x", d.window_x);
    d.window_y = GetIntSetting(kDisplaySection, "window_y", d.window_y);
    d.window_width = GetIntSetting(kDisplaySection, "window_width", d.window_width);
    d.window_height = GetIntSetting(kDisplaySection, "window_height", d.window_height);
    d.render_width = GetIntSetting(kDisplaySection, "render_width", d.render_width);
    d.render_height = GetIntSetting(kDisplaySection, "render_height", d.render_height);
    d.display_frequency = GetFloatSetting(kDisplaySection, "display_frequency", d.display_frequency);
    d.ipd_m = GetFloatSetting(kDisplaySection, "ipd_meters", d.ipd_m);
    const int32_t universe = GetIntSetting(kSettingsSection, "universe_id", static_cast<int32_t>(d.universe_id));
    if (universe > 0) d.universe_id = static_cast<uint64_t>(universe);
    d.seconds_from_vsync_to_photons =
        GetFloatSetting(kDisplaySection, "seconds_from_vsync_to_photons", d.seconds_from_vsync_to_photons);
    return s;
}

void DeviceProvider::CreateDevices(const DriverSettings& s) {
    const std::string& p = s.serial_prefix;
    if (s.hmd_enable) {
        auto hmd = std::make_unique<HmdDevice>(protocol::device_ids::kHmd, p + "-HMD-0", "vrdummy HMD", s.display);
        hmd_ = hmd.get();
        devices_.push_back(std::move(hmd));
    }
    if (s.controllers_enable) {
        devices_.push_back(std::make_unique<ControllerDevice>(protocol::device_ids::kControllerLeft, true, p + "-CTRL-L",
                                                              "vrdummy Controller"));
        devices_.push_back(std::make_unique<ControllerDevice>(protocol::device_ids::kControllerRight, false,
                                                              p + "-CTRL-R", "vrdummy Controller"));
    }
    for (int32_t i = 0; i < s.tracker_count; ++i) {
        const auto id = static_cast<uint16_t>(protocol::device_ids::kTrackerBase + i);
        devices_.push_back(std::make_unique<TrackerDevice>(id, p + "-TRK-" + std::to_string(id), "vrdummy Tracker"));
    }
}

TrackedDevice* DeviceProvider::FindDevice(uint16_t device_id) {
    for (auto& d : devices_) {
        if (d->device_id() == device_id) return d.get();
    }
    return nullptr;
}

void DeviceProvider::OnFeederConnected() {
    handshake_done_.store(false);
    SetLogSink([this](const protocol::LogRecord& record) { pipe_.Send(protocol::Encode(protocol::Message{record})); });
}

void DeviceProvider::OnFeederDisconnected() {
    SetLogSink({});
    handshake_done_.store(false);
    for (auto& d : devices_) d->SetFeederConnected(false);
}

void DeviceProvider::SendHelloAck() {
    protocol::HelloAck ack;
    for (auto& d : devices_) {
        protocol::DeviceInfo info;
        info.device_id = d->device_id();
        info.device_class = d->device_class();
        info.serial = d->serial();
        ack.devices.push_back(std::move(info));
    }
    if (!pipe_.Send(protocol::Encode(protocol::Message{ack}))) {
        VRDUMMY_LOG_WARN("failed to send HelloAck");
    }
}

void DeviceProvider::OnMessage(const protocol::Message& message) {
    if (const auto* hello = std::get_if<protocol::Hello>(&message)) {
        if (hello->protocol_version != protocol::kProtocolVersion) {
            VRDUMMY_LOG_WARN("feeder protocol version %u differs from %u", hello->protocol_version,
                             protocol::kProtocolVersion);
        }
        SendHelloAck();
        handshake_done_.store(true);
        for (auto& d : devices_) d->SetFeederConnected(true);
        VRDUMMY_LOG_INFO("feeder handshake complete");
        return;
    }
    if (std::holds_alternative<protocol::Goodbye>(message)) {
        VRDUMMY_LOG_INFO("feeder sent goodbye");
        OnFeederDisconnected();
        return;
    }
    if (!handshake_done_.load()) {
        VRDUMMY_LOG_DEBUG("message before handshake dropped");
        return;
    }
    if (const auto* pose = std::get_if<protocol::Pose>(&message)) {
        if (auto* d = FindDevice(pose->device_id)) {
            d->ApplyPose(*pose);
        } else {
            VRDUMMY_LOG_DEBUG("pose for unknown device %u", pose->device_id);
        }
        return;
    }
    if (const auto* input = std::get_if<protocol::Input>(&message)) {
        if (auto* d = FindDevice(input->device_id)) {
            d->ApplyInput(*input);
        } else {
            VRDUMMY_LOG_DEBUG("input for unknown device %u", input->device_id);
        }
        return;
    }
    if (const auto* display = std::get_if<protocol::Display>(&message)) {
        if (hmd_ != nullptr) {
            hmd_->ApplyDisplay(*display);
        } else {
            VRDUMMY_LOG_DEBUG("display update without hmd");
        }
        return;
    }
}

void DeviceProvider::PoseThread() {
    const auto period = std::chrono::microseconds(1'000'000 / settings_.pose_rate_hz);
    auto next = std::chrono::steady_clock::now();
    while (running_.load()) {
        for (auto& d : devices_) d->SubmitPose();
        next += period;
        std::this_thread::sleep_until(next);
    }
}

void DeviceProvider::ProcessEvent(const vr::VREvent_t& event) {
    if (event.eventType == vr::VREvent_Input_HapticVibration) {
        const auto& h = event.data.hapticVibration;
        VRDUMMY_LOG_DEBUG("haptic: container=%llu duration=%.3f frequency=%.1f amplitude=%.2f",
                          static_cast<unsigned long long>(h.containerHandle), static_cast<double>(h.fDurationSeconds),
                          static_cast<double>(h.fFrequency), static_cast<double>(h.fAmplitude));
    }
}

}  // namespace vrdummy
