#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <d3dcommon.h>
#include <d3d12sdklayers.h>
#include <dxgi.h>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include "function-ordinals.inc"

extern "C" __declspec(dllimport) HRESULT WINAPI D3D10CreateBlob(SIZE_T, ID3DBlob**);
extern "C" __declspec(dllimport) UINT WINAPI D3D12XboxGetProcessDebugFlags();
extern "C" __declspec(dllimport) HRESULT WINAPI D3D12XboxSetProcessDebugFlags(UINT);

// The Xbox object has two guest tables. Its first slot takes a flag mask.
struct Debug;
struct DebugMethods {
    HRESULT (WINAPI* set_flags)(Debug*, UINT);
};
struct DebugUnknown {
    HRESULT (WINAPI* query)(Debug*, REFIID, void**);
    ULONG (WINAPI* add_ref)(Debug*);
    ULONG (WINAPI* release)(Debug*);
};
struct Debug {
    const DebugMethods* methods;
    const DebugUnknown* unknown;
};

static FARPROC lookup(HMODULE module, const char* name, WORD ordinal) {
    auto address = GetProcAddress(module, name);
    if (address && address == GetProcAddress(module, MAKEINTRESOURCEA(ordinal))) return address;
    std::fprintf(stderr, "Missing or mismatched %s at ordinal %u\n", name, ordinal);
    return nullptr;
}

static bool check_debug(HMODULE module) {
    using Factory = HRESULT (WINAPI*)(REFIID, void**);
    auto factory = reinterpret_cast<Factory>(lookup(module, "D3D12GetDebugInterface", ORD_D3D12GetDebugInterface));
    if (!factory) return false;
    Debug* debug = nullptr;
    const auto iid = __uuidof(ID3D12Debug);
    if (factory(iid, reinterpret_cast<void**>(&debug)) != S_OK || !debug) return false;
    void* other = nullptr;
    if (factory(iid, &other) != S_OK || other != debug ||
        factory(__uuidof(IUnknown), &other) != E_NOINTERFACE || other != debug ||
        debug->unknown->query(debug, __uuidof(IUnknown), &other) != E_NOINTERFACE || other != debug) return false;
    other = nullptr;
    if (debug->unknown->query(debug, iid, &other) != S_OK || other != debug ||
        debug->unknown->add_ref(debug) != 1 || debug->unknown->release(debug) != 0) return false;
    const UINT saved_flags = D3D12XboxGetProcessDebugFlags();
    const bool flags_ok = debug->methods->set_flags(debug, 0x81123457) == S_OK &&
                          D3D12XboxGetProcessDebugFlags() == 0x81100001 &&
                          debug->methods->set_flags(debug, 0) == S_OK && D3D12XboxGetProcessDebugFlags() == 0;
    return D3D12XboxSetProcessDebugFlags(saved_flags) == S_OK && flags_ok;
}

static bool check_dxgix(HMODULE module) {
    struct Entry { const char* name; WORD ordinal; };
    constexpr Entry entries[]{
        {"DXGIXGetFrameStatistics", ORD_DXGIXGetFrameStatistics},
        {"DXGIXPresentArray", ORD_DXGIXPresentArray},
        {"DXGIXSetFrameNotification", ORD_DXGIXSetFrameNotification},
        {"DXGIXSetVLineNotification", ORD_DXGIXSetVLineNotification},
    };
    // Both reference DLLs return their HRESULT without reading any argument.
    using Call = HRESULT (WINAPI*)(void*, void*, void*, void*);
    for (const auto& entry : entries) {
        auto call = reinterpret_cast<Call>(lookup(module, entry.name, entry.ordinal));
        UINT canary = 0xfedcba98;
        if (!call || call(nullptr, nullptr, nullptr, nullptr) != DXGI_ERROR_UNSUPPORTED ||
            call(&canary, &canary, &canary, &canary) != DXGI_ERROR_UNSUPPORTED || canary != 0xfedcba98) return false;
    }
    return true;
}

static bool check_blob(HMODULE module) {
    if (!lookup(module, "D3D10CreateBlob", ORD_D3D10CreateBlob)) return false;
    ID3DBlob* blob = nullptr;
    const HRESULT hr = D3D10CreateBlob(257, &blob);
    if (hr != S_OK || !blob) {
        std::fprintf(stderr, "D3D10CreateBlob returned %#lx\n", hr);
        return false;
    }
    if (blob->GetBufferSize() != 257 || !blob->GetBufferPointer()) return false;
    ID3DBlob* other = nullptr;
    if (blob->QueryInterface(IID_PPV_ARGS(&other)) != S_OK || other != blob || other->Release() != 1) return false;
    auto bytes = static_cast<BYTE*>(blob->GetBufferPointer());
    std::memset(bytes, 0x49, 257);
    if (blob->AddRef() != 2 || blob->Release() != 1) return false;
    return bytes[0] == 0x49 && bytes[256] == 0x49 && blob->Release() == 0;
}

int wmain(int argc, wchar_t** argv) {
    int arg = 1;
    // Pin the packaged driver during a control run, as a live device would.
    if (argc > 2 && !std::wcscmp(argv[arg], L"--reference-umd")) {
        if (!LoadLibraryW(argv[arg + 1])) {
            std::fprintf(stderr, "Could not load reference driver (%lu)\n", GetLastError());
            return 2;
        }
        arg += 2;
    }
    if (argc != arg && (argc != arg + 2 || std::wcscmp(argv[arg], L"--report"))) return 2;
    const HMODULE module = GetModuleHandleW(L"d3d12_xs.dll");
    if (!module || !check_debug(module)) { std::fputs("Debug interface failed\n", stderr); return 1; }
    if (!check_dxgix(module)) { std::fputs("Legacy DXGIX calls failed\n", stderr); return 1; }
    if (!check_blob(module)) { std::fputs("Blob checks failed\n", stderr); return 1; }
    FILE* out = stdout;
    if (argc != arg && _wfopen_s(&out, argv[arg + 1], L"wb")) return 2;
    const int result = std::fputs("d3d12-exports PASS debug=1 blob=1 dxgix=4\n", out);
    return (out != stdout && std::fclose(out)) || result < 0;
}
