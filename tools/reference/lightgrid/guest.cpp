// Dispatch the original LightGridInjectionCS in one 4x4x4 group with sparse active cells.
#include "../compute_buffers.h"
#include "lightgrid_blobs.h"
#include <vector>
using namespace compute_buffers;

static void run(FILE* out, bool small_light) {
    ComPtr<ID3D12Device> dev;
    check(D3D12CreateDevice(nullptr, D3D_FEATURE_LEVEL_12_0, IID_PPV_ARGS(&dev)), "create device");
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGIAdapter1> adapter;
    DXGI_ADAPTER_DESC1 gpu{};
    check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "create DXGI");
    check(factory->EnumAdapterByLuid(dev->GetAdapterLuid(), IID_PPV_ARGS(&adapter)), "enumerate adapter");
    check(adapter->GetDesc1(&gpu), "adapter");
    std::fprintf(out, "adapter=%04x:%04x\n", gpu.VendorId, gpu.DeviceId);
    D3D12_FEATURE_DATA_D3D12_OPTIONS1 wave{};
    check(dev->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS1, &wave, sizeof(wave)), "query wave size");
    std::fprintf(out, "wave_min=%u\nwave_max=%u\n", wave.WaveLaneCountMin, wave.WaveLaneCountMax);
    ComPtr<ID3D12RootSignature> root;
    check(dev->CreateRootSignature(0, cooked_shader, sizeof(cooked_shader), IID_PPV_ARGS(&root)),
          "create original root");
    D3D12_COMPUTE_PIPELINE_STATE_DESC pd{};
    pd.pRootSignature = root.Get();
    pd.CS = {desktop_shader, sizeof(desktop_shader)};
    ComPtr<ID3D12PipelineState> pso;
    check(dev->CreateComputePipelineState(&pd, IID_PPV_ARGS(&pso)), "create native wave64 PSO");
    const auto upload_heap = [&](UINT size) {
        return buffer(dev.Get(), size, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
    };
    std::vector<UINT> b0(1024), b1(16384), b2(64);
    std::array<float, 4> sphere{0, 0, 0, 1000000}, direction{};
    constexpr UINT one = 0x3f800000, cells = 24, first = 5, sentinel = 0xdeadbeef;
    // Finite screen/depth transforms, one local sphere and one capture volume.
    for (UINT i = 0; i < 4; ++i) {
        b0[4 * (36 + i) + i] = one;
        b0[4 * (12 + i) + i] = one;
    }
    b0[4 * 31 + 2] = one;
    b0[4 * 31 + 3] = one;
    b0[4 * 30 + 2] = one;
    b0[4 * 65 + 2] = one;
    b0[4 * 65 + 3] = one;
    b0[4 * 134 + 2] = one;
    b0[4 * 134 + 3] = one;
    std::memcpy(&b1[3], &sphere[3], 4);
    b2[0] = 1;
    b2[1] = 1;
    b2[3] = cells;
    b2[4] = 3;
    b2[5] = 2;
    b2[6] = 4;
    b2[7] = 4;
    b2[12] = one;
    b2[14] = one;
    auto params0 = upload_heap(4096), params1 = upload_heap(65536), params2 = upload_heap(256);
    upload(params0.Get(), b0.data(), 4096);
    upload(params1.Get(), b1.data(), 65536);
    upload(params2.Get(), b2.data(), 256);
    auto lights = upload_heap(16), directions = upload_heap(16);
    if (small_light)
        sphere = {4.0f, 0.0f, 6.0f, 0.25f};
    upload(lights.Get(), sphere.data(), 16);
    upload(directions.Get(), direction.data(), 16);
    constexpr UINT words = 512, bytes = words * 4, heads = 16, links = 128;
    std::vector<UINT> initial(words, sentinel);
    initial[0] = first;
    for (UINT i = 0; i < cells * 2; ++i)
        initial[heads + i] = 0xcc000000u | i;
    auto seed = upload_heap(bytes);
    upload(seed.Get(), initial.data(), bytes);
    auto dst = buffer(dev.Get(), bytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST,
                      D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
    auto read = buffer(dev.Get(), bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, 96, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                                  0};
    ComPtr<ID3D12DescriptorHeap> heap;
    check(dev->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&heap)), "create views");
    const UINT step = dev->GetDescriptorHandleIncrementSize(hd.Type);
    auto cpu = [&](UINT i) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        return {heap->GetCPUDescriptorHandleForHeapStart().ptr + i * step};
    };
    auto handle = [&](UINT i) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        return {heap->GetGPUDescriptorHandleForHeapStart().ptr + i * step};
    };
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format = DXGI_FORMAT_R32G32B32A32_FLOAT;
    srv.ViewDimension = D3D12_SRV_DIMENSION_BUFFER;
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.Buffer.NumElements = 1;
    dev->CreateShaderResourceView(lights.Get(), &srv, cpu(0));
    dev->CreateShaderResourceView(directions.Get(), &srv, cpu(1));
    const D3D12_CONSTANT_BUFFER_VIEW_DESC cbvs[] = {{params0->GetGPUVirtualAddress(), 4096},
                                                    {params1->GetGPUVirtualAddress(), 65536},
                                                    {params2->GetGPUVirtualAddress(), 256}};
    for (UINT i = 0; i < 3; ++i)
        dev->CreateConstantBufferView(&cbvs[i], cpu(64 + i));
    D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};
    uav.Format = DXGI_FORMAT_R32_UINT;
    uav.ViewDimension = D3D12_UAV_DIMENSION_BUFFER;
    const UINT starts[] = {0, heads, links}, counts[] = {1, 2 * cells, words - links};
    for (UINT i = 0; i < 3; ++i) {
        uav.Buffer.FirstElement = starts[i];
        uav.Buffer.NumElements = counts[i];
        dev->CreateUnorderedAccessView(dst.Get(), nullptr, &uav, cpu(80 + i));
    }
    D3D12_COMMAND_QUEUE_DESC q{};
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12CommandAllocator> alloc;
    ComPtr<ID3D12GraphicsCommandList> list;
    check(dev->CreateCommandQueue(&q, IID_PPV_ARGS(&queue)), "create queue");
    check(dev->CreateCommandAllocator(q.Type, IID_PPV_ARGS(&alloc)), "create allocator");
    check(dev->CreateCommandList(0, q.Type, alloc.Get(), pso.Get(), IID_PPV_ARGS(&list)), "create list");
    list->CopyBufferRegion(dst.Get(), 0, seed.Get(), 0, bytes);
    transition(list.Get(), dst.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    list->SetDescriptorHeaps(1, heap.GetAddressOf());
    list->SetComputeRootSignature(root.Get());
    list->SetComputeRootDescriptorTable(0, handle(0));
    list->SetComputeRootDescriptorTable(1, handle(64));
    list->SetComputeRootDescriptorTable(3, handle(80));
    list->Dispatch(1, 1, 1);
    transition(list.Get(), dst.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
    list->CopyBufferRegion(read.Get(), 0, dst.Get(), 0, bytes);
    submit_and_wait(dev.Get(), queue.Get(), list.Get());
    void* mapped{};
    D3D12_RANGE full{0, bytes}, empty{};
    check(read->Map(0, &full, &mapped), "map readback");
    auto values = static_cast<const UINT*>(mapped);
    auto expected = initial;
    UINT allocated = first;
    for (UINT i = 0; i < cells * 2; ++i) {
        // The orthographic x/y/z bounds of cell (2,0,2) are [3,5], [-1,1], and [5,9].
        // The small local sphere therefore enables just physical lane 34. The capture still covers all cells.
        if (small_light && i < cells && i != 14)
            continue;
        expected[heads + i] = allocated;
        expected[links + 2 * allocated] = 0;
        expected[links + 2 * allocated + 1] = initial[heads + i];
        ++allocated;
    }
    expected[0] = allocated;
    UINT matched = 0;
    for (UINT i = 0; i < words; ++i) {
        matched += expected[i] == values[i];
        if (expected[i] != values[i] && i < 25)
            std::fprintf(out, "mismatch_%u_%u=%08x/%08x\n", unsigned(small_light), i, values[i], expected[i]);
    }
    std::fprintf(out, "%s_counter=%u\n%s_matched_words=%u\n", small_light ? "small" : "broad", values[0],
                 small_light ? "small" : "broad", matched);
    read->Unmap(0, &empty);
    if (matched != words)
        throw std::runtime_error("light grid mismatch");
}
int main() {
    FILE* out{};
    if (fopen_s(&out, "readback.txt", "wb"))
        return 2;
    int status = 0;
    try {
        run(out, false);
        run(out, true);
    } catch (const std::exception& e) {
        std::fprintf(out, "error=%s\n", e.what());
        status = 1;
    }
    return fclose(out) ? 2 : status;
}
