// Queue-ordered raw Xbox BLAS body retention control. No runtime integration.
#include "../independent-shaders/context.h"
#include "retained_dxil.h"
using namespace shader_control;

static void wait(ID3D12Fence* fence, UINT64 value) {
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) throw std::runtime_error("create event");
    HRESULT hr = fence->SetEventOnCompletion(value, event);
    DWORD result = SUCCEEDED(hr) ? WaitForSingleObject(event, 15000) : WAIT_FAILED;
    CloseHandle(event);
    check(hr, "set completion event");
    if (result != WAIT_OBJECT_0 || fence->GetCompletedValue() < value)
        throw std::runtime_error("wait for copy");
}

int main() {
    FILE* receipt{};
    if (fopen_s(&receipt, "result.txt", "wb")) return 2;
    int result = 0;
    try {
        Context ctx(receipt);
        std::ifstream input("source.bin", std::ios::binary);
        std::vector<char> bytes{std::istreambuf_iterator<char>{input}, {}};
        if (bytes.size() < 65 || bytes.size() > 16 * 1024 * 1024)
            throw std::runtime_error("invalid serialized body");
        const UINT body = static_cast<UINT>(bytes.size() - 64);
        constexpr UINT offset = 256;
        auto a = buffer(ctx.device.Get(), static_cast<UINT>(bytes.size()) + offset, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
        auto b = buffer(ctx.device.Get(), static_cast<UINT>(bytes.size()) + offset, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ);
        auto old = buffer(ctx.device.Get(), body, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST);
        auto newer = buffer(ctx.device.Get(), body, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST);
        auto clone = buffer(ctx.device.Get(), body, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST);
        auto readback = buffer(ctx.device.Get(), body * 3 + 192, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST);
        auto observed = buffer(ctx.device.Get(), 192, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS);
        D3D12_ROOT_PARAMETER params[4]{};
        for (UINT n = 0; n < 3; ++n) {
            params[n].ParameterType = D3D12_ROOT_PARAMETER_TYPE_SRV;
            params[n].Descriptor.ShaderRegister = n;
        }
        params[3].ParameterType = D3D12_ROOT_PARAMETER_TYPE_UAV;
        D3D12_ROOT_SIGNATURE_DESC desc{4, params, 0, nullptr, D3D12_ROOT_SIGNATURE_FLAG_NONE};
        ComPtr<ID3DBlob> serialized;
        check(D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &serialized, nullptr), "root serialization");
        ComPtr<ID3D12RootSignature> root;
        check(ctx.device->CreateRootSignature(0, serialized->GetBufferPointer(), serialized->GetBufferSize(), IID_PPV_ARGS(&root)), "root signature");
        D3D12_COMPUTE_PIPELINE_STATE_DESC pd{};
        pd.pRootSignature = root.Get();
        pd.CS = {retained_dxil, sizeof(retained_dxil)};
        ComPtr<ID3D12PipelineState> pipeline;
        check(ctx.device->CreateComputePipelineState(&pd, IID_PPV_ARGS(&pipeline)), "pipeline");
        ComPtr<ID3D12CommandQueue> second;
        D3D12_COMMAND_QUEUE_DESC q{};
        check(ctx.device->CreateCommandQueue(&q, IID_PPV_ARGS(&second)), "second queue");
        ComPtr<ID3D12CommandAllocator> allocator_a, allocator_b;
        ComPtr<ID3D12GraphicsCommandList> list_a, list_b;
        check(ctx.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator_a)), "allocator a");
        check(ctx.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator_b)), "allocator b");
        check(ctx.device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator_a.Get(), nullptr, IID_PPV_ARGS(&list_a)), "list a");
        check(ctx.device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator_b.Get(), nullptr, IID_PPV_ARGS(&list_b)), "list b");
        list_a->CopyBufferRegion(old.Get(), 0, a.Get(), offset + 64, body);
        transition(list_a.Get(), old.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list_b->CopyBufferRegion(newer.Get(), 0, b.Get(), offset + 64, body);
        list_b->CopyBufferRegion(clone.Get(), 0, old.Get(), 0, body);
        transition(list_b.Get(), newer.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
        transition(list_b.Get(), clone.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list_b->CopyBufferRegion(readback.Get(), 0, old.Get(), 0, body);
        list_b->CopyBufferRegion(readback.Get(), body, newer.Get(), 0, body);
        list_b->CopyBufferRegion(readback.Get(), body * 2, clone.Get(), 0, body);
        transition(list_b.Get(), old.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        transition(list_b.Get(), newer.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        transition(list_b.Get(), clone.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
        list_b->SetComputeRootSignature(root.Get());
        list_b->SetPipelineState(pipeline.Get());
        list_b->SetComputeRootShaderResourceView(0, old->GetGPUVirtualAddress());
        list_b->SetComputeRootShaderResourceView(1, newer->GetGPUVirtualAddress());
        list_b->SetComputeRootShaderResourceView(2, clone->GetGPUVirtualAddress());
        list_b->SetComputeRootUnorderedAccessView(3, observed->GetGPUVirtualAddress());
        list_b->Dispatch(1, 1, 1);
        transition(list_b.Get(), observed.Get(), D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list_b->CopyBufferRegion(readback.Get(), body * 3, observed.Get(), 0, 192);
        check(list_a->Close(), "close a");
        check(list_b->Close(), "close b");
        // Populate both inputs after close. Only generation B differs, at the
        // last payload byte, so its distinct body is visible in the readback.
        std::vector<char> data(offset + bytes.size());
        std::copy(bytes.begin(), bytes.end(), data.begin() + offset);
        upload(a.Get(), data.data(), data.size());
        data.back() ^= 0x5a;
        upload(b.Get(), data.data(), data.size());
        ComPtr<ID3D12Fence> ready, complete;
        check(ctx.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&ready)), "ready fence");
        check(ctx.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&complete)), "completion fence");
        ID3D12CommandList* first[] = {list_a.Get()};
        ID3D12CommandList* next[] = {list_b.Get()};
        // Queue B waits on A's copy; CPU does not wait between submissions.
        check(second->Wait(ready.Get(), 1), "queue wait");
        second->ExecuteCommandLists(1, next);
        ctx.queue->ExecuteCommandLists(1, first);
        check(ctx.queue->Signal(ready.Get(), 1), "signal ready");
        check(second->Signal(complete.Get(), 1), "signal complete");
        wait(complete.Get(), 1);
        write_buffer(readback.Get(), "retained", body * 3 + 192);
        // The previous executions are complete before replay. Restore the
        // recorded initial states, rewrite both uploads, and execute again.
        ComPtr<ID3D12CommandAllocator> reset_allocator;
        ComPtr<ID3D12GraphicsCommandList> reset_list;
        check(ctx.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&reset_allocator)), "reset allocator");
        check(ctx.device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, reset_allocator.Get(), nullptr, IID_PPV_ARGS(&reset_list)), "reset list");
        transition(reset_list.Get(), old.Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
        transition(reset_list.Get(), newer.Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
        transition(reset_list.Get(), clone.Get(), D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
        transition(reset_list.Get(), observed.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        submit_and_wait(ctx.device.Get(), ctx.queue.Get(), reset_list.Get());
        upload(a.Get(), data.data(), data.size());
        data.back() ^= 0x5a;
        upload(b.Get(), data.data(), data.size());
        check(second->Wait(ready.Get(), 2), "replay queue wait");
        second->ExecuteCommandLists(1, next);
        ctx.queue->ExecuteCommandLists(1, first);
        check(ctx.queue->Signal(ready.Get(), 2), "replay signal ready");
        check(second->Signal(complete.Get(), 2), "replay signal complete");
        wait(complete.Get(), 2);
        write_buffer(readback.Get(), "replayed", body * 3 + 192);
        std::fprintf(receipt, "body_bytes=%u\nsource_offset=%u\nsource_after_close=1\ncross_queue_wait=1\ncompleted_list_replay=1\n", body, offset + 64);
    } catch (const std::exception& e) {
        std::fprintf(receipt, "error=%s\n", e.what()); result = 1;
    }
    return std::fclose(receipt) ? 2 : result;
}
