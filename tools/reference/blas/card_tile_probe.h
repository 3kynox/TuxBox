// The title dispatches one tile queue for direct-light visibility and another
// for radiosity. Sample the packed mask and the atlas pixels on the host.
static void probe_card_tile(ID3D12Device5 *dev, ID3D12CommandQueue *queue, ID3D12Resource *scene, FILE *receipt,
                            UINT entry, bool active, bool lit = false, bool trace = false, bool occluded = false) {
    const bool radiosity = entry == 1064;
    const UINT count = radiosity ? 16 : 7, width = trace ? 8 : 4, side = 8;
    D3D12_DESCRIPTOR_RANGE ranges[]{{D3D12_DESCRIPTOR_RANGE_TYPE_SRV, count, 0, 1, 0},
                                    {D3D12_DESCRIPTOR_RANGE_TYPE_UAV, 4, 0, 1, 0}};
    D3D12_ROOT_PARAMETER params[7]{};
    for (UINT i = 0; i < 2; ++i) {
        params[i].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
        params[i].DescriptorTable = {1, ranges + i};
    }
    for (UINT i = 0; i < 4; ++i) {
        params[2 + i].ParameterType = D3D12_ROOT_PARAMETER_TYPE_CBV;
        params[2 + i].Descriptor = {i, 1};
    }
    params[6].ParameterType = D3D12_ROOT_PARAMETER_TYPE_UAV;
    params[6].Descriptor = {0, 2};
    D3D12_STATIC_SAMPLER_DESC samplers[2]{};
    for (UINT i = 0; i < 2; ++i) {
        samplers[i].Filter = D3D12_FILTER_MIN_MAG_MIP_POINT;
        samplers[i].AddressU = samplers[i].AddressV = samplers[i].AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
        samplers[i].MaxLOD = D3D12_FLOAT32_MAX;
        samplers[i].ShaderRegister = i;
        samplers[i].RegisterSpace = 1;
    }
    D3D12_ROOT_SIGNATURE_DESC rs{7, params, 2, samplers, D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ObservedRayPipeline observed(dev, rs, entry,
                                 radiosity ? L"LumenRadiosityHardwareRayTracingRGS_aafff1ba"
                                           : L"LumenSceneDirectLightingHardwareRayTracingRGS_516b84a8",
                                 trace ? "card-input-observer.dxil" : "occlusion-observer.dxil");
    ComPtr<ID3D12CommandAllocator> alloc;
    ComPtr<ID3D12GraphicsCommandList4> list;
    check(dev->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&alloc)), "card allocator");
    check(dev->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, alloc.Get(), nullptr, IID_PPV_ARGS(&list)),
          "card list");
    auto transition = [&](ID3D12Resource *resource, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to) {
        D3D12_RESOURCE_BARRIER barrier{};
        barrier.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to};
        list->ResourceBarrier(1, &barrier);
    };
    D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, count + 4,
                                  D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, 0};
    ComPtr<ID3D12DescriptorHeap> heap;
    check(dev->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&heap)), "card descriptors");
    UINT step = dev->GetDescriptorHandleIncrementSize(hd.Type);
    auto cpu = heap->GetCPUDescriptorHandleForHeapStart();
    std::array<ComPtr<ID3D12Resource>, 16> srvs, texture_uploads;
    for (UINT i = 0; i < count; ++i, cpu.ptr += step) {
        D3D12_SHADER_RESOURCE_VIEW_DESC d{};
        d.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
        if (i == (radiosity ? 15u : 5u)) {
            d.ViewDimension = D3D12_SRV_DIMENSION_RAYTRACING_ACCELERATION_STRUCTURE;
            d.RaytracingAccelerationStructure.Location = scene->GetGPUVirtualAddress();
        } else if (radiosity ? (i >= 5 && i <= 12 && i != 9) : (i == 2 || i == 3)) {
            d.Format = DXGI_FORMAT_R32G32B32A32_FLOAT;
            d.ViewDimension = radiosity && i == 8 ? D3D12_SRV_DIMENSION_TEXTURECUBE : D3D12_SRV_DIMENSION_TEXTURE2D;
            d.Texture2D.MipLevels = 1;
            if (lit && i >= 5 && i <= 8) {
                const UINT faces = i == 8 ? 6u : 1u;
                D3D12_RESOURCE_DESC tex{};
                tex.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
                tex.Width = tex.Height = tex.MipLevels = tex.SampleDesc.Count = 1;
                tex.DepthOrArraySize = static_cast<UINT16>(faces);
                tex.Format = d.Format;
                D3D12_HEAP_PROPERTIES local{};
                local.Type = D3D12_HEAP_TYPE_DEFAULT;
                check(dev->CreateCommittedResource(&local, D3D12_HEAP_FLAG_NONE, &tex, D3D12_RESOURCE_STATE_COPY_DEST,
                                                   nullptr, IID_PPV_ARGS(&srvs[i])),
                      "card surface");
                std::array<BYTE, 6 * 512> rows{};
                const std::array<float, 4> sample = i == 8   ? std::array<float, 4>{1, 2, 3, 1}
                                                    : i == 6 ? std::array<float, 4>{0.5f, 0.5f, 0, 1}
                                                             : std::array<float, 4>{1, 1, 1, 1};
                for (UINT face = 0; face < faces; ++face)
                    std::memcpy(rows.data() + face * 512, sample.data(), sizeof(sample));
                texture_uploads[i] = upload(dev, rows.data(), rows.size());
                for (UINT face = 0; face < faces; ++face) {
                    D3D12_TEXTURE_COPY_LOCATION from{}, to{};
                    from.pResource = texture_uploads[i].Get();
                    from.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
                    from.PlacedFootprint = {face * 512ull, {tex.Format, 1, 1, 1, 256}};
                    to.pResource = srvs[i].Get();
                    to.SubresourceIndex = face;
                    list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
                }
                transition(srvs[i].Get(), D3D12_RESOURCE_STATE_COPY_DEST,
                           D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
            }
        } else {
            std::array<UINT, 32> words{};
            d.ViewDimension = D3D12_SRV_DIMENSION_BUFFER;
            if ((lit || trace) && i == 0) {
                // The three float4 card basis rows also carry their world-space origin in W.
                const float basis[12]{1, 0, 0, trace ? -1.f : 20.f, 0, 1, 0, trace ? 0.f : 20.f,
                                      0, 0, 1, trace ? -1.f : 20.f};
                std::memcpy(words.data(), basis, sizeof(basis));
            }
            if (radiosity && i == 1) {
                const float pageSize[2]{8.0f, 8.0f};
                std::memcpy(words.data() + 2, pageSize, sizeof(pageSize));
            }
            const bool tile = i == (radiosity ? 14u : 6u) || i == (radiosity ? 13u : 4u);
            if (i == (radiosity ? 13u : 4u))
                words[0] = active ? 1 : 0;
            d.Buffer.StructureByteStride = tile ? 4 : 16;
            if (radiosity && (i == 3 || i == 4)) {
                d.Buffer.StructureByteStride = 0;
                d.Buffer.Flags = D3D12_BUFFER_SRV_FLAG_RAW;
                d.Format = DXGI_FORMAT_R32_TYPELESS;
            }
            d.Buffer.NumElements = sizeof(words) / (d.Buffer.StructureByteStride ? d.Buffer.StructureByteStride : 4u);
            srvs[i] = upload(dev, words.data(), sizeof(words));
        }
        dev->CreateShaderResourceView(srvs[i].Get(), &d, cpu);
    }
    std::array<ComPtr<ID3D12Resource>, 4> uavs, seeds;
    std::array<UINT, 32> initial{};
    initial[0] = 2; // pre-resolve pixel 0 of the direct-light tile
    initial[4] = 0xdeadbeef;
    constexpr UINT pitch = D3D12_TEXTURE_DATA_PITCH_ALIGNMENT;
    std::array<UINT, side * pitch / 4> pixels;
    pixels.fill(0xdeadbeef);
    auto location = [&](ID3D12Resource *res) {
        D3D12_TEXTURE_COPY_LOCATION loc{};
        loc.pResource = res;
        loc.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        loc.PlacedFootprint = {0, {DXGI_FORMAT_R32G32B32A32_FLOAT, side, side, 1, pitch}};
        return loc;
    };
    for (UINT i = 0; i < 4; ++i, cpu.ptr += step) {
        D3D12_UNORDERED_ACCESS_VIEW_DESC d{};
        if (i != 3) {
            d.ViewDimension = D3D12_UAV_DIMENSION_BUFFER;
            d.Buffer.StructureByteStride = i == 2 ? 8 : 4;
            d.Buffer.NumElements = sizeof(initial) / d.Buffer.StructureByteStride;
            uavs[i] = buffer(dev, sizeof(initial), D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST, true);
            seeds[i] = upload(dev, initial.data(), sizeof(initial));
            list->CopyBufferRegion(uavs[i].Get(), 0, seeds[i].Get(), 0, sizeof(initial));
        } else {
            D3D12_HEAP_PROPERTIES local{};
            local.Type = D3D12_HEAP_TYPE_DEFAULT;
            D3D12_RESOURCE_DESC tex{};
            tex.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
            tex.Width = tex.Height = side;
            tex.MipLevels = tex.DepthOrArraySize = tex.SampleDesc.Count = 1;
            tex.Format = DXGI_FORMAT_R32G32B32A32_FLOAT;
            tex.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
            check(dev->CreateCommittedResource(&local, D3D12_HEAP_FLAG_NONE, &tex, D3D12_RESOURCE_STATE_COPY_DEST,
                                               nullptr, IID_PPV_ARGS(&uavs[i])),
                  "card atlas");
            seeds[i] = upload(dev, pixels.data(), sizeof(pixels));
            D3D12_TEXTURE_COPY_LOCATION dest{};
            dest.pResource = uavs[i].Get();
            auto src = location(seeds[i].Get());
            list->CopyTextureRegion(&dest, 0, 0, 0, &src, nullptr);
            d.ViewDimension = D3D12_UAV_DIMENSION_TEXTURE2D;
            d.Format = tex.Format;
        }
        transition(uavs[i].Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        dev->CreateUnorderedAccessView(uavs[i].Get(), nullptr, &d, cpu);
    }
    std::array<UINT, 160> callbacks{};
    auto calls = buffer(dev, sizeof(callbacks), D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST, true);
    auto zero = upload(dev, callbacks.data(), sizeof(callbacks));
    list->CopyBufferRegion(calls.Get(), 0, zero.Get(), 0, sizeof(callbacks));
    transition(calls.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    std::array<std::array<UINT, 1024>, 4> cb{};
    auto f = [&](UINT c, UINT word, float value) { std::memcpy(&cb[c][word], &value, 4); };
    if (radiosity) {
        f(0, 28 * 4 + 2, 1.f);
        f(0, 37 * 4 + 1, .25f);
        f(1, 190 * 4, 1.f);
        f(1, 190 * 4 + 1, 1.f);
        f(1, 190 * 4 + 2, 1.f);
        f(3, 1, 1.f);
        cb[0][36 * 4 + 2] = 1;
        cb[0][38 * 4] = width;
        f(0, 38 * 4 + 2, 10.f);
        f(0, 39 * 4, 10.f);
    } else {
        cb[0][31 * 4] = 1;
        f(0, 31 * 4 + 1, 8.f);
        f(0, 31 * 4 + 2, 8.f);
        f(2, 5 * 4 + 2, 1.f);
    }
    std::array<ComPtr<ID3D12Resource>, 4> constants;
    ID3D12DescriptorHeap *heaps[]{heap.Get()};
    list->SetDescriptorHeaps(1, heaps);
    list->SetComputeRootSignature(observed.root.Get());
    auto gpu = heap->GetGPUDescriptorHandleForHeapStart();
    list->SetComputeRootDescriptorTable(0, gpu);
    gpu.ptr += UINT64(count) * step;
    list->SetComputeRootDescriptorTable(1, gpu);
    for (UINT i = 0; i < 4; ++i) {
        constants[i] = upload(dev, cb[i].data(), sizeof(cb[i]));
        list->SetComputeRootConstantBufferView(2 + i, constants[i]->GetGPUVirtualAddress());
    }
    list->SetComputeRootUnorderedAccessView(6, calls->GetGPUVirtualAddress());
    list->SetPipelineState1(observed.pipeline.Get());
    D3D12_DISPATCH_RAYS_DESC dispatch{};
    UINT64 base = observed.sbt->GetGPUVirtualAddress();
    dispatch.RayGenerationShaderRecord = {base, 32};
    dispatch.MissShaderTable = {base + 64, 32, 32};
    dispatch.HitGroupTable = {base + 128, 128, 32};
    dispatch.Width = width;
    dispatch.Height = dispatch.Depth = 1;
    list->DispatchRays(&dispatch);
    constexpr UINT atlas_offset = 1024;
    auto rb = buffer(dev, atlas_offset + sizeof(pixels), D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
    transition(uavs[0].Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
    transition(uavs[3].Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
    transition(calls.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
    list->CopyBufferRegion(rb.Get(), 0, uavs[0].Get(), 0, sizeof(initial));
    list->CopyBufferRegion(rb.Get(), sizeof(initial), calls.Get(), 0, sizeof(callbacks));
    auto dst = location(rb.Get());
    dst.PlacedFootprint.Offset = atlas_offset;
    D3D12_TEXTURE_COPY_LOCATION src{};
    src.pResource = uavs[3].Get();
    list->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
    submit(dev, queue, list.Get());
    void *data{};
    D3D12_RANGE read{0, atlas_offset + sizeof(pixels)}, none{};
    check(rb->Map(0, &read, &data), "card readback");
    auto *words = static_cast<const UINT *>(data);
    UINT mask = words[0], changed = 0, misses = 0, hits = 0;
    bool clean = words[4] == 0xdeadbeef;
    bool raysMatch = true;
    std::array<UINT, 4> pixel{};
    for (UINT i = 0; i < width; ++i) {
        hits += words[initial.size() + i * 4];
        misses += words[initial.size() + i * 4 + 1];
        if (trace && i > 0 && occluded) {
            const auto *origin = words + initial.size() + width * 12 + i * 4;
            const auto *direction = words + initial.size() + width * 16 + i * 4;
            const std::array<UINT, 4> expectedOrigin{0xbf800000, 0, 0xbf800000, 0};
            const std::array<UINT, 4> expectedDirection{0, 0, 0x3f800000, 0x3f800000};
            raysMatch &= std::memcmp(origin, expectedOrigin.data(), 16) == 0
                         && std::memcmp(direction, expectedDirection.data(), 16) == 0;
        }
    }
    for (UINT y = 0; y < side; ++y)
        for (UINT x = 0; x < side; ++x) {
            auto pix = words + (atlas_offset + y * pitch) / 4 + x * 4;
            if (radiosity && active)
                clean &= (pix[0] != 0xdeadbeef) == (y < 2);
            if (pix[0] != 0xdeadbeef) {
                if (!changed)
                    std::memcpy(pixel.data(), pix, 16);
                ++changed;
                clean &= std::memcmp(pixel.data(), pix, 16) == 0;
            }
        }
    rb->Unmap(0, &none);
    const char *name = trace ? (occluded ? "occluded" : "unoccluded") : lit ? "lit" : active ? "active" : "empty";
    std::fprintf(receipt, "card_%u_%s=mask %08x; atlas %u; hits %u; misses %u; guard %u; color %08x,%08x,%08x,%08x\n",
                 entry, name, mask, changed, hits, misses, clean, pixel[0], pixel[1], pixel[2], pixel[3]);
    const bool traceOk = !trace || (raysMatch && !changed &&
                         (occluded ? (mask == 0xfffeu && hits == 7u && misses == 7u)
                                   : (mask == 0xaaaau && !hits && misses == 14u)));
    const bool directOk = trace || !active || radiosity || (mask == 0xaau && !changed && !hits && misses == 6u);
    const bool noSurfaceOk =
        !radiosity || lit || !active || (changed == 16 && !hits && !misses && pixel == std::array<UINT, 4>{});
    const bool litOk = !lit || (changed == 16 && !hits && misses == 16 &&
                                pixel == std::array<UINT, 4>{0x3f800000, 0x40000000, 0x40400000, 0x3f800000});
    if (!clean || !traceOk || !noSurfaceOk || !directOk || !litOk
        || (!active && (mask != 2 || changed || hits || misses)))
        throw std::runtime_error("card tile control mismatch");
}
