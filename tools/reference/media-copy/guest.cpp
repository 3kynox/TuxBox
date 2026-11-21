// Exercise the five retail COPY-list methods and its separate chroma placement.
#include "frame_abi.h"
#include <cstring>
using frame_abi::check;
using frame_abi::ComPtr;
using frame_abi::method;
constexpr GUID compact{0x30bc77b9, 0x8379, 0x4b25, {0x95, 0xde, 0xb8, 0x58, 0x8c, 0x42, 0x27, 0x5d}};
constexpr UINT width = 94, height = 38, pitch = 256;
static ComPtr<ID3D12Resource> resource(ID3D12Device* d, DXGI_FORMAT fmt, UINT w, UINT h, D3D12_HEAP_TYPE heap,
                                       D3D12_RESOURCE_STATES state) {
    D3D12_HEAP_PROPERTIES hp{};
    hp.Type = heap;
    hp.CreationNodeMask = hp.VisibleNodeMask = 1;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension =
        fmt == DXGI_FORMAT_UNKNOWN ? D3D12_RESOURCE_DIMENSION_BUFFER : D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = w;
    desc.Height = h;
    desc.DepthOrArraySize = desc.MipLevels = desc.SampleDesc.Count = 1;
    desc.Format = fmt;
    desc.Layout = fmt == DXGI_FORMAT_UNKNOWN ? D3D12_TEXTURE_LAYOUT_ROW_MAJOR : D3D12_TEXTURE_LAYOUT_UNKNOWN;
    ComPtr<ID3D12Resource> r;
    check(d->CreateCommittedResource(&hp, D3D12_HEAP_FLAG_NONE, &desc, state, nullptr, IID_PPV_ARGS(&r)),
          "resource");
    return r;
}
static void submit(ID3D12Device* dev, ID3D12CommandQueue* q, ID3D12CommandList* list) {
    q->ExecuteCommandLists(1, &list);
    ComPtr<ID3D12Fence> f;
    check(dev->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&f)), "fence");
    check(q->Signal(f.Get(), 1), "signal");
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event)
        throw std::runtime_error("event");
    check(f->SetEventOnCompletion(1, event), "event schedule");
    DWORD wait = WaitForSingleObject(event, 15000);
    CloseHandle(event);
    if (wait != WAIT_OBJECT_0 || f->GetCompletedValue() != 1)
        throw std::runtime_error("GPU fence failed");
}
static D3D12_TEXTURE_COPY_LOCATION plane(ID3D12Resource* res, UINT id = 0) {
    D3D12_TEXTURE_COPY_LOCATION t{};
    t.pResource = res;
    t.SubresourceIndex = id;
    return t;
}
static D3D12_TEXTURE_COPY_LOCATION footprint(ID3D12Resource* res, UINT p) {
    D3D12_TEXTURE_COPY_LOCATION t{};
    t.pResource = res;
    t.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    t.PlacedFootprint.Offset = p ? pitch * height : 0;
    t.PlacedFootprint.Footprint = {p ? DXGI_FORMAT_R8G8_UNORM : DXGI_FORMAT_R8_UNORM, width >> p, height >> p, 1,
                                   pitch};
    return t;
}
static BYTE sample(UINT p, UINT x, UINT y) { return static_cast<BYTE>((p ? 181 : 17) + x * 19 + y * 43); }
static D3D12_RESOURCE_BARRIER transition(ID3D12Resource* r, UINT sub, D3D12_RESOURCE_STATES a,
                                         D3D12_RESOURCE_STATES b) {
    D3D12_RESOURCE_BARRIER v{};
    v.Transition = {r, sub, a, b};
    return v;
}
int main() {
    FILE* result{};
    if (fopen_s(&result, "result.txt", "wb"))
        return 2;
    setvbuf(result, nullptr, _IONBF, 0);
    try {
        const std::array<UINT, 18> profile{0x20004, 0, ~0u, 0, 0, 0, ~0u, ~0u, 0, 1};
        auto dll = LoadLibraryExW(L"d3d12_xs.dll", nullptr,
                                  LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
        using Factory = HRESULT(WINAPI*)(IUnknown*, const void*, REFIID, void**);
        auto spawn = dll ? reinterpret_cast<Factory>(GetProcAddress(dll, "D3D12XboxCreateDevice")) : nullptr;
        if (!spawn)
            throw std::runtime_error("factory");
        ComPtr<ID3D12Device> guest;
        check(spawn(nullptr, profile.data(), IID_PPV_ARGS(&guest)), "guest device");
        ComPtr<ID3D12CommandAllocator> alloc;
        check(guest->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_COPY, IID_PPV_ARGS(&alloc)), "copy allocator");
        ComPtr<ID3D12Device> dev;
        check(alloc->GetDevice(IID_PPV_ARGS(&dev)), "native device");
        ComPtr<IDXGIFactory4> fac;
        ComPtr<IDXGIAdapter1> adapter;
        DXGI_ADAPTER_DESC1 gd{};
        check(CreateDXGIFactory1(IID_PPV_ARGS(&fac)), "factory");
        check(fac->EnumAdapterByLuid(dev->GetAdapterLuid(), IID_PPV_ARGS(&adapter)), "adapter");
        check(adapter->GetDesc1(&gd), "get adapter");
        fprintf(result, "adapter=%04x:%04x\n", gd.VendorId, gd.DeviceId);
        D3D12_COMMAND_QUEUE_DESC qd{};
        qd.Type = D3D12_COMMAND_LIST_TYPE_COPY;
        ComPtr<ID3D12CommandQueue> q;
        check(method<frame_abi::CreateQueue>(guest.Get(), 48)(guest.Get(), &qd, IID_PPV_ARGS(&q)),
              "guest copy queue");
        auto nv = resource(dev.Get(), DXGI_FORMAT_NV12, width, height, D3D12_HEAP_TYPE_DEFAULT,
                           D3D12_RESOURCE_STATE_COMMON);
        auto y = resource(dev.Get(), DXGI_FORMAT_R8_UNORM, width, height, D3D12_HEAP_TYPE_DEFAULT,
                          D3D12_RESOURCE_STATE_COMMON);
        auto uv = resource(dev.Get(), DXGI_FORMAT_R8G8_UNORM, width / 2, height / 2, D3D12_HEAP_TYPE_DEFAULT,
                           D3D12_RESOURCE_STATE_COMMON);
        auto up = resource(dev.Get(), DXGI_FORMAT_UNKNOWN, pitch * height * 3 / 2, 1, D3D12_HEAP_TYPE_UPLOAD,
                           D3D12_RESOURCE_STATE_GENERIC_READ);
        auto back = resource(dev.Get(), DXGI_FORMAT_UNKNOWN, pitch * height * 3 / 2, 1, D3D12_HEAP_TYPE_READBACK,
                             D3D12_RESOURCE_STATE_COPY_DEST);
        void* data{};
        D3D12_RANGE empty{};
        check(up->Map(0, &empty, &data), "upload map");
        for (UINT p = 0; p < 2; p++)
            for (UINT row = 0; row < (height >> p); row++)
                for (UINT x = 0; x < width; x++)
                    static_cast<BYTE*>(data)[(p ? height * pitch : 0) + row * pitch + x] = sample(p, x, row);
        up->Unmap(0, nullptr);
        ComPtr<ID3D12CommandAllocator> prep;
        check(dev->CreateCommandAllocator(qd.Type, IID_PPV_ARGS(&prep)), "prep allocator");
        ComPtr<ID3D12GraphicsCommandList> native;
        check(dev->CreateCommandList(0, qd.Type, prep.Get(), nullptr, IID_PPV_ARGS(&native)), "prep list");
        for (UINT p = 0; p < 2; p++) {
            auto src = footprint(up.Get(), p), dst = plane(nv.Get(), p);
            native->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        }
        check(native->Close(), "close upload");
        submit(dev.Get(), q.Get(), native.Get());
        UINT params[4] = {3, 0, 1, 0};
        ComPtr<IUnknown> list;
        check(method<frame_abi::CreateList>(guest.Get(), 73)(guest.Get(), params, alloc.Get(), nullptr, compact,
                                                             reinterpret_cast<void**>(list.GetAddressOf())),
              "create compact copy list");
        ComPtr<IUnknown> same;
        check(list->QueryInterface(compact, reinterpret_cast<void**>(same.GetAddressOf())),
              "query compact identity");
        if (same.Get() != list.Get())
            throw std::runtime_error("compact COM identity");
        same.Reset();
        using Close = HRESULT(WINAPI*)(IUnknown*);
        using Reset = HRESULT(WINAPI*)(IUnknown*, ID3D12CommandAllocator*, ID3D12PipelineState*);
        auto close = method<Close>(list.Get(), 9);
        check(close(list.Get()), "close initial");
        check(method<Reset>(list.Get(), 10)(list.Get(), alloc.Get(), nullptr), "reset compact");
        using Barrier = void(WINAPI*)(IUnknown*, UINT, const D3D12_RESOURCE_BARRIER*);
        auto barriers = method<Barrier>(list.Get(), 15);
        auto media_read = static_cast<D3D12_RESOURCE_STATES>(0x880);
        D3D12_RESOURCE_BARRIER before[] = {
            transition(nv.Get(), 0, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition(y.Get(), ~0u, media_read, D3D12_RESOURCE_STATE_COPY_DEST),
            transition(uv.Get(), ~0u, media_read, D3D12_RESOURCE_STATE_COPY_DEST)};
        barriers(list.Get(), 3, before);
        using Copy = void(WINAPI*)(IUnknown*, const D3D12_TEXTURE_COPY_LOCATION*, UINT, UINT, UINT,
                                   const D3D12_TEXTURE_COPY_LOCATION*, const D3D12_BOX*);
        auto copy = method<Copy>(list.Get(), 12);
        auto luma = plane(nv.Get()), destY = plane(y.Get());
        D3D12_BOX rect{0, 0, 0, width, height, 1};
        copy(list.Get(), &destY, 0, 0, 0, &luma, &rect);
        using Placed =
            HRESULT(WINAPI*)(ID3D12Device*, UINT64, const D3D12_RESOURCE_DESC*, UINT, const void*, REFIID, void**);
        D3D12_RESOURCE_DESC aliasDesc = uv->GetDesc();
        aliasDesc.Width = pitch / 2;
        ComPtr<ID3D12Resource> alias;
        check(method<Placed>(guest.Get(), 44)(guest.Get(), nv->GetGPUVirtualAddress() + pitch * height, &aliasDesc,
                                              0x800, nullptr, IID_PPV_ARGS(&alias)),
              "console chroma alias");
        auto srcUV = plane(alias.Get()), dstUV = plane(uv.Get());
        rect.right /= 2;
        rect.bottom /= 2;
        copy(list.Get(), &dstUV, 0, 0, 0, &srcUV, &rect);
        alias.Reset();
        for (auto& b : before)
            std::swap(b.Transition.StateBefore, b.Transition.StateAfter);
        barriers(list.Get(), 3, before);
        check(close(list.Get()), "close recorded");
        nv.Reset(); // The recording allocator must retain the actual source until the GPU finishes.
        submit(dev.Get(), q.Get(), reinterpret_cast<ID3D12CommandList*>(list.Get()));
        check(alloc->Reset(), "release completed recording");
        check(prep->Reset(), "reset prep allocator");
        check(native->Reset(prep.Get(), nullptr), "readback list");
        for (UINT p = 0; p < 2; p++) {
            auto src = plane(p ? uv.Get() : y.Get()), dst = footprint(back.Get(), p);
            native->CopyTextureRegion(&dst, 0, 0, 0, &src, nullptr);
        }
        check(native->Close(), "close readback");
        submit(dev.Get(), q.Get(), native.Get());
        check(back->Map(0, nullptr, &data), "read both planes");
        for (UINT p = 0; p < 2; p++) {
            UINT match = 0;
            for (UINT row = 0; row < (height >> p); row++)
                for (UINT x = 0; x < width; x++)
                    match +=
                        static_cast<BYTE*>(data)[(p ? height * pitch : 0) + row * pitch + x] == sample(p, x, row);
            fprintf(result, "plane_%u_matching=%u\n", p, match);
            if (match != width * (height >> p))
                throw std::runtime_error("plane content mismatch");
        }
        back->Unmap(0, &empty);
        fclose(result);
        return 0;
    } catch (const std::exception& e) {
        fprintf(result, "error=%s\n", e.what());
        fclose(result);
        return 1;
    }
}
