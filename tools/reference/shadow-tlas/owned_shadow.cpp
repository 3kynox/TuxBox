// GPU control for the isolated Rust-owned, immutable serialized BLAS snapshot.
#include "../independent-shaders/context.h"
#include "owned_shadow_dxil.h"
using namespace shader_control;
extern "C" int tb_shadow_create(ID3D12Device*, const unsigned char*, size_t, void**);
extern "C" int tb_shadow_info(const void*, UINT64*, UINT64*);
extern "C" int tb_shadow_hold(void*, ID3D12Fence*, UINT64);
extern "C" int tb_shadow_try_release(void*);

static void wait(ID3D12Fence* fence, UINT64 value) {
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) throw std::runtime_error("create event");
    HRESULT hr = fence->SetEventOnCompletion(value, event);
    DWORD status = SUCCEEDED(hr) ? WaitForSingleObject(event, 15000) : WAIT_FAILED;
    CloseHandle(event); check(hr, "completion event");
    if (status != WAIT_OBJECT_0 || fence->GetCompletedValue() < value) throw std::runtime_error("fence wait");
}
int main() {
    FILE* receipt{};
    if (fopen_s(&receipt, "result.txt", "wb")) return 2;
    int result = 0;
    try {
        Context ctx(receipt);
        std::ifstream input("source.bin", std::ios::binary);
        std::vector<unsigned char> data{std::istreambuf_iterator<char>{input}, {}};
        if (data.size() < 128) throw std::runtime_error("short input");
        auto invalid = data;
        invalid[0] = 1;
        void* handle{};
        if (tb_shadow_create(ctx.device.Get(), invalid.data(), invalid.size(), &handle) != -2 || handle)
            throw std::runtime_error("invalid serialized header accepted");
        handle = reinterpret_cast<void*>(1);
        if (tb_shadow_create(ctx.device.Get(), data.data(), 127, &handle) != -1 || handle)
            throw std::runtime_error("short source accepted or stale output retained");
        invalid = data;
        UINT root_pointer{}; std::memcpy(&root_pointer, invalid.data() + 112, sizeof(root_pointer));
        const size_t node = 64 + (root_pointer >> 3) * 64;
        if ((root_pointer & 7) != 4 || node + 28 > invalid.size()) throw std::runtime_error("unexpected root");
        const unsigned short nan = 0x7e00;
        std::memcpy(invalid.data() + node + 22, &nan, sizeof(nan));
        if (tb_shadow_create(ctx.device.Get(), invalid.data(), invalid.size(), &handle) != -2 || handle)
            throw std::runtime_error("invalid node bounds accepted");
        if (tb_shadow_create(ctx.device.Get(), data.data(), data.size(), &handle) != 0 || !handle)
            throw std::runtime_error("owned shadow creation");
        UINT64 address{}, size{};
        if (tb_shadow_info(handle, &address, &size) != 0 || size != data.size()-64 || !address)
            throw std::runtime_error("shadow address or size");
        std::array<unsigned char,64> expected{};
        std::memcpy(expected.data(), data.data()+64, expected.size());
        data[64] ^= 0x5a; // must not mutate the immutable GPU snapshot
        auto output = buffer(ctx.device.Get(), 64, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
        auto readback = buffer(ctx.device.Get(), 64, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
        auto output2 = buffer(ctx.device.Get(), 64, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
        auto readback2 = buffer(ctx.device.Get(), 64, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
        D3D12_COMMAND_QUEUE_DESC queue_desc{};
        ComPtr<ID3D12CommandQueue> queue2;
        check(ctx.device->CreateCommandQueue(&queue_desc, IID_PPV_ARGS(&queue2)), "second queue");
        D3D12_ROOT_PARAMETER params[2]{};
        params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_SRV;
        params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_UAV;
        D3D12_ROOT_SIGNATURE_DESC desc{2, params, 0, nullptr, D3D12_ROOT_SIGNATURE_FLAG_NONE};
        ComPtr<ID3DBlob> blob;
        check(D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &blob, nullptr), "root serialization");
        ComPtr<ID3D12RootSignature> root;
        check(ctx.device->CreateRootSignature(0, blob->GetBufferPointer(), blob->GetBufferSize(), IID_PPV_ARGS(&root)), "root");
        D3D12_COMPUTE_PIPELINE_STATE_DESC pd{}; pd.pRootSignature = root.Get();
        pd.CS = {owned_shadow_dxil, sizeof(owned_shadow_dxil)};
        ComPtr<ID3D12PipelineState> pso;
        check(ctx.device->CreateComputePipelineState(&pd, IID_PPV_ARGS(&pso)), "pipeline");
        ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> list;
        check(ctx.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator)), "allocator");
        check(ctx.device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator.Get(), pso.Get(), IID_PPV_ARGS(&list)), "list");
        list->SetComputeRootSignature(root.Get());
        list->SetComputeRootShaderResourceView(0, address);
        list->SetComputeRootUnorderedAccessView(1, output->GetGPUVirtualAddress());
        list->Dispatch(1,1,1);
        transition(list.Get(), output.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list->CopyBufferRegion(readback.Get(), 0, output.Get(), 0, 64);
        check(list->Close(), "close");
        ComPtr<ID3D12CommandAllocator> allocator2; ComPtr<ID3D12GraphicsCommandList> list2;
        check(ctx.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator2)), "second allocator");
        check(ctx.device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator2.Get(), pso.Get(), IID_PPV_ARGS(&list2)), "second list");
        list2->SetComputeRootSignature(root.Get());
        list2->SetComputeRootShaderResourceView(0, address);
        list2->SetComputeRootUnorderedAccessView(1, output2->GetGPUVirtualAddress());
        list2->Dispatch(1,1,1);
        transition(list2.Get(), output2.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list2->CopyBufferRegion(readback2.Get(), 0, output2.Get(), 0, 64);
        check(list2->Close(), "close second list");
        ComPtr<ID3D12Fence> gate, gate2, complete, extra;
        check(ctx.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&gate)), "gate");
        check(ctx.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&gate2)), "second gate");
        check(ctx.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&complete)), "complete");
        check(ctx.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&extra)), "extra");
        if (tb_shadow_hold(handle, complete.Get(), 1) || tb_shadow_hold(handle, extra.Get(), 1))
            throw std::runtime_error("register holds");
        if (tb_shadow_hold(handle, complete.Get(), 0) != -1 || tb_shadow_hold(handle, complete.Get(), ~UINT64(0)) != -1)
            throw std::runtime_error("reserved fence value accepted");
        check(ctx.queue->Wait(gate.Get(), 1), "queue wait");
        ID3D12CommandList* lists[] = {list.Get()};
        ctx.queue->ExecuteCommandLists(1, lists);
        check(ctx.queue->Signal(complete.Get(), 1), "queue signal");
        check(queue2->Wait(gate2.Get(), 1), "second queue wait");
        ID3D12CommandList* lists2[] = {list2.Get()};
        queue2->ExecuteCommandLists(1, lists2);
        check(queue2->Signal(extra.Get(), 1), "second queue signal");
        if (tb_shadow_try_release(handle) != 0) throw std::runtime_error("released while GPU blocked");
        check(gate->Signal(1), "release GPU gate");
        wait(complete.Get(), 1);
        D3D12_RANGE range{0,64}, none{}; void* mapped{};
        check(readback->Map(0, &range, &mapped), "readback");
        bool identical = std::memcmp(mapped, expected.data(), 64) == 0;
        readback->Unmap(0, &none);
        if (!identical) throw std::runtime_error("GPU snapshot differed");
        if (tb_shadow_try_release(handle) != 0) throw std::runtime_error("released with second fence pending");
        check(gate2->Signal(1), "release second GPU gate");
        wait(extra.Get(), 1);
        check(readback2->Map(0, &range, &mapped), "second readback");
        identical = std::memcmp(mapped, expected.data(), 64) == 0;
        readback2->Unmap(0, &none);
        if (!identical) throw std::runtime_error("second GPU snapshot differed");
        if (tb_shadow_try_release(handle) != 1) throw std::runtime_error("completed resource not retired");
        std::fprintf(receipt, "immutable_gpu_snapshot=1\nmalformed_rejected=1\nblocked_gpu_retained=1\ntwo_fence_retirement=1\ntwo_gpu_queues=1\nbody_bytes=%llu\n", size);
    } catch (const std::exception& e) { std::fprintf(receipt, "error=%s\n", e.what()); result = 1; }
    return std::fclose(receipt) ? 2 : result;
}
