// Guest for the DXC utility path used to obtain an owned, writable binary blob.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <objidl.h>
#include <dxcapi.h>
#include <wrl/client.h>
#include <array>
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
}

int wmain(int argc, wchar_t** argv) {
    FILE* report = stdout;
    if (argc == 3 && !std::wcscmp(argv[1], L"--report") && *argv[2]) {
        if (_wfopen_s(&report, argv[2], L"wb") != 0) {
            std::fputs("dxc-utils failed: cannot open report\n", stderr);
            return 1;
        }
    } else if (argc != 1) {
        std::fputs("usage: dxc-utils.exe [--report FILE]\n", stderr);
        return 2;
    }
    int status = 0;
    try {
        run();
        if (std::fprintf(report, "dxc-utils PASS bytes=%u source-freed=1 writable=%u retained=1\n", size, size) < 0)
            throw std::runtime_error("cannot write receipt");
    } catch (const std::exception& error) {
        std::fprintf(stderr, "dxc-utils failed: %s\n", error.what());
        if (report != stdout) std::fprintf(report, "dxc-utils failed: %s\n", error.what());
        status = 1;
    }
    if (report != stdout && std::fclose(report) != 0) status = 1;
    return status;
}
