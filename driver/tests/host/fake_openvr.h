// Fakes of the OpenVR driver-side interfaces that vrserver normally provides.
// They record every call so that tests can inspect what the driver did.
#pragma once

#include <cstring>
#include <map>
#include <mutex>
#include <string>
#include <variant>
#include <vector>

#include "openvr_driver.h"

namespace fake {

using SettingValue = std::variant<bool, int32_t, float, std::string>;

class Settings : public vr::IVRSettings {
public:
    std::map<std::string, std::map<std::string, SettingValue>> values;

    void Set(const char* section, const char* key, SettingValue v) { values[section][key] = std::move(v); }

    const char* GetSettingsErrorNameFromEnum(vr::EVRSettingsError) override { return "fake"; }
    void SetBool(const char* s, const char* k, bool v, vr::EVRSettingsError* e) override { Store(s, k, v, e); }
    void SetInt32(const char* s, const char* k, int32_t v, vr::EVRSettingsError* e) override { Store(s, k, v, e); }
    void SetFloat(const char* s, const char* k, float v, vr::EVRSettingsError* e) override { Store(s, k, v, e); }
    void SetString(const char* s, const char* k, const char* v, vr::EVRSettingsError* e) override {
        Store(s, k, std::string(v), e);
    }
    bool GetBool(const char* s, const char* k, vr::EVRSettingsError* e) override { return Get<bool>(s, k, e, false); }
    int32_t GetInt32(const char* s, const char* k, vr::EVRSettingsError* e) override { return Get<int32_t>(s, k, e, 0); }
    float GetFloat(const char* s, const char* k, vr::EVRSettingsError* e) override { return Get<float>(s, k, e, 0.0f); }
    void GetString(const char* s, const char* k, char* out, uint32_t len, vr::EVRSettingsError* e) override {
        const std::string v = Get<std::string>(s, k, e, std::string());
        if (len == 0) return;
        strncpy_s(out, len, v.c_str(), _TRUNCATE);
    }
    void RemoveSection(const char* s, vr::EVRSettingsError* e) override {
        values.erase(s);
        if (e) *e = vr::VRSettingsError_None;
    }
    void RemoveKeyInSection(const char* s, const char* k, vr::EVRSettingsError* e) override {
        values[s].erase(k);
        if (e) *e = vr::VRSettingsError_None;
    }

private:
    void Store(const char* s, const char* k, SettingValue v, vr::EVRSettingsError* e) {
        values[s][k] = std::move(v);
        if (e) *e = vr::VRSettingsError_None;
    }
    template <typename T>
    T Get(const char* s, const char* k, vr::EVRSettingsError* e, T fallback) {
        auto sec = values.find(s);
        if (sec != values.end()) {
            auto it = sec->second.find(k);
            if (it != sec->second.end() && std::holds_alternative<T>(it->second)) {
                if (e) *e = vr::VRSettingsError_None;
                return std::get<T>(it->second);
            }
        }
        if (e) *e = vr::VRSettingsError_UnsetSettingHasNoDefault;
        return fallback;
    }
};

class Properties : public vr::IVRProperties {
public:
    struct Value {
        vr::PropertyTypeTag_t tag = 0;
        std::vector<uint8_t> bytes;
    };
    static constexpr vr::PropertyContainerHandle_t kContainerBase = 1000;

    std::mutex mutex;
    std::map<std::pair<vr::PropertyContainerHandle_t, vr::ETrackedDeviceProperty>, Value> values;

    static vr::PropertyContainerHandle_t ContainerFor(uint32_t device_index) { return kContainerBase + device_index; }

    vr::ETrackedPropertyError ReadPropertyBatch(vr::PropertyContainerHandle_t, vr::PropertyRead_t* batch,
                                                uint32_t count) override {
        for (uint32_t i = 0; i < count; ++i) batch[i].eError = vr::TrackedProp_UnknownProperty;
        return vr::TrackedProp_UnknownProperty;
    }
    vr::ETrackedPropertyError WritePropertyBatch(vr::PropertyContainerHandle_t container, vr::PropertyWrite_t* batch,
                                                 uint32_t count) override {
        std::lock_guard<std::mutex> lock(mutex);
        for (uint32_t i = 0; i < count; ++i) {
            vr::PropertyWrite_t& w = batch[i];
            if (w.writeType == vr::PropertyWrite_Set) {
                Value v;
                v.tag = w.unTag;
                const auto* p = static_cast<const uint8_t*>(w.pvBuffer);
                v.bytes.assign(p, p + w.unBufferSize);
                values[{container, w.prop}] = std::move(v);
            } else if (w.writeType == vr::PropertyWrite_Erase) {
                values.erase({container, w.prop});
            }
            w.eError = vr::TrackedProp_Success;
        }
        return vr::TrackedProp_Success;
    }
    const char* GetPropErrorNameFromEnum(vr::ETrackedPropertyError) override { return "fake"; }
    vr::PropertyContainerHandle_t TrackedDeviceToPropertyContainer(vr::TrackedDeviceIndex_t device) override {
        return ContainerFor(device);
    }

    bool Has(uint32_t device_index, vr::ETrackedDeviceProperty prop) {
        std::lock_guard<std::mutex> lock(mutex);
        return values.count({ContainerFor(device_index), prop}) != 0;
    }
    std::string GetString(uint32_t device_index, vr::ETrackedDeviceProperty prop) {
        std::lock_guard<std::mutex> lock(mutex);
        auto it = values.find({ContainerFor(device_index), prop});
        if (it == values.end() || it->second.tag != vr::k_unStringPropertyTag) return "";
        const auto& b = it->second.bytes;
        size_t n = 0;
        while (n < b.size() && b[n] != 0) ++n;
        return std::string(reinterpret_cast<const char*>(b.data()), n);
    }
    template <typename T>
    T GetScalar(uint32_t device_index, vr::ETrackedDeviceProperty prop, vr::PropertyTypeTag_t tag, T fallback) {
        std::lock_guard<std::mutex> lock(mutex);
        auto it = values.find({ContainerFor(device_index), prop});
        if (it == values.end() || it->second.tag != tag || it->second.bytes.size() != sizeof(T)) return fallback;
        T v;
        std::memcpy(&v, it->second.bytes.data(), sizeof(T));
        return v;
    }
    float GetFloat(uint32_t d, vr::ETrackedDeviceProperty p) { return GetScalar<float>(d, p, vr::k_unFloatPropertyTag, 0.0f); }
    int32_t GetInt32(uint32_t d, vr::ETrackedDeviceProperty p) { return GetScalar<int32_t>(d, p, vr::k_unInt32PropertyTag, 0); }
    bool GetBool(uint32_t d, vr::ETrackedDeviceProperty p) { return GetScalar<bool>(d, p, vr::k_unBoolPropertyTag, false); }
};

class DriverInput : public vr::IVRDriverInput {
public:
    struct Component {
        vr::PropertyContainerHandle_t container = 0;
        std::string name;
        enum Kind { Bool, Scalar, Haptic, Other } kind = Other;
        bool bool_value = false;
        float scalar_value = 0.0f;
        int updates = 0;
    };

    std::mutex mutex;
    std::map<vr::VRInputComponentHandle_t, Component> components;
    vr::VRInputComponentHandle_t next_handle = 1;

    vr::EVRInputError CreateBooleanComponent(vr::PropertyContainerHandle_t c, const char* name,
                                             vr::VRInputComponentHandle_t* h) override {
        return Create(c, name, Component::Bool, h);
    }
    vr::EVRInputError UpdateBooleanComponent(vr::VRInputComponentHandle_t h, bool v, double) override {
        std::lock_guard<std::mutex> lock(mutex);
        auto it = components.find(h);
        if (it == components.end() || it->second.kind != Component::Bool) return vr::VRInputError_InvalidHandle;
        it->second.bool_value = v;
        it->second.updates++;
        return vr::VRInputError_None;
    }
    vr::EVRInputError CreateScalarComponent(vr::PropertyContainerHandle_t c, const char* name,
                                            vr::VRInputComponentHandle_t* h, vr::EVRScalarType,
                                            vr::EVRScalarUnits) override {
        return Create(c, name, Component::Scalar, h);
    }
    vr::EVRInputError UpdateScalarComponent(vr::VRInputComponentHandle_t h, float v, double) override {
        std::lock_guard<std::mutex> lock(mutex);
        auto it = components.find(h);
        if (it == components.end() || it->second.kind != Component::Scalar) return vr::VRInputError_InvalidHandle;
        it->second.scalar_value = v;
        it->second.updates++;
        return vr::VRInputError_None;
    }
    vr::EVRInputError CreateHapticComponent(vr::PropertyContainerHandle_t c, const char* name,
                                            vr::VRInputComponentHandle_t* h) override {
        return Create(c, name, Component::Haptic, h);
    }
    vr::EVRInputError CreateSkeletonComponent(vr::PropertyContainerHandle_t c, const char* name, const char*,
                                              const char*, vr::EVRSkeletalTrackingLevel,
                                              const vr::VRBoneTransform_t*, uint32_t,
                                              vr::VRInputComponentHandle_t* h) override {
        return Create(c, name, Component::Other, h);
    }
    vr::EVRInputError UpdateSkeletonComponent(vr::VRInputComponentHandle_t, vr::EVRSkeletalMotionRange,
                                              const vr::VRBoneTransform_t*, uint32_t) override {
        return vr::VRInputError_None;
    }
    vr::EVRInputError CreatePoseComponent(vr::PropertyContainerHandle_t c, const char* name,
                                          vr::VRInputComponentHandle_t* h) override {
        return Create(c, name, Component::Other, h);
    }
    vr::EVRInputError UpdatePoseComponent(vr::VRInputComponentHandle_t, const vr::HmdMatrix34_t*, double) override {
        return vr::VRInputError_None;
    }
    vr::EVRInputError CreateEyeTrackingComponent(vr::PropertyContainerHandle_t c, const char* name,
                                                 vr::VRInputComponentHandle_t* h) override {
        return Create(c, name, Component::Other, h);
    }
    vr::EVRInputError UpdateEyeTrackingComponent(vr::VRInputComponentHandle_t, const vr::VREyeTrackingData_t*,
                                                 double) override {
        return vr::VRInputError_None;
    }

    // Finds a component by device index and path.
    Component Find(uint32_t device_index, const std::string& name) {
        std::lock_guard<std::mutex> lock(mutex);
        for (const auto& [h, c] : components) {
            if (c.container == Properties::ContainerFor(device_index) && c.name == name) return c;
        }
        return Component{};
    }
    size_t CountFor(uint32_t device_index) {
        std::lock_guard<std::mutex> lock(mutex);
        size_t n = 0;
        for (const auto& [h, c] : components) {
            if (c.container == Properties::ContainerFor(device_index)) ++n;
        }
        return n;
    }

private:
    vr::EVRInputError Create(vr::PropertyContainerHandle_t c, const char* name, Component::Kind kind,
                             vr::VRInputComponentHandle_t* h) {
        std::lock_guard<std::mutex> lock(mutex);
        Component comp;
        comp.container = c;
        comp.name = name;
        comp.kind = kind;
        *h = next_handle++;
        components[*h] = comp;
        return vr::VRInputError_None;
    }
};

class DriverLog : public vr::IVRDriverLog {
public:
    std::mutex mutex;
    std::vector<std::string> lines;
    void Log(const char* message) override {
        std::lock_guard<std::mutex> lock(mutex);
        lines.emplace_back(message);
    }
    bool Contains(const std::string& needle) {
        std::lock_guard<std::mutex> lock(mutex);
        for (const auto& l : lines) {
            if (l.find(needle) != std::string::npos) return true;
        }
        return false;
    }
};

class ServerDriverHost : public vr::IVRServerDriverHost {
public:
    struct Device {
        std::string serial;
        vr::ETrackedDeviceClass device_class = vr::TrackedDeviceClass_Invalid;
        vr::ITrackedDeviceServerDriver* driver = nullptr;
    };

    std::mutex mutex;
    std::vector<Device> devices;
    std::map<uint32_t, vr::DriverPose_t> last_pose;
    std::map<uint32_t, uint64_t> pose_updates;
    // vrserver activates devices after TrackedDeviceAdded; the fake does it inline.
    bool activate_on_add = true;

    bool TrackedDeviceAdded(const char* serial, vr::ETrackedDeviceClass device_class,
                            vr::ITrackedDeviceServerDriver* driver) override {
        uint32_t index = 0;
        {
            std::lock_guard<std::mutex> lock(mutex);
            index = static_cast<uint32_t>(devices.size());
            devices.push_back(Device{serial, device_class, driver});
        }
        if (activate_on_add) driver->Activate(index);
        return true;
    }
    void TrackedDevicePoseUpdated(uint32_t device, const vr::DriverPose_t& pose, uint32_t size) override {
        if (size != sizeof(vr::DriverPose_t)) return;
        std::lock_guard<std::mutex> lock(mutex);
        last_pose[device] = pose;
        pose_updates[device]++;
    }
    void VsyncEvent(double) override {}
    void VendorSpecificEvent(uint32_t, vr::EVREventType, const vr::VREvent_Data_t&, double) override {}
    bool IsExiting() override { return false; }
    bool PollNextEvent(vr::VREvent_t*, uint32_t) override { return false; }
    void GetRawTrackedDevicePoses(float, vr::TrackedDevicePose_t*, uint32_t) override {}
    void RequestRestart(const char*, const char*, const char*, const char*) override {}
    uint32_t GetFrameTimings(vr::Compositor_FrameTiming*, uint32_t) override { return 0; }
    void SetDisplayEyeToHead(uint32_t, const vr::HmdMatrix34_t&, const vr::HmdMatrix34_t&) override {}
    void SetDisplayProjectionRaw(uint32_t, const vr::HmdRect2_t&, const vr::HmdRect2_t&) override {}
    void SetRecommendedRenderTargetSize(uint32_t, uint32_t, uint32_t) override {}

    int FindIndex(const std::string& serial) {
        std::lock_guard<std::mutex> lock(mutex);
        for (size_t i = 0; i < devices.size(); ++i) {
            if (devices[i].serial == serial) return static_cast<int>(i);
        }
        return -1;
    }
    bool GetPose(uint32_t index, vr::DriverPose_t* out) {
        std::lock_guard<std::mutex> lock(mutex);
        auto it = last_pose.find(index);
        if (it == last_pose.end()) return false;
        *out = it->second;
        return true;
    }
    uint64_t Updates(uint32_t index) {
        std::lock_guard<std::mutex> lock(mutex);
        auto it = pose_updates.find(index);
        return it == pose_updates.end() ? 0 : it->second;
    }
};

class DriverManager : public vr::IVRDriverManager {
public:
    uint32_t GetDriverCount() const override { return 1; }
    uint32_t GetDriverName(vr::DriverId_t, char* out, uint32_t len) override {
        if (len > 0) strncpy_s(out, len, "vrdummy", _TRUNCATE);
        return 8;
    }
    vr::DriverHandle_t GetDriverHandle(const char*) override { return 1; }
    bool IsEnabled(vr::DriverId_t) const override { return true; }
};

class Resources : public vr::IVRResources {
public:
    uint32_t LoadSharedResource(const char*, char*, uint32_t) override { return 0; }
    uint32_t GetResourceFullPath(const char*, const char*, char* out, uint32_t len) override {
        if (len > 0) out[0] = 0;
        return 0;
    }
};

// Driver context handing out the fakes above by interface version string.
class DriverContext : public vr::IVRDriverContext {
public:
    Settings settings;
    Properties properties;
    DriverInput input;
    DriverLog log;
    ServerDriverHost host;
    DriverManager manager;
    Resources resources;

    void* GetGenericInterface(const char* version, vr::EVRInitError* error) override {
        if (error) *error = vr::VRInitError_None;
        if (std::strcmp(version, vr::IVRSettings_Version) == 0) return static_cast<vr::IVRSettings*>(&settings);
        if (std::strcmp(version, vr::IVRProperties_Version) == 0) return static_cast<vr::IVRProperties*>(&properties);
        if (std::strcmp(version, vr::IVRDriverInput_Version) == 0) return static_cast<vr::IVRDriverInput*>(&input);
        if (std::strcmp(version, vr::IVRDriverLog_Version) == 0) return static_cast<vr::IVRDriverLog*>(&log);
        if (std::strcmp(version, vr::IVRServerDriverHost_Version) == 0) return static_cast<vr::IVRServerDriverHost*>(&host);
        if (std::strcmp(version, vr::IVRDriverManager_Version) == 0) return static_cast<vr::IVRDriverManager*>(&manager);
        if (std::strcmp(version, vr::IVRResources_Version) == 0) return static_cast<vr::IVRResources*>(&resources);
        if (error) *error = vr::VRInitError_Init_InterfaceNotFound;
        return nullptr;
    }
    vr::DriverHandle_t GetDriverHandle() override { return 1; }
};

}  // namespace fake
