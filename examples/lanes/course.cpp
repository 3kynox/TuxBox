#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include "course.h"
#include "course_shader.h"

using Microsoft::WRL::ComPtr;

namespace {
void check(HRESULT hr, const char* operation) {
    if (FAILED(hr)) {
        char code[16];
        std::snprintf(code, sizeof(code), "0x%08lx", static_cast<unsigned long>(hr));
        throw std::runtime_error(std::string(operation) + " failed with " + code);
    }
}
}

std::string generate_course(Course& rows, std::uint32_t seed, const wchar_t* selected) {
    ComPtr<IDXGIFactory1> factory;
    check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "DXGI factory");
    ComPtr<ID3D12Device> device;
    DXGI_ADAPTER_DESC1 gpu{};
    for (UINT n = 0; ; ++n) {
        ComPtr<IDXGIAdapter1> candidate;
        HRESULT hr = factory->EnumAdapters1(n, &candidate);
        if (hr == DXGI_ERROR_NOT_FOUND) throw std::runtime_error("no matching D3D12 hardware adapter");
        check(hr, "adapter enumeration");
        check(candidate->GetDesc1(&gpu), "adapter description");
        wchar_t id[32];
        std::swprintf(id, 32, L"%04x:%04x", gpu.VendorId, gpu.DeviceId);
        if (gpu.Flags & DXGI_ADAPTER_FLAG_SOFTWARE || (selected && _wcsicmp(id, selected))) continue;
        hr = D3D12CreateDevice(candidate.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device));
        if (SUCCEEDED(hr)) break;
        if (selected) check(hr, "desktop D3D12 device");
    }
    D3D12_ROOT_PARAMETER params[2]{};
    params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    params[0].Constants = {1, 2, 2};
    params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_UAV;
    params[1].Descriptor = {2, 1};
    D3D12_ROOT_SIGNATURE_DESC signature{};
    signature.NumParameters = 2;
    signature.pParameters = params;
    ComPtr<ID3DBlob> serialized;
    check(D3D12SerializeRootSignature(&signature, D3D_ROOT_SIGNATURE_VERSION_1, &serialized, nullptr),
          "Xbox-facing root serializer");
    ComPtr<ID3D12RootSignature> root;
    check(device->CreateRootSignature(0, serialized->GetBufferPointer(), serialized->GetBufferSize(),
                                      IID_PPV_ARGS(&root)), "course root signature");
    D3D12_COMPUTE_PIPELINE_STATE_DESC shader{};
    shader.pRootSignature = root.Get();
    shader.CS = {course_dxil, sizeof(course_dxil)};
    ComPtr<ID3D12PipelineState> pipeline;
    check(device->CreateComputePipelineState(&shader, IID_PPV_ARGS(&pipeline)), "course pipeline");
    D3D12_COMMAND_QUEUE_DESC q{};
    q.Type = D3D12_COMMAND_LIST_TYPE_COMPUTE;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> commands;
    check(device->CreateCommandQueue(&q, IID_PPV_ARGS(&queue)), "course queue");
    check(device->CreateCommandAllocator(q.Type, IID_PPV_ARGS(&allocator)), "course allocator");
    check(device->CreateCommandList(0, q.Type, allocator.Get(), pipeline.Get(), IID_PPV_ARGS(&commands)), "course list");

    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width = sizeof(rows);
    desc.Height = desc.DepthOrArraySize = desc.MipLevels = 1;
    desc.SampleDesc.Count = 1;
    desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    D3D12_HEAP_PROPERTIES heap{};
    heap.CreationNodeMask = heap.VisibleNodeMask = 1;
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    ComPtr<ID3D12Resource> result, readback;
    check(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                                          nullptr, IID_PPV_ARGS(&result)), "course output");
    heap.Type = D3D12_HEAP_TYPE_READBACK;
    desc.Flags = D3D12_RESOURCE_FLAG_NONE;
    check(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST,
                                          nullptr, IID_PPV_ARGS(&readback)), "course readback");
    const UINT constants[] = {seed, course_length};
    commands->SetComputeRootSignature(root.Get());
    commands->SetComputeRoot32BitConstants(0, 2, constants, 0);
    commands->SetComputeRootUnorderedAccessView(1, result->GetGPUVirtualAddress());
    commands->Dispatch((course_length + 7) / 8, 1, 1);
    D3D12_RESOURCE_BARRIER step{};
    step.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    step.Transition = {result.Get(), D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                       D3D12_RESOURCE_STATE_COPY_SOURCE};
    commands->ResourceBarrier(1, &step);
    commands->CopyBufferRegion(readback.Get(), 0, result.Get(), 0, sizeof(rows));
    check(commands->Close(), "course list close");
    ComPtr<ID3D12Fence> fence;
    check(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)), "course fence");
    ID3D12CommandList* batch[] = {commands.Get()};
    queue->ExecuteCommandLists(1, batch);
    check(queue->Signal(fence.Get(), 1), "course signal");
    HANDLE done = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!done) throw std::runtime_error("cannot create course completion event");
    HRESULT hr = fence->SetEventOnCompletion(1, done);
    DWORD wait = SUCCEEDED(hr) ? WaitForSingleObject(done, 15000) : WAIT_FAILED;
    CloseHandle(done);
    check(hr, "course completion notification");
    if (wait != WAIT_OBJECT_0 || fence->GetCompletedValue() != 1) throw std::runtime_error("course dispatch did not complete");
    D3D12_RANGE bytes{0, sizeof(rows)}, no_writes{};
    void* mapped = nullptr;
    check(readback->Map(0, &bytes, &mapped), "course readback map");
    std::memcpy(rows.data(), mapped, sizeof(rows));
    readback->Unmap(0, &no_writes);
    for (auto packed : rows)
        if ((packed & 255) > 2 || (packed >> 8) > 2 || (packed & 255) == (packed >> 8))
            throw std::runtime_error("GPU generated an invalid lane pair");
    char id[32];
    std::snprintf(id, sizeof(id), "%04x:%04x", gpu.VendorId, gpu.DeviceId);
    return id;
}
