// Procedural candidates are only a proposed broad phase for a future shadow
// scene. This checks API behavior at zero, not triangle correctness.
static void probe_zero_aabb(ID3D12Device5 *dev, ID3D12CommandQueue *queue, FILE *receipt) {
    const D3D12_RAYTRACING_AABB box{-1, -1, -1, 1, 1, 1};
    auto aabb = upload(dev, &box, sizeof(box));
    D3D12_RAYTRACING_GEOMETRY_DESC geometry{};
    geometry.Type = D3D12_RAYTRACING_GEOMETRY_TYPE_PROCEDURAL_PRIMITIVE_AABBS;
    geometry.AABBs.AABBCount = 1;
    geometry.AABBs.AABBs = {aabb->GetGPUVirtualAddress(), sizeof(box)};
    Inputs bottom{};
    bottom.Type = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_BOTTOM_LEVEL;
    bottom.DescsLayout = D3D12_ELEMENTS_LAYOUT_ARRAY;
    bottom.NumDescs = 1;
    bottom.pGeometryDescs = &geometry;
    Sizes bs{};
    dev->GetRaytracingAccelerationStructurePrebuildInfo(&bottom, &bs);
    auto blas = buffer(dev, bs.ResultDataMaxSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                       D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE, true);
    auto bscratch = buffer(dev, bs.ScratchDataSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                           D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
    D3D12_RAYTRACING_INSTANCE_DESC instance{};
    for (UINT i=0; i<3; ++i) instance.Transform[i][i] = 1;
    instance.InstanceMask = 0xff;
    instance.AccelerationStructure = blas->GetGPUVirtualAddress();
    auto instance_buffer = upload(dev, &instance, sizeof(instance));
    Inputs top{};
    top.Type = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_TOP_LEVEL;
    top.DescsLayout = D3D12_ELEMENTS_LAYOUT_ARRAY;
    top.NumDescs = 1;
    top.InstanceDescs = instance_buffer->GetGPUVirtualAddress();
    Sizes ts{};
    dev->GetRaytracingAccelerationStructurePrebuildInfo(&top, &ts);
    auto tlas = buffer(dev, ts.ResultDataMaxSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                       D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE, true);
    auto tscratch = buffer(dev, ts.ScratchDataSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                           D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
    const auto root = root_signature(dev);
    D3D12_COMPUTE_PIPELINE_STATE_DESC desc{};
    desc.pRootSignature = root.Get();
    desc.CS = {zero_aabb_dxil, sizeof(zero_aabb_dxil)};
    ComPtr<ID3D12PipelineState> pipeline;
    check(dev->CreateComputePipelineState(&desc, IID_PPV_ARGS(&pipeline)), "zero AABB pipeline");
    ComPtr<ID3D12CommandAllocator> alloc;
    ComPtr<ID3D12GraphicsCommandList4> list;
    check(dev->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&alloc)), "zero AABB allocator");
    check(dev->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, alloc.Get(), pipeline.Get(), IID_PPV_ARGS(&list)), "zero AABB list");
    Build build{};
    build.Inputs = bottom;
    build.DestAccelerationStructureData = blas->GetGPUVirtualAddress();
    build.ScratchAccelerationStructureData = bscratch->GetGPUVirtualAddress();
    list->BuildRaytracingAccelerationStructure(&build, 0, nullptr);
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_UAV;
    barrier.UAV.pResource = blas.Get();
    list->ResourceBarrier(1, &barrier);
    build.Inputs = top;
    build.DestAccelerationStructureData = tlas->GetGPUVirtualAddress();
    build.ScratchAccelerationStructureData = tscratch->GetGPUVirtualAddress();
    list->BuildRaytracingAccelerationStructure(&build, 0, nullptr);
    barrier.UAV.pResource = tlas.Get();
    list->ResourceBarrier(1, &barrier);
    const float origins[][4]{{.25f,.25f,0,0}, {.25f,.25f,-1,0}, {.8f,.8f,0,0}, {2,0,0,0}};
    auto source = upload(dev, origins, sizeof(origins));
    auto output = buffer(dev, 64, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
    auto readback = buffer(dev, 64, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
    list->SetComputeRootSignature(root.Get());
    list->SetComputeRootShaderResourceView(0, tlas->GetGPUVirtualAddress());
    list->SetComputeRootUnorderedAccessView(1, output->GetGPUVirtualAddress());
    list->SetComputeRootShaderResourceView(2, source->GetGPUVirtualAddress());
    list->Dispatch(4, 1, 1);
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition = {output.Get(), 0, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE};
    list->ResourceBarrier(1, &barrier);
    list->CopyBufferRegion(readback.Get(), 0, output.Get(), 0, 64);
    submit(dev, queue, list.Get());
    D3D12_RANGE read{0,64}, empty{};
    void *data{};
    check(readback->Map(0, &read, &data), "zero AABB readback");
    std::array<std::array<UINT,4>,4> values{};
    std::memcpy(values.data(), data, 64);
    readback->Unmap(0, &empty);
    for (UINT i=0; i<4; ++i)
        std::fprintf(receipt, "zero_aabb_%u=%u,%u,%08x\n", i, values[i][0], values[i][1], values[i][2]);
    if (values[0] != std::array<UINT,4>{1,2,0,0} || values[1] != std::array<UINT,4>{1,0,0,0} ||
        values[2] != std::array<UINT,4>{1,0,0,0} || values[3] != std::array<UINT,4>{0,0,0,0})
        throw std::runtime_error("zero AABB candidate mismatch");
}
