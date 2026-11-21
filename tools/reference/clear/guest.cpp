// Execute the actual cached Frosty ClearCS DXIL using its cooked descriptor tables.
#include "../compute_buffers.h"
#include "clear_blobs.h"
#include <algorithm>
#include <cstring>
#include <vector>

using namespace compute_buffers;

static void run(ID3D12Device* device, ID3D12RootSignature* root, ID3D12PipelineState* pso, UINT components,
                bool integer_view, FILE* receipt) {
    constexpr UINT count = 140, start = 5, end = 102;
    const std::array<UINT, 4> color = integer_view
                                          ? std::array<UINT, 4>{0x01234567, 0x00000001, 0xdeadbeef, 0xffffffff}
                                          : std::array<UINT, 4>{0x3e800000, 0xbf000000, 0x3fc00000, 0x41000000};
    const UINT bytes = count * components * sizeof(UINT);
    std::vector<UINT> expected(count * components, 0xdeadc0de);
    auto initial = buffer(device, bytes, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
    auto params = buffer(device, 256, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
    auto output = buffer(device, bytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST,
                         D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
    auto readback = buffer(device, bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
    std::array<UINT, 64> cb{};
    std::copy(color.begin(), color.end(), cb.begin());
    cb[4] = start;
    cb[8] = end;
    upload(params.Get(), cb.data(), sizeof(cb));
    upload(initial.Get(), expected.data(), bytes);
    D3D12_DESCRIPTOR_HEAP_DESC desc{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 32,
                                    D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, 0};
    ComPtr<ID3D12DescriptorHeap> heap;
    check(device->CreateDescriptorHeap(&desc, IID_PPV_ARGS(&heap)), "CreateDescriptorHeap");
    auto cpu = heap->GetCPUDescriptorHandleForHeapStart();
    auto gpu = heap->GetGPUDescriptorHandleForHeapStart();
    UINT stride = device->GetDescriptorHandleIncrementSize(desc.Type);
    D3D12_CONSTANT_BUFFER_VIEW_DESC cbv{params->GetGPUVirtualAddress(), 256};
    device->CreateConstantBufferView(&cbv, cpu);
    D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};
    uav.Format = integer_view ? (components == 4 ? DXGI_FORMAT_R32G32B32A32_UINT : DXGI_FORMAT_R32_UINT)
                              : (components == 4 ? DXGI_FORMAT_R32G32B32A32_FLOAT : DXGI_FORMAT_R32_FLOAT);
    uav.ViewDimension = D3D12_UAV_DIMENSION_BUFFER;
    uav.Buffer.NumElements = count;
    device->CreateUnorderedAccessView(output.Get(), nullptr, &uav, {cpu.ptr + 16 * stride});
    D3D12_COMMAND_QUEUE_DESC queue_desc{};
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    check(device->CreateCommandQueue(&queue_desc, IID_PPV_ARGS(&queue)), "CreateCommandQueue");
    check(device->CreateCommandAllocator(queue_desc.Type, IID_PPV_ARGS(&allocator)), "CreateCommandAllocator");
    check(device->CreateCommandList(0, queue_desc.Type, allocator.Get(), pso, IID_PPV_ARGS(&list)),
          "CreateCommandList");
    list->CopyBufferRegion(output.Get(), 0, initial.Get(), 0, bytes);
    transition(list.Get(), output.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    ID3D12DescriptorHeap* heaps[] = {heap.Get()};
    list->SetDescriptorHeaps(1, heaps);
    list->SetComputeRootSignature(root);
    list->SetComputeRootDescriptorTable(1, gpu);
    list->SetComputeRootDescriptorTable(3, {gpu.ptr + 16 * stride});
    list->Dispatch(2, 1, 1);
    transition(list.Get(), output.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
    list->CopyBufferRegion(readback.Get(), 0, output.Get(), 0, bytes);
    submit_and_wait(device, queue.Get(), list.Get());
    for (UINT i = start; i < end; ++i) {
        std::copy_n(color.begin(), components, expected.begin() + i * components);
    }
    void* mapped{};
    D3D12_RANGE read{0, bytes}, empty{};
    check(readback->Map(0, &read, &mapped), "Map readback");
    auto* actual = static_cast<const UINT*>(mapped);
    UINT matched = 0;
    for (UINT i = 0; i < expected.size(); ++i)
        matched += (actual[i] == expected[i]);
    readback->Unmap(0, &empty);
    std::fprintf(receipt, "%s_%u_matched_words=%u\n", integer_view ? "uint_view" : "float_view", components,
                 matched);
    if (!integer_view && matched != expected.size())
        throw std::runtime_error("GPU float output differs from CPU oracle");
    std::fflush(receipt);
}

int main() {
    FILE* receipt{};
    if (fopen_s(&receipt, "readback.txt", "wb"))
        return 2;
    int status = 0;
    try {
        ComPtr<ID3D12Device> device;
        check(D3D12CreateDevice(nullptr, D3D_FEATURE_LEVEL_12_0, IID_PPV_ARGS(&device)), "D3D12CreateDevice");
        ComPtr<IDXGIFactory4> factory;
        ComPtr<IDXGIAdapter1> adapter;
        DXGI_ADAPTER_DESC1 gpu{};
        check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "CreateDXGIFactory1");
        check(factory->EnumAdapterByLuid(device->GetAdapterLuid(), IID_PPV_ARGS(&adapter)), "EnumAdapterByLuid");
        check(adapter->GetDesc1(&gpu), "GetDesc1");
        std::fprintf(receipt, "adapter=%04x:%04x\n", gpu.VendorId, gpu.DeviceId);
        ComPtr<ID3D12RootSignature> root;
        check(device->CreateRootSignature(0, cooked_shader, sizeof(cooked_shader), IID_PPV_ARGS(&root)),
              "CreateRootSignature");
        D3D12_COMPUTE_PIPELINE_STATE_DESC desc{};
        desc.pRootSignature = root.Get();
        desc.CS = {desktop_shader, sizeof(desktop_shader)};
        ComPtr<ID3D12PipelineState> pso;
        check(device->CreateComputePipelineState(&desc, IID_PPV_ARGS(&pso)), "CreateComputePipelineState");
        run(device.Get(), root.Get(), pso.Get(), 4, false, receipt);
        run(device.Get(), root.Get(), pso.Get(), 1, false, receipt);
        // The cached shader declares a float UAV. Measure the unsigned view separately
        // since the retail caller is RHIClearUAVUint and a driver can treat it differently.
        run(device.Get(), root.Get(), pso.Get(), 4, true, receipt);
        run(device.Get(), root.Get(), pso.Get(), 1, true, receipt);
    } catch (const std::exception& error) {
        std::fprintf(receipt, "error=%s\n", error.what());
        status = 1;
    }
    return fclose(receipt) ? 2 : status;
}
