#include "renderer.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <d3d11.h>
#include <d3dcompiler.h>
#include <dxgi.h>
#include <windows.h>

#include <cstdio>
#include <cstring>

namespace vrscene {

namespace {

const char* kShader = R"(
cbuffer Frame : register(b0) { row_major float4x4 view_proj; };
struct VSIn { float3 pos : POSITION; float4 col : COLOR; };
struct VSOut { float4 pos : SV_POSITION; float4 col : COLOR; };
VSOut vs_main(VSIn i) { VSOut o; o.pos = mul(view_proj, float4(i.pos, 1.0)); o.col = i.col; return o; }
float4 ps_main(VSOut i) : SV_TARGET { return i.col; }
)";

template <typename T>
void Release(T*& p) {
    if (p) {
        p->Release();
        p = nullptr;
    }
}

bool Compile(const char* entry, const char* target, ID3DBlob** blob) {
    ID3DBlob* errors = nullptr;
    const HRESULT hr = D3DCompile(kShader, std::strlen(kShader), "scene.hlsl", nullptr, nullptr, entry, target,
                                  D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, blob, &errors);
    if (FAILED(hr)) {
        std::fprintf(stderr, "shader %s: %s\n", entry, errors ? static_cast<const char*>(errors->GetBufferPointer()) : "?");
        Release(errors);
        return false;
    }
    Release(errors);
    return true;
}

}  // namespace

Renderer::~Renderer() {
    Release(raster_);
    Release(layout_);
    Release(ps_);
    Release(vs_);
    Release(constant_buffer_);
    Release(vertex_buffer_);
    Release(depth_dsv_);
    Release(depth_tex_);
    for (auto& r : eye_rtv_) Release(r);
    for (auto& t : eye_tex_) Release(t);
    Release(context_);
    Release(device_);
}

bool Renderer::Init(vr::IVRSystem* sys) {
    int32_t adapter_index = -1;
    sys->GetDXGIOutputInfo(&adapter_index);
    IDXGIFactory1* factory = nullptr;
    IDXGIAdapter1* adapter = nullptr;
    if (adapter_index >= 0 && SUCCEEDED(CreateDXGIFactory1(__uuidof(IDXGIFactory1), reinterpret_cast<void**>(&factory)))) {
        if (FAILED(factory->EnumAdapters1(static_cast<UINT>(adapter_index), &adapter))) adapter = nullptr;
    }
    const D3D_FEATURE_LEVEL levels[] = {D3D_FEATURE_LEVEL_11_0};
    HRESULT hr = D3D11CreateDevice(adapter, adapter ? D3D_DRIVER_TYPE_UNKNOWN : D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, levels, 1,
                                   D3D11_SDK_VERSION, &device_, nullptr, &context_);
    Release(adapter);
    Release(factory);
    if (FAILED(hr)) {
        std::fprintf(stderr, "D3D11CreateDevice failed: 0x%08lx\n", static_cast<unsigned long>(hr));
        return false;
    }

    sys->GetRecommendedRenderTargetSize(&width_, &height_);
    D3D11_TEXTURE2D_DESC td{};
    td.Width = width_;
    td.Height = height_;
    td.MipLevels = 1;
    td.ArraySize = 1;
    td.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_DEFAULT;
    td.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    td.MiscFlags = D3D11_RESOURCE_MISC_SHARED;  // the compositor opens the texture from its process
    for (int eye = 0; eye < 2; ++eye) {
        if (FAILED(device_->CreateTexture2D(&td, nullptr, &eye_tex_[eye])) ||
            FAILED(device_->CreateRenderTargetView(eye_tex_[eye], nullptr, &eye_rtv_[eye]))) {
            std::fprintf(stderr, "eye render target creation failed\n");
            return false;
        }
    }
    D3D11_TEXTURE2D_DESC dd = td;
    dd.Format = DXGI_FORMAT_D32_FLOAT;
    dd.BindFlags = D3D11_BIND_DEPTH_STENCIL;
    dd.MiscFlags = 0;
    if (FAILED(device_->CreateTexture2D(&dd, nullptr, &depth_tex_)) ||
        FAILED(device_->CreateDepthStencilView(depth_tex_, nullptr, &depth_dsv_))) {
        std::fprintf(stderr, "depth buffer creation failed\n");
        return false;
    }

    ID3DBlob* vs = nullptr;
    ID3DBlob* ps = nullptr;
    if (!Compile("vs_main", "vs_5_0", &vs) || !Compile("ps_main", "ps_5_0", &ps)) return false;
    device_->CreateVertexShader(vs->GetBufferPointer(), vs->GetBufferSize(), nullptr, &vs_);
    device_->CreatePixelShader(ps->GetBufferPointer(), ps->GetBufferSize(), nullptr, &ps_);
    const D3D11_INPUT_ELEMENT_DESC elements[] = {
        {"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"COLOR", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 12, D3D11_INPUT_PER_VERTEX_DATA, 0},
    };
    hr = device_->CreateInputLayout(elements, 2, vs->GetBufferPointer(), vs->GetBufferSize(), &layout_);
    Release(vs);
    Release(ps);
    if (FAILED(hr)) return false;

    D3D11_BUFFER_DESC cb{};
    cb.ByteWidth = sizeof(Mat4);
    cb.Usage = D3D11_USAGE_DYNAMIC;
    cb.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    cb.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    if (FAILED(device_->CreateBuffer(&cb, nullptr, &constant_buffer_))) return false;

    D3D11_RASTERIZER_DESC rd{};
    rd.FillMode = D3D11_FILL_SOLID;
    rd.CullMode = D3D11_CULL_NONE;
    rd.DepthClipEnable = TRUE;
    rd.AntialiasedLineEnable = TRUE;
    device_->CreateRasterizerState(&rd, &raster_);
    return true;
}

void Renderer::RenderEye(vr::IVRSystem* sys, vr::EVREye eye, const Mat4& head_to_world, const Lines& lines) {
    const Mat4 eye_to_head = Mat4::From34(sys->GetEyeToHeadTransform(eye));
    const Mat4 proj = Mat4::From44(sys->GetProjectionMatrix(eye, 0.05f, 50.0f));
    const Mat4 view_proj = proj * (head_to_world * eye_to_head).RigidInverse();

    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (SUCCEEDED(context_->Map(constant_buffer_, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) {
        std::memcpy(mapped.pData, &view_proj, sizeof(view_proj));
        context_->Unmap(constant_buffer_, 0);
    }

    const int i = eye == vr::Eye_Left ? 0 : 1;
    const float clear[4] = {0.05f, 0.05f, 0.08f, 1.0f};
    context_->OMSetRenderTargets(1, &eye_rtv_[i], depth_dsv_);
    context_->ClearRenderTargetView(eye_rtv_[i], clear);
    context_->ClearDepthStencilView(depth_dsv_, D3D11_CLEAR_DEPTH, 1.0f, 0);
    D3D11_VIEWPORT vp{};
    vp.Width = static_cast<float>(width_);
    vp.Height = static_cast<float>(height_);
    vp.MaxDepth = 1.0f;
    context_->RSSetViewports(1, &vp);
    context_->RSSetState(raster_);
    context_->IASetInputLayout(layout_);
    context_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_LINELIST);
    const UINT stride = sizeof(Lines::Vertex);
    const UINT offset = 0;
    context_->IASetVertexBuffers(0, 1, &vertex_buffer_, &stride, &offset);
    context_->VSSetShader(vs_, nullptr, 0);
    context_->VSSetConstantBuffers(0, 1, &constant_buffer_);
    context_->PSSetShader(ps_, nullptr, 0);
    context_->Draw(static_cast<UINT>(lines.v.size()), 0);
}

void Renderer::RenderAndSubmit(vr::IVRSystem* sys, const vr::HmdMatrix34_t& hmd_pose, const Lines& lines) {
    if (lines.v.empty()) return;
    if (lines.v.size() > vertex_capacity_) {
        Release(vertex_buffer_);
        vertex_capacity_ = lines.v.size() * 2;
        D3D11_BUFFER_DESC vb{};
        vb.ByteWidth = static_cast<UINT>(vertex_capacity_ * sizeof(Lines::Vertex));
        vb.Usage = D3D11_USAGE_DYNAMIC;
        vb.BindFlags = D3D11_BIND_VERTEX_BUFFER;
        vb.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
        if (FAILED(device_->CreateBuffer(&vb, nullptr, &vertex_buffer_))) return;
    }
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (SUCCEEDED(context_->Map(vertex_buffer_, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) {
        std::memcpy(mapped.pData, lines.v.data(), lines.v.size() * sizeof(Lines::Vertex));
        context_->Unmap(vertex_buffer_, 0);
    }

    const Mat4 head_to_world = Mat4::From34(hmd_pose);
    for (vr::EVREye eye : {vr::Eye_Left, vr::Eye_Right}) {
        RenderEye(sys, eye, head_to_world, lines);
        vr::Texture_t tex{};
        tex.handle = eye_tex_[eye == vr::Eye_Left ? 0 : 1];
        tex.eType = vr::TextureType_DirectX;
        tex.eColorSpace = vr::ColorSpace_Gamma;
        vr::VRCompositor()->Submit(eye, &tex);
    }
    context_->Flush();
}

}  // namespace vrscene
