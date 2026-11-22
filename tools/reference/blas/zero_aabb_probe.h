// Procedural candidates are only a proposed broad phase for a future shadow
// scene. This checks API behavior at zero, not triangle correctness.
static void probe_zero_aabb(ID3D12Device5 *dev, ID3D12CommandQueue *queue, FILE *receipt) {
    const D3D12_RAYTRACING_AABB boxes[]{{-.25f, -.25f, -1, 1.25f, 1.25f, 1},
                                        {1.75f, -.25f, -1, 3.25f, 1.25f, 1}};
    auto aabb = upload(dev, boxes, sizeof(boxes));
    const auto node_words = read_file("sidecar-leaves.bin");
    const auto directory_words = read_file("sidecar-directory.bin");
    if (node_words.size() != 128 || directory_words.size() != 96)
        throw std::runtime_error("unexpected sidecar fixture layout");
    auto nodes_upload = upload(dev, node_words.data(), node_words.size());
    auto nodes = buffer(dev, node_words.size(), D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST);
    auto directory_upload = upload(dev, directory_words.data(), directory_words.size());
    auto directory = buffer(dev, directory_words.size(), D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST);
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
    list->CopyBufferRegion(nodes.Get(), 0, nodes_upload.Get(), 0, node_words.size());
    list->CopyBufferRegion(directory.Get(), 0, directory_upload.Get(), 0, directory_words.size());
    D3D12_RESOURCE_BARRIER nodes_ready{};
    nodes_ready.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    nodes_ready.Transition = {nodes.Get(), 0, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
    list->ResourceBarrier(1, &nodes_ready);
    nodes_ready.Transition.pResource = directory.Get();
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
    D3D12_ROOT_PARAMETER private_params[7]{};
    for (UINT i : {0u,1u,2u,5u}) {
        private_params[i].ParameterType = D3D12_ROOT_PARAMETER_TYPE_SRV;
        private_params[i].Descriptor.RegisterSpace = 127;
    }
    private_params[1].Descriptor.ShaderRegister = 1;
    private_params[2].Descriptor.ShaderRegister = 2;
    private_params[5].Descriptor.ShaderRegister = 3;
    for (UINT i : {3u,6u}) {
        private_params[i].ParameterType = D3D12_ROOT_PARAMETER_TYPE_UAV;
        private_params[i].Descriptor.RegisterSpace = 127;
    }
    private_params[6].Descriptor.ShaderRegister = 1;
    private_params[4].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    private_params[4].Constants = {0, 127, 2};
    D3D12_ROOT_SIGNATURE_DESC private_desc{7, private_params, 0, nullptr, D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> private_blob;
    check(D3D12SerializeRootSignature(&private_desc, D3D_ROOT_SIGNATURE_VERSION_1, &private_blob, nullptr), "inline candidate root serialize");
    ComPtr<ID3D12RootSignature> private_root;
    check(dev->CreateRootSignature(0, private_blob->GetBufferPointer(), private_blob->GetBufferSize(), IID_PPV_ARGS(&private_root)), "inline candidate root");
    D3D12_COMPUTE_PIPELINE_STATE_DESC private_pso{};
    private_pso.pRootSignature = private_root.Get();
    private_pso.CS = {inline219_dxil, sizeof(inline219_dxil)};
    ComPtr<ID3D12PipelineState> candidate_pipeline;
    check(dev->CreateComputePipelineState(&private_pso, IID_PPV_ARGS(&candidate_pipeline)), "inline candidate pipeline");
    auto inline_output = buffer(dev, result_bytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
    auto inline_readback = buffer(dev, result_bytes + 4, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
    const UINT zero = 0;
    auto zero_upload = upload(dev, &zero, sizeof(zero));
    auto status = buffer(dev, 4, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST, true);
    list->CopyBufferRegion(status.Get(), 0, zero_upload.Get(), 0, 4);
    D3D12_RESOURCE_BARRIER status_ready{};
    status_ready.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    status_ready.Transition = {status.Get(), 0, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_UNORDERED_ACCESS};
    list->ResourceBarrier(1, &status_ready);
    list->SetPipelineState(candidate_pipeline.Get());
    list->SetComputeRootSignature(private_root.Get());
    list->SetComputeRootShaderResourceView(0, tlas->GetGPUVirtualAddress());
    list->SetComputeRootShaderResourceView(1, nodes->GetGPUVirtualAddress());
    list->SetComputeRootShaderResourceView(2, directory->GetGPUVirtualAddress());
    list->SetComputeRootUnorderedAccessView(3, status->GetGPUVirtualAddress());
    const UINT extents[]{static_cast<UINT>(directory_words.size()), static_cast<UINT>(node_words.size())};
    list->SetComputeRoot32BitConstants(4, 2, extents, 0);
    list->SetComputeRootShaderResourceView(5, source->GetGPUVirtualAddress());
    list->SetComputeRootUnorderedAccessView(6, inline_output->GetGPUVirtualAddress());
    list->Dispatch(static_cast<UINT>(std::size(origins)), 1, 1);
    auto bad_output = buffer(dev, result_bytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, true);
    auto bad_readback = buffer(dev, result_bytes + 4, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
    auto bad_status = buffer(dev, 4, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST, true);
    list->CopyBufferRegion(bad_status.Get(), 0, zero_upload.Get(), 0, 4);
    status_ready.Transition.pResource = bad_status.Get();
    list->ResourceBarrier(1, &status_ready);
    list->SetComputeRootUnorderedAccessView(3, bad_status->GetGPUVirtualAddress());
    const UINT bad_extents[]{15, static_cast<UINT>(node_words.size())};
    list->SetComputeRoot32BitConstants(4, 2, bad_extents, 0);
    list->SetComputeRootUnorderedAccessView(6, bad_output->GetGPUVirtualAddress());
    list->Dispatch(static_cast<UINT>(std::size(origins)), 1, 1);
    D3D12_RESOURCE_BARRIER bad_barriers[2]{};
    for (auto &item : bad_barriers) item.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    bad_barriers[0].Transition = {bad_output.Get(), 0, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE};
    bad_barriers[1].Transition = {bad_status.Get(), 0, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE};
    list->ResourceBarrier(2, bad_barriers);
    list->CopyBufferRegion(bad_readback.Get(), 0, bad_output.Get(), 0, result_bytes);
    list->CopyBufferRegion(bad_readback.Get(), result_bytes, bad_status.Get(), 0, 4);
    D3D12_RESOURCE_BARRIER inline_barriers[2]{};
    for (auto &item : inline_barriers) item.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    inline_barriers[0].Transition = {inline_output.Get(), 0, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE};
    inline_barriers[1].Transition = {status.Get(), 0, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE};
    list->ResourceBarrier(2, inline_barriers);
    list->CopyBufferRegion(inline_readback.Get(), 0, inline_output.Get(), 0, result_bytes);
    list->CopyBufferRegion(inline_readback.Get(), result_bytes, status.Get(), 0, 4);
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
    D3D12_RANGE inline_range{0, result_bytes + 4};
    check(inline_readback->Map(0, &inline_range, &data), "inline candidate readback");
    std::array<std::array<UINT,4>,std::size(origins)> inline_values{};
    std::memcpy(inline_values.data(), data, result_bytes);
    UINT inline_status{};
    std::memcpy(&inline_status, static_cast<const BYTE *>(data) + result_bytes, 4);
    inline_readback->Unmap(0, &empty);
    std::fprintf(receipt, "inline219_candidate_status=%u\n", inline_status);
    for (UINT i=0; i<std::size(origins); ++i)
        std::fprintf(receipt, "inline219_candidate_%u=%u,%u,%08x,%08x\n", i, inline_values[i][0], inline_values[i][1], inline_values[i][2], inline_values[i][3]);
    bool inline_matches = true;
    for (UINT i=0; i<std::size(origins); ++i) {
        inline_matches &= inline_values[i][0] == values[i][0] && inline_values[i][1] == values[i][1] &&
                          inline_values[i][3] == values[i][3];
        const UINT a = inline_values[i][2], b = values[i][2];
        inline_matches &= a == b || ((a & 0x7fffffffu) == 0 && (b & 0x7fffffffu) == 0);
        std::fprintf(receipt, "inline219_reference_%u=%u,%u,%08x,%08x\n", i, values[i][0], values[i][1], values[i][2], values[i][3]);
    }
    if (inline_status != 4 || !inline_matches)
        throw std::runtime_error("inline 219 guarded candidate mismatch");
    check(bad_readback->Map(0, &inline_range, &data), "invalid inline directory readback");
    std::array<std::array<UINT,4>,std::size(origins)> bad_values{};
    std::memcpy(bad_values.data(), data, result_bytes);
    UINT bad_bits{};
    std::memcpy(&bad_bits, static_cast<const BYTE *>(data) + result_bytes, 4);
    bad_readback->Unmap(0, &empty);
    std::fprintf(receipt, "inline219_invalid_directory_status=%u\n", bad_bits);
    if (bad_bits != 2 || std::any_of(bad_values.begin(), bad_values.end(),
        [](const auto &value) { return value[1] != 0 || value[2] != 0 || value[3] != 0; }))
        throw std::runtime_error("inline 219 invalid directory accepted");
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
