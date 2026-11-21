#include "frame_abi.h"
#include "query_dxil.h"
#include <cstring>

using frame_abi::check;
using frame_abi::ComPtr;
using frame_abi::method;
using Inputs = D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS;
using Sizes = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO;
using Build = D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC;

static ComPtr<ID3D12Resource> buffer(ID3D12Device* dev, UINT64 bytes, D3D12_HEAP_TYPE heap,
                                     D3D12_RESOURCE_STATES state, bool uav = false) {
    D3D12_HEAP_PROPERTIES props{};
    props.Type = heap;
    props.CreationNodeMask = props.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width = bytes;
    desc.Height = desc.DepthOrArraySize = desc.MipLevels = desc.SampleDesc.Count = 1;
    desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if (uav) desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    ComPtr<ID3D12Resource> res;
    check(dev->CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &desc, state,
                                        nullptr, IID_PPV_ARGS(&res)), "create buffer");
    return res;
}

static ComPtr<ID3D12Resource> upload(ID3D12Device* dev, const void* data, size_t bytes) {
    auto res = buffer(dev, bytes, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
    D3D12_RANGE empty{}, written{0, bytes};
    void* dest{};
    check(res->Map(0, &empty, &dest), "map upload");
    std::memcpy(dest, data, bytes);
    res->Unmap(0, &written);
    return res;
}

static ComPtr<ID3D12RootSignature> root_signature(ID3D12Device* device) {
    D3D12_ROOT_PARAMETER params[2]{};
    params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_SRV;
    params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_UAV;
    D3D12_ROOT_SIGNATURE_DESC desc{2, params, 0, nullptr, D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> blob;
    check(D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &blob, nullptr), "serialize root");
    ComPtr<ID3D12RootSignature> root;
    check(device->CreateRootSignature(0, blob->GetBufferPointer(), blob->GetBufferSize(),
                                       IID_PPV_ARGS(&root)), "create root");
    return root;
}

static ComPtr<ID3D12Device> provider_device(bool xbox) {
    ComPtr<ID3D12Device> device;
    if (xbox) {
        const std::array<UINT, 18> profile{0x20004, 0, ~0u, 0, 0, 0, ~0u, ~0u, 0, 1};
        HMODULE module = LoadLibraryExW(L"d3d12_xs.dll", nullptr,
                                        LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
        using Factory = HRESULT(WINAPI*)(IUnknown*, const void*, REFIID, void**);
        auto factory = module ? reinterpret_cast<Factory>(GetProcAddress(module, "D3D12XboxCreateDevice")) : nullptr;
        if (!factory) throw std::runtime_error("missing Xbox device factory");
        check(factory(nullptr, profile.data(), IID_PPV_ARGS(&device)), "create Xbox device");
    } else {
        check(D3D12CreateDevice(nullptr, D3D_FEATURE_LEVEL_12_0, IID_PPV_ARGS(&device)), "create desktop device");
    }
    return device;
}

static void submit(ID3D12Device* owner, ID3D12CommandQueue* queue, ID3D12GraphicsCommandList* list) {
    check(list->Close(), "close list");
    ID3D12CommandList* lists[] = {list};
    ComPtr<ID3D12Fence> fence;
    check(owner->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)), "create fence");
    queue->ExecuteCommandLists(1, lists);
    check(queue->Signal(fence.Get(), 1), "signal fence");
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) throw std::runtime_error("create event");
    HRESULT hr = fence->SetEventOnCompletion(1, event);
    DWORD wait = SUCCEEDED(hr) ? WaitForSingleObject(event, 15000) : WAIT_FAILED;
    CloseHandle(event);
    check(hr, "fence event");
    if (wait != WAIT_OBJECT_0 || fence->GetCompletedValue() != 1)
        throw std::runtime_error("GPU fence did not complete");
}

// Only the guest prebuild and BLAS build go through Xbox ABI slots. The same desktop
// device creates the inputs, builds the verification TLAS and runs the ray queries.
static void run(bool xbox, bool guest_build, FILE* receipt) {
    const auto provider = provider_device(xbox);
    const auto root = root_signature(provider.Get());
    ComPtr<ID3D12Device5> owner;
    check(root->GetDevice(IID_PPV_ARGS(&owner)), "root owning device");
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGIAdapter1> adapter;
    DXGI_ADAPTER_DESC1 gpu{};
    check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)), "DXGI factory");
    check(factory->EnumAdapterByLuid(owner->GetAdapterLuid(), IID_PPV_ARGS(&adapter)), "DXGI adapter");
    check(adapter->GetDesc1(&gpu), "GPU description");
    std::fprintf(receipt, "adapter=%04x:%04x\n", gpu.VendorId, gpu.DeviceId);
    D3D12_FEATURE_DATA_D3D12_OPTIONS5 options{};
    check(owner->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS5, &options, sizeof(options)), "options5");
    std::fprintf(receipt, "raytracing_tier=%u\n", static_cast<unsigned>(options.RaytracingTier));
    if (options.RaytracingTier < D3D12_RAYTRACING_TIER_1_1) throw std::runtime_error("requires DXR 1.1");

    const float vertices[][3] = {{-1.5f,-0.5f,0},{-0.5f,-0.5f,0},{-1,0.5f,0},
                                 {0.5f,-0.5f,1},{1.5f,-0.5f,1},{1,0.5f,1}};
    auto mesh = upload(owner.Get(), vertices, sizeof(vertices));
    static_assert(sizeof(D3D12_RAYTRACING_GEOMETRY_DESC) == 56);
    std::array<D3D12_RAYTRACING_GEOMETRY_DESC, 2> geometries{};
    for (UINT i = 0; i < geometries.size(); ++i) {
        auto& g = geometries[i];
        g.Type = D3D12_RAYTRACING_GEOMETRY_TYPE_TRIANGLES;
        g.Flags = D3D12_RAYTRACING_GEOMETRY_FLAG_OPAQUE;
        g.Triangles.VertexFormat = DXGI_FORMAT_R32G32B32_FLOAT;
        g.Triangles.VertexCount = 3;
        g.Triangles.VertexBuffer.StartAddress = mesh->GetGPUVirtualAddress() + i * 3 * sizeof(vertices[0]);
        g.Triangles.VertexBuffer.StrideInBytes = sizeof(vertices[0]);
    }
    Inputs bottom{};
    bottom.Type = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_BOTTOM_LEVEL;
    bottom.NumDescs = static_cast<UINT>(geometries.size());
    bottom.pGeometryDescs = geometries.data();
    bottom.Flags = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PREFER_FAST_TRACE;
    Sizes native{};
    owner->GetRaytracingAccelerationStructurePrebuildInfo(&bottom, &native);
    if (!native.ResultDataMaxSizeInBytes || !native.ScratchDataSizeInBytes) throw std::runtime_error("empty prebuild");
    Sizes sizes = native;
    if (xbox) {
        struct { UINT64 before; Sizes sizes; UINT64 after; } guarded{1, {}, 2};
        using Prebuild = void(WINAPI*)(ID3D12Device*, const Inputs*, Sizes*);
        method<Prebuild>(provider.Get(), 102)(provider.Get(), &bottom, &guarded.sizes);
        if (guarded.before != 1 || guarded.after != 2 || std::memcmp(&native, &guarded.sizes, sizeof(native)))
            throw std::runtime_error("Xbox prebuild output differs from desktop or modified guard");
        sizes = guarded.sizes;
    }
    std::fprintf(receipt, "blas_result_bytes=%llu\nblas_scratch_bytes=%llu\n",
                 sizes.ResultDataMaxSizeInBytes, sizes.ScratchDataSizeInBytes);
    std::fflush(receipt);
    std::array<ComPtr<ID3D12Resource>, 2> structures, scratch;
    // Frosty copies each 48-byte desktop description into an 80-byte batch entry.
    struct Entry {
        Build build;
        std::array<UINT64, 4> reserved;
    };
    static_assert(sizeof(Inputs) == 24 && sizeof(Sizes) == 24 && sizeof(Entry) == 80);
    std::array<Entry, 2> batch{};
    for (size_t i = 0; i < batch.size(); ++i) {
        structures[i] = buffer(owner.Get(), sizes.ResultDataMaxSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                               D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE, true);
        scratch[i] = buffer(owner.Get(), sizes.ScratchDataSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                            D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
        batch[i].build.Inputs = bottom;
        batch[i].build.DestAccelerationStructureData = structures[i]->GetGPUVirtualAddress();
        batch[i].build.ScratchAccelerationStructureData = scratch[i]->GetGPUVirtualAddress();
    }
    D3D12_COMMAND_QUEUE_DESC queue_desc{};
    ComPtr<ID3D12CommandQueue> queue;
    check(provider->CreateCommandQueue(&queue_desc, IID_PPV_ARGS(&queue)), "create queue");
    ComPtr<ID3D12CommandAllocator> allocator;
    check(owner->CreateCommandAllocator(queue_desc.Type, IID_PPV_ARGS(&allocator)), "create allocator");
    ComPtr<ID3D12GraphicsCommandList> list;
    if (guest_build) {
        const UINT params[]{0, 0, 1, 0};
        check(method<frame_abi::CreateList>(provider.Get(), 73)(provider.Get(), params, allocator.Get(), nullptr,
                                                                IID_PPV_ARGS(&list)), "Xbox list create");
    } else {
        check(owner->CreateCommandList(0, queue_desc.Type, allocator.Get(), nullptr, IID_PPV_ARGS(&list)),
              "create list");
    }
    if (guest_build) {
        const IID list6_iid{0xc3827890, 0x4024, 0x474c, {0x98, 0xcf, 0x64, 0x5b, 0x51, 0xdc, 0x48, 0x74}};
        ComPtr<IUnknown> list6;
        check(list->QueryInterface(list6_iid, reinterpret_cast<void**>(list6.GetAddressOf())), "Xbox List6 query");
        if (static_cast<void*>(list.Get()) != static_cast<void*>(list6.Get()))
            throw std::runtime_error("List6 did not preserve Xbox command list identity");
        using BatchBuild = void(WINAPI*)(IUnknown*, UINT, const Entry*);
        method<BatchBuild>(list6.Get(), 131)(list6.Get(), static_cast<UINT>(batch.size()), batch.data());
    } else {
        ComPtr<ID3D12GraphicsCommandList4> list4;
        check(list.As(&list4), "List4 query");
        for (const auto& entry : batch)
            list4->BuildRaytracingAccelerationStructure(&entry.build, 0, nullptr);
    }
    D3D12_RESOURCE_BARRIER ready{};
    ready.Type = D3D12_RESOURCE_BARRIER_TYPE_UAV;
    list->ResourceBarrier(1, &ready);
    submit(owner.Get(), queue.Get(), list.Get());

    std::array<D3D12_RAYTRACING_INSTANCE_DESC, 2> instances{};
    for (UINT i = 0; i < instances.size(); ++i) {
        auto& instance = instances[i];
        for (UINT axis = 0; axis < 3; ++axis) instance.Transform[axis][axis] = 1;
        instance.Transform[0][3] = static_cast<float>(i * 4);
        instance.InstanceMask = 0xff;
        instance.InstanceID = 7 + i;
        instance.AccelerationStructure = structures[i]->GetGPUVirtualAddress();
    }
    auto instance_buffer = upload(owner.Get(), instances.data(), sizeof(instances));
    Inputs top{};
    top.Type = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_TOP_LEVEL;
    top.NumDescs = static_cast<UINT>(instances.size());
    top.InstanceDescs = instance_buffer->GetGPUVirtualAddress();
    Sizes top_sizes{};
    owner->GetRaytracingAccelerationStructurePrebuildInfo(&top, &top_sizes);
    auto scene = buffer(owner.Get(), top_sizes.ResultDataMaxSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                        D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE, true);
    auto scene_scratch = buffer(owner.Get(), top_sizes.ScratchDataSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                                D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
    D3D12_COMPUTE_PIPELINE_STATE_DESC pso_desc{};
    pso_desc.pRootSignature = root.Get();
    pso_desc.CS = {query_dxil, sizeof(query_dxil)};
    ComPtr<ID3D12PipelineState> pso;
    check(owner->CreateComputePipelineState(&pso_desc, IID_PPV_ARGS(&pso)), "create rayquery pipeline");
    check(allocator->Reset(), "reset completed allocator");
    ComPtr<ID3D12GraphicsCommandList4> verify;
    check(owner->CreateCommandList(0, queue_desc.Type, allocator.Get(), pso.Get(), IID_PPV_ARGS(&verify)),
          "create verification list");
    Build tlas{};
    tlas.Inputs = top;
    tlas.DestAccelerationStructureData = scene->GetGPUVirtualAddress();
    tlas.ScratchAccelerationStructureData = scene_scratch->GetGPUVirtualAddress();
    verify->BuildRaytracingAccelerationStructure(&tlas, 0, nullptr);
    ready.UAV.pResource = scene.Get();
    verify->ResourceBarrier(1, &ready);
    // The hit words encode instance ID, geometry index and distance in sixteenths.
    constexpr UINT64 bytes = 5 * sizeof(UINT);
    auto output = buffer(owner.Get(), bytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
    auto readback = buffer(owner.Get(), bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
    verify->SetComputeRootSignature(root.Get());
    verify->SetComputeRootShaderResourceView(0, scene->GetGPUVirtualAddress());
    verify->SetComputeRootUnorderedAccessView(1, output->GetGPUVirtualAddress());
    verify->Dispatch(1, 1, 1);
    D3D12_RESOURCE_BARRIER copy{};
    copy.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    copy.Transition = {output.Get(), 0, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE};
    verify->ResourceBarrier(1, &copy);
    verify->CopyBufferRegion(readback.Get(), 0, output.Get(), 0, bytes);
    submit(owner.Get(), queue.Get(), verify.Get());
    D3D12_RANGE read{0, bytes}, empty{};
    void* data{};
    check(readback->Map(0, &read, &data), "map ray query results");
    std::array<UINT, 5> values{};
    std::memcpy(values.data(), data, bytes);
    readback->Unmap(0, &empty);
    std::fprintf(receipt, "ray_query_results=%x,%x,%x,%x,%x\n",
                 values[0], values[1], values[2], values[3], values[4]);
    if (values != std::array<UINT, 5>{0x70010, 0x70120, 0x80010, 0x80120, 0xdead})
        throw std::runtime_error("ray queries differ");
}

int wmain(int argc, wchar_t** argv) {
    if (argc != 3 || (wcscmp(argv[1], L"desktop") && wcscmp(argv[1], L"xbox") &&
                      wcscmp(argv[1], L"xbox-prebuild"))) return 2;
    FILE* receipt{};
    if (_wfopen_s(&receipt, argv[2], L"wb")) return 2;
    int status = 0;
    try {
        run(wcscmp(argv[1], L"desktop") != 0, wcscmp(argv[1], L"xbox") == 0, receipt);
    } catch (const std::exception& e) {
        std::fprintf(receipt, "error=%s\n", e.what());
        status = 1;
    }
    return fclose(receipt) ? 2 : status;
}
