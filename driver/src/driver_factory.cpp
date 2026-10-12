// Entry point called by vrserver.
#include <cstring>

#include "device_provider.h"
#include "openvr_driver.h"

#if defined(_WIN32)
#define VRDUMMY_EXPORT extern "C" __declspec(dllexport)
#else
#define VRDUMMY_EXPORT extern "C" __attribute__((visibility("default")))
#endif

namespace {
vrdummy::DeviceProvider g_provider;
}

VRDUMMY_EXPORT void* HmdDriverFactory(const char* interface_name, int* return_code) {
    if (std::strcmp(vr::IServerTrackedDeviceProvider_Version, interface_name) == 0) {
        return &g_provider;
    }
    if (return_code != nullptr) *return_code = vr::VRInitError_Init_InterfaceNotFound;
    return nullptr;
}
