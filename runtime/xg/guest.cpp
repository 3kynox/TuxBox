// Independent guest for the Scarlett address computer. Frosty calls slot 2
// and copies each complete 0x6c8-byte plane before releasing via slot 1.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <dxgiformat.h>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cwchar>

namespace {
struct Desc {
    UINT dimension;
    UINT64 alignment, width;
    UINT height;
    UINT16 slices, mips;
    DXGI_FORMAT format;
    UINT samples, quality, layout, flags;
};
static_assert(sizeof(Desc) == 56 && offsetof(Desc, width) == 16 && offsetof(Desc, layout) == 44);

struct Layout {
    UINT64 bytes, alignment;
    UINT mips, planes;
    BYTE plane[4][0x6c8];
    BYTE tail[8];
};
static_assert(sizeof(Layout) == 0x1b40 && offsetof(Layout, plane) == 0x18);

struct Computer;
struct Methods {
    void* slot0;
    ULONG (WINAPI* release)(Computer*);
    HRESULT (WINAPI* get_layout)(Computer*, Layout*);
};
struct Computer { const Methods* methods; };
extern "C" __declspec(dllimport) HRESULT WINAPI XGCreateTextureComputer(const Desc*, Computer**);

struct Sample {
    UINT width, height;
    UINT16 mips;
    DXGI_FORMAT format;
    UINT layout;
    UINT64 bytes, alignment, plane_hash;
};
// Reference outputs from Frosty's native XG module, SHA-256
// fee85ef80b6fe47edaa083270933c08a37f9e27e7541f86f731daecf7ead1493.
// Layouts 256 and 261 produce distinct footprints for the same RGBA texture.
constexpr Sample samples[]{
    {37, 13, 1, DXGI_FORMAT_R8G8B8A8_UNORM, 256, 3328, 256, 0xfc4498b176d2d5d7},
    {37, 13, 1, DXGI_FORMAT_R8G8B8A8_UNORM, 261, 8192, 4096, 0x4622024bbf0f7ee3},
    {128, 128, 5, DXGI_FORMAT_R8G8B8A8_UNORM, 261, 90112, 4096, 0xdb330eae8a07b417},
    {65, 33, 3, DXGI_FORMAT_BC7_UNORM, 261, 16384, 4096, 0x28e99d8a6e487a93},
};

struct Guarded {
    std::array<BYTE, 32> before;
    Layout layout;
    std::array<BYTE, 32> after;
};
bool guards_intact(const Guarded& output) {
    for (BYTE value : output.before) if (value != 0xa7) return false;
    for (BYTE value : output.after) if (value != 0xa7) return false;
    return true;
}

bool verify(const Sample& sample) {
    Desc desc{};
    desc.dimension = 3; // Texture2D
    desc.width = sample.width;
    desc.height = sample.height;
    desc.slices = desc.samples = 1;
    desc.mips = sample.mips;
    desc.format = sample.format;
    desc.layout = sample.layout;
    Computer* computer = nullptr;
    HRESULT hr = XGCreateTextureComputer(&desc, &computer);
    if (FAILED(hr) || !computer) {
        std::fprintf(stderr, "XGCreateTextureComputer failed, HRESULT %#lx\n", static_cast<unsigned long>(hr));
        return false;
    }
    Guarded first, second;
    std::memset(&first, 0xa7, sizeof(first));
    std::memset(&second, 0xa7, sizeof(second));
    std::memset(&second.layout, 0x35, sizeof(Layout));
    hr = computer->methods->get_layout(computer, &first.layout);
    if (SUCCEEDED(hr)) hr = computer->methods->get_layout(computer, &second.layout);
    const ULONG remaining = computer->methods->release(computer);
    const auto& result = first.layout;
    UINT64 hash = 14695981039346656037ull;
    for (BYTE b : result.plane[0]) hash = (hash ^ b) * 1099511628211ull;
    if (FAILED(hr) || remaining || !guards_intact(first) || !guards_intact(second) ||
        std::memcmp(&result, &second.layout, sizeof(Layout)) || result.planes != 1 ||
        result.mips != sample.mips || result.bytes != sample.bytes ||
        result.alignment != sample.alignment || hash != sample.plane_hash) {
        std::fprintf(stderr, "XG layout mismatch for %ux%u, format %u, layout %u (hr %#lx, bytes %llu, hash %#llx)\n",
                     sample.width, sample.height, static_cast<unsigned>(sample.format), sample.layout,
                     static_cast<unsigned long>(hr), result.bytes, hash);
        return false;
    }
    return true;
}
}

int wmain(int argc, wchar_t** argv) {
    if (argc != 1 && (argc != 3 || std::wcscmp(argv[1], L"--report") || !*argv[2])) {
        std::fputs("usage: xg-texture.exe [--report FILE]\n", stderr);
        return 2;
    }
    for (const auto& sample : samples) if (!verify(sample)) return 1;
    constexpr char receipt[] = "xg-texture PASS descriptors=4 layout-queries=8 plane-records=4 releases=4";
    if (argc == 3) {
        FILE* file;
        if (_wfopen_s(&file, argv[2], L"wb")) return 1;
        const int written = std::fprintf(file, "%s\n", receipt);
        if (std::fclose(file) || written < 0) return 1;
    }
    std::puts(receipt);
    return 0;
}
