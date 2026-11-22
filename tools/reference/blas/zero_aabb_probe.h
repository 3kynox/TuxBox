// Procedural candidates are only a proposed broad phase for a future shadow
// scene. This checks API behavior at zero, not triangle correctness.
static void probe_zero_aabb(ID3D12Device5 *dev, ID3D12CommandQueue *queue, FILE *receipt) {
    const D3D12_RAYTRACING_AABB boxes[]{{-.25f, -.25f, -1, 1.25f, 1.25f, 1},
                                        {1.75f, -.25f, -1, 3.25f, 1.25f, 1}};
    auto aabb = upload(dev, boxes, sizeof(boxes));
    std::array<std::array<UINT, 32>, 2> node_words{};
    const float vertices[][12]{{0,0,0, 1,0,0, 0,1,0, 1,1,0},
                               {2,0,0, 3,0,0, 2,1,0, 3,1,0}};
    for (UINT i=0; i<2; ++i) std::memcpy(node_words[i].data(), vertices[i], sizeof(vertices[i]));
    auto nodes_upload = upload(dev, node_words.data(), sizeof(node_words));
    auto nodes = buffer(dev, sizeof(node_words), D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_RAYTRACING_GEOMETRY_DESC geometry{};
    geometry.Type = D3D12_RAYTRACING_GEOMETRY_TYPE_PROCEDURAL_PRIMITIVE_AABBS;
    geometry.AABBs.AABBCount = 2;
    geometry.AABBs.AABBs = {aabb->GetGPUVirtualAddress(), sizeof(boxes[0])};
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
    std::array<D3D12_RAYTRACING_INSTANCE_DESC, 3> instances{};
    for (UINT i=0; i<3; ++i) {
        for (UINT axis=0; axis<3; ++axis) instances[i].Transform[axis][axis] = 1;
        instances[i].InstanceMask = 0xff;
        instances[i].InstanceID = 7+i;
        instances[i].AccelerationStructure = blas->GetGPUVirtualAddress();
    }
    instances[1].Transform[0][0] = 2;
    instances[1].Transform[0][3] = 4;
    instances[2].Transform[0][3] = 12;
    instances[2].InstanceMask = 0;
    auto instance_buffer = upload(dev, instances.data(), sizeof(instances));
    Inputs top{};
    top.Type = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_TOP_LEVEL;
    top.DescsLayout = D3D12_ELEMENTS_LAYOUT_ARRAY;
    top.NumDescs = static_cast<UINT>(instances.size());
    top.InstanceDescs = instance_buffer->GetGPUVirtualAddress();
    Sizes ts{};
    dev->GetRaytracingAccelerationStructurePrebuildInfo(&top, &ts);
    auto tlas = buffer(dev, ts.ResultDataMaxSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                       D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE, true);
    auto tscratch = buffer(dev, ts.ScratchDataSizeInBytes, D3D12_HEAP_TYPE_DEFAULT,
                           D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
    D3D12_ROOT_PARAMETER params[4]{};
    params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_SRV;
    params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_UAV;
    params[2].ParameterType = D3D12_ROOT_PARAMETER_TYPE_SRV;
    params[2].Descriptor.ShaderRegister = 1;
    params[3].ParameterType = D3D12_ROOT_PARAMETER_TYPE_SRV;
    params[3].Descriptor.ShaderRegister = 2;
    D3D12_ROOT_SIGNATURE_DESC root_desc{4, params, 0, nullptr, D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> blob;
    check(D3D12SerializeRootSignature(&root_desc, D3D_ROOT_SIGNATURE_VERSION_1, &blob, nullptr), "zero AABB root serialize");
    ComPtr<ID3D12RootSignature> root;
    check(dev->CreateRootSignature(0, blob->GetBufferPointer(), blob->GetBufferSize(), IID_PPV_ARGS(&root)), "zero AABB root");
    D3D12_COMPUTE_PIPELINE_STATE_DESC desc{};
    desc.pRootSignature = root.Get();
    desc.CS = {zero_aabb_dxil, sizeof(zero_aabb_dxil)};
    ComPtr<ID3D12PipelineState> pipeline;
    check(dev->CreateComputePipelineState(&desc, IID_PPV_ARGS(&pipeline)), "zero AABB pipeline");
    ComPtr<ID3D12CommandAllocator> alloc;
    ComPtr<ID3D12GraphicsCommandList4> list;
    check(dev->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&alloc)), "zero AABB allocator");
    check(dev->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, alloc.Get(), pipeline.Get(), IID_PPV_ARGS(&list)), "zero AABB list");
    list->CopyBufferRegion(nodes.Get(), 0, nodes_upload.Get(), 0, sizeof(node_words));
    D3D12_RESOURCE_BARRIER nodes_ready{};
    nodes_ready.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    nodes_ready.Transition = {nodes.Get(), 0, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
    list->ResourceBarrier(1, &nodes_ready);
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
    const float origins[][4]{{.25f,.25f,0,0}, {2.25f,.25f,0,0}, {4.5f,.25f,0,0},
                              {0,.5f,0,0}, {.5f,0,0,0}, {.8f,.8f,0,0}, {8.5f,.25f,0,0},
                              {20,0,0,0}, {.25f,.25f,-1,0}, {12.25f,.25f,0,0}};
    constexpr UINT result_bytes = sizeof(origins);
    auto source = upload(dev, origins, sizeof(origins));
    auto output = buffer(dev, result_bytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
    auto readback = buffer(dev, result_bytes, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
    list->SetComputeRootSignature(root.Get());
    list->SetComputeRootShaderResourceView(0, tlas->GetGPUVirtualAddress());
    list->SetComputeRootUnorderedAccessView(1, output->GetGPUVirtualAddress());
    list->SetComputeRootShaderResourceView(2, source->GetGPUVirtualAddress());
    list->SetComputeRootShaderResourceView(3, nodes->GetGPUVirtualAddress());
    list->Dispatch(static_cast<UINT>(std::size(origins)), 1, 1);
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition = {output.Get(), 0, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE};
    list->ResourceBarrier(1, &barrier);
    list->CopyBufferRegion(readback.Get(), 0, output.Get(), 0, result_bytes);
    submit(dev, queue, list.Get());
    D3D12_RANGE read{0,result_bytes}, empty{};
    void *data{};
    check(readback->Map(0, &read, &data), "zero AABB readback");
    std::array<std::array<UINT,4>,std::size(origins)> values{};
    std::memcpy(values.data(), data, result_bytes);
    readback->Unmap(0, &empty);
    for (UINT i=0; i<std::size(origins); ++i)
        std::fprintf(receipt, "zero_aabb_%u=%u,%u,%08x,%08x\n", i, values[i][0], values[i][1], values[i][2], values[i][3]);
    if (values[0] != std::array<UINT,4>{1,2,0,0x70000} || values[1] != std::array<UINT,4>{1,2,0,0x70001} ||
        values[2] != std::array<UINT,4>{1,2,0,0x80000} || values[3][0] != 1 || values[3][1] != 2 ||
        (values[3][2] & 0x7fffffffu) != 0 || values[3][3] != 0x70000 ||
        values[4] != std::array<UINT,4>{1,0,0,0} || values[5] != std::array<UINT,4>{1,0,0,0} ||
        values[6] != std::array<UINT,4>{1,2,0,0x80001} || values[7] != std::array<UINT,4>{0,0,0,0} ||
        values[8] != std::array<UINT,4>{1,2,0x3f800000,0x70000} ||
        values[9] != std::array<UINT,4>{0,0,0,0})
        throw std::runtime_error("zero AABB candidate mismatch");
}
