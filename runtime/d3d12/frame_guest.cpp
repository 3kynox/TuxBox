// Submit two different clears to the same target and preserve each in its own snapshot.
#include "frame_abi.h"
#include "frame_present.h"
#include "frame_cli.h"
#include <algorithm>
#include <cstring>
#include <memory>

// The desktop SDK declares this IID as a local symbol. Read the x64 import
// address table entry to exercise the same data import as the guest title.
extern "C" const IID* const __imp_IID_ID3D12Device;

namespace {
using namespace frame_abi;
constexpr UINT width = 37, height = 13;
constexpr UINT pitch = D3D12_TEXTURE_DATA_PITCH_ALIGNMENT;
constexpr D3D12_RECT inset{5, 2, 31, 10};
constexpr DXGI_FORMAT format = DXGI_FORMAT_R8G8B8A8_UNORM;
using Color = std::array<BYTE, 4>;
constexpr Color first{17, 91, 203, 255}, second{244, 51, 115, 69};
constexpr UINT64 interval = 4096, guard = D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT;
constexpr SIZE_T bytes = 2 * interval + guard;
static_assert(pitch >= width * 4 && guard + height * pitch <= interval);

void verify_device_contract(ID3D12Device* device) {
    const auto* imported = __imp_IID_ID3D12Device;
    const auto module = GetModuleHandleW(L"d3d12_xs.dll");
    if (!module || reinterpret_cast<const void*>(GetProcAddress(module, "IID_ID3D12Device")) != imported ||
        !imported || std::memcmp(imported, &__uuidof(ID3D12Device), sizeof(IID)) != 0)
        throw std::runtime_error("D3D12.x device IID data import mismatch");

    ComPtr<IUnknown> identity;
    check(device->QueryInterface(IID_PPV_ARGS(&identity)), "device IUnknown");
    ComPtr<ID3D12Device> original;
    check(identity.As(&original), "IUnknown to guest device");
    if (original.Get() != device) throw std::runtime_error("device COM identity changed");
    void* out = device;
    auto unknown = profile;
    unknown[0] ^= 1;
    if (D3D12XboxCreateDevice(nullptr, unknown.data(), __uuidof(ID3D12Device), &out) != E_NOTIMPL || out)
        throw std::runtime_error("unrecognized device-creation version accepted");
    out = device;
    if (D3D12XboxCreateDevice(nullptr, profile.data(), __uuidof(ID3D12Fence), &out) != E_NOINTERFACE || out)
        throw std::runtime_error("unrecognized factory IID accepted");
}

ComPtr<ID3D12Resource> resource(ID3D12Device* dev, bool texture) {
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = texture ? D3D12_HEAP_TYPE_DEFAULT : D3D12_HEAP_TYPE_READBACK;
    heap.CreationNodeMask = heap.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = texture ? D3D12_RESOURCE_DIMENSION_TEXTURE2D : D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width = texture ? width : bytes;
    desc.Height = texture ? height : 1;
    desc.DepthOrArraySize = desc.MipLevels = desc.SampleDesc.Count = 1;
    desc.Format = texture ? format : DXGI_FORMAT_UNKNOWN;
    desc.Flags = texture ? D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET : D3D12_RESOURCE_FLAG_NONE;
    desc.Layout = texture ? D3D12_TEXTURE_LAYOUT_UNKNOWN : D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ComPtr<ID3D12Resource> result;
    check(dev->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
          texture ? D3D12_RESOURCE_STATE_COMMON : D3D12_RESOURCE_STATE_COPY_DEST,
          nullptr, IID_PPV_ARGS(&result)), "create frame resource");
    return result;
}

void transition(ID3D12GraphicsCommandList* list, ID3D12Resource* texture,
                D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER b{};
    b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {texture, 0, before, after};
    list->ResourceBarrier(1, &b);
}

void record(ID3D12GraphicsCommandList* list, ID3D12DescriptorHeap* view,
            ID3D12Resource* texture, ID3D12Resource* readback, unsigned index) {
    transition(list, texture, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_RENDER_TARGET);
    const auto& color = index ? second : first;
    float rgba[4];
    for (size_t k = 0; k < 4; ++k) rgba[k] = color[k] / 255.f;
    const D3D12_CPU_DESCRIPTOR_HANDLE rtv{method<CpuHandle>(view, 9)(view)};
    method<Clear>(list, 52)(list, rtv, rgba, index ? 1 : 0, index ? &inset : nullptr);

    transition(list, texture, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION from{};
    from.pResource = texture;
    from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION to{};
    to.pResource = readback;
    to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    to.PlacedFootprint.Offset = guard + index * interval;
    to.PlacedFootprint.Footprint = {format, width, height, 1, pitch};
    method<Copy>(list, 83)(list, &to, 0, 0, 0, &from, nullptr, 0);
    transition(list, texture, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COMMON);
    check(list->Close(), "close recorded guest list");
}

void check_snapshots(const BYTE* mapped) {
    for (unsigned i = 0; i < 3; ++i) {
        const BYTE* start = mapped + i * interval;
        if (!std::all_of(start, start + guard, [](BYTE b) { return b == 0xa6; }))
            throw std::runtime_error("copy touched guard region " + std::to_string(i));
    }
    for (unsigned i = 0; i < 2; ++i)
        for (LONG y = 0; y < LONG(height); ++y)
            for (LONG x = 0; x < LONG(width); ++x) {
                const bool inside = x >= inset.left && x < inset.right && y >= inset.top && y < inset.bottom;
                const auto& want = i && inside ? second : first;
                if (std::memcmp(mapped + i * interval + guard + y * pitch + x * 4, want.data(), 4))
                    throw std::runtime_error("pixel mismatch in snapshot " + std::to_string(i + 1) +
                                             " at " + std::to_string(x) + "," + std::to_string(y));
            }
}

void wait_fence(ID3D12Fence* fence, UINT64 value) {
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) throw std::runtime_error("CreateEventW failed");
    HRESULT armed = fence->SetEventOnCompletion(value, event);
    const auto waited = SUCCEEDED(armed) ? WaitForSingleObject(event, 15000) : WAIT_FAILED;
    CloseHandle(event);
    check(armed, "arm guest fence");
    if (waited != WAIT_OBJECT_0 || fence->GetCompletedValue() != value)
        throw std::runtime_error("guest fence did not reach " + std::to_string(value));
}

void verify_compute_queue(ID3D12Device* device) {
    D3D12_COMMAND_QUEUE_DESC desc{};
    desc.Type = D3D12_COMMAND_LIST_TYPE_COMPUTE;
    ComPtr<ID3D12CommandQueue> compute;
    check(method<CreateQueue>(device, 48)(device, &desc, IID_PPV_ARGS(&compute)), "guest compute queue slot 48");
    ComPtr<ID3D12Device> parent;
    check(compute->GetDevice(IID_PPV_ARGS(&parent)), "get guest compute queue's device");
    if (parent.Get() != device) throw std::runtime_error("compute queue returned a different guest device");
    ComPtr<ID3D12Fence> fence;
    check(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)), "create compute queue fence");
    check(compute->Signal(fence.Get(), 7), "signal compute queue fence");
    wait_fence(fence.Get(), 7);
}

std::string run(const wchar_t* adapter, bool present) {
    DXGI_ADAPTER_DESC1 selected{};
    auto device = create_device(adapter, selected);
    verify_device_contract(device.Get());
    const auto actual = device->GetAdapterLuid();
    if (actual.LowPart != selected.AdapterLuid.LowPart || actual.HighPart != selected.AdapterLuid.HighPart)
        throw std::runtime_error("created device is on a different adapter");
    verify_compute_queue(device.Get());
    auto texture = resource(device.Get(), true);
    auto readback = resource(device.Get(), false);
    D3D12_RANGE empty{0, 0}, full{0, bytes};
    void* mapped = nullptr;
    check(readback->Map(0, &empty, &mapped), "map readback for guard initialization");
    std::memset(mapped, 0xa6, bytes);
    readback->Unmap(0, &full);

    D3D12_DESCRIPTOR_HEAP_DESC view_desc{};
    view_desc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    view_desc.NumDescriptors = 1;
    ComPtr<ID3D12DescriptorHeap> view;
    check(device->CreateDescriptorHeap(&view_desc, IID_PPV_ARGS(&view)), "create RTV heap");
    const D3D12_CPU_DESCRIPTOR_HANDLE rtv{method<CpuHandle>(view.Get(), 9)(view.Get())};
    if (!rtv.ptr) throw std::runtime_error("guest heap returned a null RTV handle");
    device->CreateRenderTargetView(texture.Get(), nullptr, rtv);

    D3D12_COMMAND_QUEUE_DESC desc{};
    desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ComPtr<ID3D12CommandQueue> queue;
    check(device->CreateCommandQueue(&desc, IID_PPV_ARGS(&queue)), "guest direct queue slot 8");
    ComPtr<ID3D12Device> parent;
    check(queue->GetDevice(IID_PPV_ARGS(&parent)), "get guest queue's device");
    if (parent.Get() != device.Get()) throw std::runtime_error("queue returned a different guest device");
    ComPtr<ID3D12Fence> fence;
    check(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)), "create frame fence");
    const UINT params[] = {D3D12_COMMAND_LIST_TYPE_DIRECT, 0, 1, 0};
    std::array<ComPtr<ID3D12CommandAllocator>, 2> alloc;
    std::array<ComPtr<ID3D12GraphicsCommandList>, 2> lists;
    for (unsigned i = 0; i < 2; ++i) {
        check(device->CreateCommandAllocator(desc.Type, IID_PPV_ARGS(&alloc[i])), "create direct allocator");
        check(method<CreateList>(device.Get(), 73)(device.Get(), params, alloc[i].Get(), nullptr,
              IID_PPV_ARGS(&lists[i])), "guest list slot 73");
        check(lists[i]->Reset(alloc[i].Get(), nullptr), "reset newly created guest list");
        record(lists[i].Get(), view.Get(), texture.Get(), readback.Get(), i);
    }
    std::unique_ptr<tuxbox_test::DisplayCheck> display;
    if (present) display = std::make_unique<tuxbox_test::DisplayCheck>(width, height);
    // Keep both lists/allocators alive until their queue reaches its final fence.
    for (unsigned i = 0; i < 2; ++i) {
        ID3D12CommandList* batch[] = {lists[i].Get()};
        queue->ExecuteCommandLists(1, batch);
        if (display) {
            const auto& center = i ? second : first;
            display->verify(queue.Get(), texture.Get(), i + 1, RGB(first[0], first[1], first[2]),
                            RGB(center[0], center[1], center[2]));
        }
        check(queue->Signal(fence.Get(), i + 1), "signal guest queue");
    }
    wait_fence(fence.Get(), 2);
    check(readback->Map(0, &full, &mapped), "map completed snapshots");
    try { check_snapshots(static_cast<const BYTE*>(mapped)); }
    catch (...) { readback->Unmap(0, &empty); throw; }
    readback->Unmap(0, &empty);
    char receipt[196];
    constexpr UINT pixels = 2 * width * height;
    constexpr UINT inset_pixels = (inset.right - inset.left) * (inset.bottom - inset.top);
    std::snprintf(receipt, sizeof(receipt),
                  "xbox-frame PASS pci=%04x:%04x submissions=2 snapshots=2 pixels=%u/%u inset=%u guards=3 fence=2 compute_fence=7%s",
                  selected.VendorId, selected.DeviceId, pixels, pixels, inset_pixels, present ? " displayed=2/2" : "");
    return receipt;
}
}

int wmain(int argc, wchar_t** argv) {
    return tuxbox_test::frame_main(argc, argv, "xbox-frame", true, run);
}
