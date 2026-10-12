// Minimal Direct3D 11 stereo renderer: draws a line list into one render
// target per eye and submits both to the OpenVR compositor.
#pragma once

#include <cstdint>

#include "openvr.h"
#include "scene.h"

struct ID3D11Device;
struct ID3D11DeviceContext;
struct ID3D11Texture2D;
struct ID3D11RenderTargetView;
struct ID3D11DepthStencilView;
struct ID3D11Buffer;
struct ID3D11VertexShader;
struct ID3D11PixelShader;
struct ID3D11InputLayout;
struct ID3D11RasterizerState;

namespace vrscene {

class Renderer {
public:
    Renderer() = default;
    ~Renderer();
    Renderer(const Renderer&) = delete;
    Renderer& operator=(const Renderer&) = delete;

    // Creates the device on the adapter SteamVR reports and the eye targets
    // at the recommended size. Returns false with a message on stderr.
    bool Init(vr::IVRSystem* sys);

    // Renders the lines for both eyes from the given HMD pose and submits
    // the frames. Call once per WaitGetPoses.
    void RenderAndSubmit(vr::IVRSystem* sys, const vr::HmdMatrix34_t& hmd_pose, const Lines& lines);

    uint32_t width() const { return width_; }
    uint32_t height() const { return height_; }

private:
    void RenderEye(vr::IVRSystem* sys, vr::EVREye eye, const Mat4& head_to_world, const Lines& lines);

    ID3D11Device* device_ = nullptr;
    ID3D11DeviceContext* context_ = nullptr;
    ID3D11Texture2D* eye_tex_[2] = {nullptr, nullptr};
    ID3D11RenderTargetView* eye_rtv_[2] = {nullptr, nullptr};
    ID3D11Texture2D* depth_tex_ = nullptr;
    ID3D11DepthStencilView* depth_dsv_ = nullptr;
    ID3D11Buffer* vertex_buffer_ = nullptr;
    ID3D11Buffer* constant_buffer_ = nullptr;
    ID3D11VertexShader* vs_ = nullptr;
    ID3D11PixelShader* ps_ = nullptr;
    ID3D11InputLayout* layout_ = nullptr;
    ID3D11RasterizerState* raster_ = nullptr;
    uint32_t width_ = 0;
    uint32_t height_ = 0;
    size_t vertex_capacity_ = 0;
};

}  // namespace vrscene
