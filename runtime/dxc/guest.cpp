// Guest for the DXC utility path used to obtain an owned, writable binary blob.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <objidl.h>
#include <dxcapi.h>
#include <wrl/client.h>
#include <wrl/implements.h>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <memory>
#include <stdexcept>
#include <string>

namespace {
using Microsoft::WRL::ComPtr;
constexpr UINT32 size = 257;

void check(HRESULT hr, const char* operation) {
    if (SUCCEEDED(hr)) return;
    char code[16];
    std::snprintf(code, sizeof(code), "%#lx", static_cast<unsigned long>(hr));
    throw std::runtime_error(std::string(operation) + " failed: " + code);
}

void run() {
    // Include every possible byte and an embedded zero before the last byte.
    std::array<BYTE, size> expected;
    for (UINT32 i = 0; i < size; ++i) expected[i] = static_cast<BYTE>(i * 73 + 19);
    auto source = std::make_unique<BYTE[]>(size);
    std::memcpy(source.get(), expected.data(), size);

    ComPtr<IDxcUtils> utils;
    check(DxcCreateInstance(CLSID_DxcUtils, IID_PPV_ARGS(&utils)), "DxcCreateInstance");
    ComPtr<IDxcBlobEncoding> original;
    check(utils->CreateBlob(source.get(), size, DXC_CP_ACP, &original), "IDxcUtils::CreateBlob");
    if (!original || original->GetBufferPointer() == source.get())
        throw std::runtime_error("CreateBlob did not return an independent allocation");
    SecureZeroMemory(source.get(), size);
    source.reset();

    ComPtr<IDxcBlob> retained;
    check(original.As(&retained), "query IDxcBlob");
    original.Reset();
    utils.Reset();
    auto bytes = static_cast<BYTE*>(retained->GetBufferPointer());
    if (!bytes || retained->GetBufferSize() != size || std::memcmp(bytes, expected.data(), size))
        throw std::runtime_error("retained blob lost the copied binary data or size");

    for (UINT32 i = 0; i < size; ++i) {
        expected[i] ^= 0xff;
        bytes[i] = expected[i];
    }
    if (retained->GetBufferSize() != size || std::memcmp(retained->GetBufferPointer(), expected.data(), size))
        throw std::runtime_error("writes through the blob's buffer did not persist");
}
// Track the allocations made through the caller's IMalloc across factory and blob lifetimes.
class Allocator final : public Microsoft::WRL::RuntimeClass<
        Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>, IMalloc> {
public:
    std::atomic<ULONG> allocations{0};
    std::atomic<ULONG> frees{0};
    void* STDMETHODCALLTYPE Alloc(SIZE_T bytes) override {
        auto p = HeapAlloc(GetProcessHeap(), 0, bytes);
        if (p) ++allocations;
        return p;
    }
    void* STDMETHODCALLTYPE Realloc(void* p, SIZE_T bytes) override {
        if (!p) return Alloc(bytes);
        if (bytes) return HeapReAlloc(GetProcessHeap(), 0, p, bytes);
        Free(p);
        return nullptr;
    }
    void STDMETHODCALLTYPE Free(void* p) override {
        if (p && HeapFree(GetProcessHeap(), 0, p)) ++frees;
    }
    SIZE_T STDMETHODCALLTYPE GetSize(void* p) override { return p ? HeapSize(GetProcessHeap(), 0, p) : SIZE_T(-1); }
    int STDMETHODCALLTYPE DidAlloc(void*) override { return -1; }
    void STDMETHODCALLTYPE HeapMinimize() override {}
};

void run_allocator() {
    const auto module = GetModuleHandleW(L"dxcompiler_xs.dll");
    if (!module || GetProcAddress(module, MAKEINTRESOURCEA(3)) != GetProcAddress(module, "DxcCreateInstance") ||
        GetProcAddress(module, MAKEINTRESOURCEA(4)) != GetProcAddress(module, "DxcCreateInstance2"))
        throw std::runtime_error("console DXC factory ordinals differ");
    auto allocator = Microsoft::WRL::Make<Allocator>();
    if (!allocator) throw std::bad_alloc();
    ComPtr<IDxcUtils> utils;
    check(DxcCreateInstance2(allocator.Get(), CLSID_DxcUtils, IID_PPV_ARGS(&utils)), "DxcCreateInstance2");
    if (!allocator->allocations) throw std::runtime_error("factory ignored caller allocator");
    constexpr BYTE source[] = {0xff, 0, 0x7f, 0x31, 0};
    ComPtr<IDxcBlobEncoding> blob;
    check(utils->CreateBlob(source, sizeof(source), DXC_CP_ACP, &blob), "CreateBlob");
    utils.Reset();
    if (!blob || blob->GetBufferSize() != sizeof(source) ||
        std::memcmp(blob->GetBufferPointer(), source, sizeof(source)))
        throw std::runtime_error("blob failed after factory release");
    blob.Reset();
    if (allocator->allocations != allocator->frees)
        throw std::runtime_error("caller allocator has outstanding allocations");
}

}

int wmain(int argc, wchar_t** argv) {
    FILE* report = stdout;
    const bool custom = argc > 1 && !std::wcscmp(argv[1], L"--allocator");
    const int next = custom ? 2 : 1;
    if (argc == next + 2 && !std::wcscmp(argv[next], L"--report") && *argv[next + 1]) {
        if (_wfopen_s(&report, argv[next + 1], L"wb") != 0) {
            std::fputs("dxc-utils failed: cannot open report\n", stderr);
            return 1;
        }
    } else if (argc != next) {
        std::fputs("usage: dxc-utils.exe [--allocator] [--report FILE]\n", stderr);
        return 2;
    }
    int status = 0;
    try {
        int written;
        if (custom) {
            run_allocator();
            written = std::fprintf(report, "dxc-allocator PASS ordinals=3,4 custom=1 retained=1 balanced=1\n");
        } else {
            run();
            written = std::fprintf(report, "dxc-utils PASS bytes=%u source-freed=1 writable=%u retained=1\n", size, size);
        }
        if (written < 0)
            throw std::runtime_error("cannot write receipt");
    } catch (const std::exception& error) {
        std::fprintf(stderr, "dxc-utils failed: %s\n", error.what());
        if (report != stdout) std::fprintf(report, "dxc-utils failed: %s\n", error.what());
        status = 1;
    }
    if (report != stdout && std::fclose(report) != 0) status = 1;
    return status;
}
