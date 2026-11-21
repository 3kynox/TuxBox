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
    ULONG (WINAPI* add_ref)(Computer*);
    ULONG (WINAPI* release)(Computer*);
    HRESULT (WINAPI* get_layout)(Computer*, Layout*);
    void* other[3];
    UINT64 (WINAPI* element_offset)(Computer*, UINT, UINT, UINT64, UINT, UINT, UINT, UINT*);
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

// Hash the complete layout and every returned element address in mip/row order.
// Goldens were captured from the same native XG module as the samples above.
struct AddressCase {
    UINT width, height;
    UINT16 mips;
    DXGI_FORMAT format;
    UINT layout;
    UINT64 layout_hash, address_hash;
};
constexpr AddressCase address_cases[]{
    {1, 29, 5, DXGI_FORMAT_R8G8B8A8_UNORM, 261, 0x5fb8b9b2e388ec41, 0xfed8254ff0c5da20},
    {261, 183, 5, DXGI_FORMAT_B8G8R8A8_UNORM, 261, 0x245daf2aa8688834, 0xac277f5a5410c3ea},
    {128, 128, 5, DXGI_FORMAT_BC7_UNORM, 256, 0xcb6161b60b4aaca3, 0x86e3e5ec57eee485},
    {65, 33, 5, DXGI_FORMAT_BC7_UNORM, 261, 0x8a0097d72947a2da, 0x6e056f26dbe773ba},
    {513, 257, 5, DXGI_FORMAT_R8G8B8A8_UNORM, 256, 0xa21aa988236a8804, 0x0fb6d256e228228b},
    // Shape fields recorded during Frosty's startup (one sample with quality zero).
    {32, 32, 6, DXGI_FORMAT_B8G8R8A8_TYPELESS, 261, 0xba76c744255a51ae, 0xf62c0a92129835b4},
    {843, 5, 1, DXGI_FORMAT_B8G8R8A8_TYPELESS, 261, 0x3d34d55e54e1d625, 0x7f4f57f937403266},
    {512, 512, 10, DXGI_FORMAT_BC3_TYPELESS, 261, 0x57dcba197e218104, 0x43df594108211cfe},
    {256, 256, 9, DXGI_FORMAT_BC5_UNORM, 261, 0x733a03937023d402, 0xeaccc2ed7a2c7afe},
};
UINT64 hash_bytes(UINT64 hash, const void* bytes, size_t size) {
    auto p = static_cast<const BYTE*>(bytes);
    for (size_t i = 0; i < size; ++i) hash = (hash ^ p[i]) * 1099511628211ull;
    return hash;
}
bool verify_addresses(const AddressCase& test, UINT& checked) {
    Desc desc{};
    desc.dimension = 3;
    desc.width = test.width;
    desc.height = test.height;
    desc.mips = test.mips;
    desc.slices = desc.samples = 1;
    desc.format = test.format;
    desc.layout = test.layout;
    Computer* c = nullptr;
    if (FAILED(XGCreateTextureComputer(&desc, &c)) || !c) return false;
    bool ok = c->methods->add_ref(c) == 2;
    ok = (c->methods->release(c) == 1) && ok;
    Guarded output;
    std::memset(&output, 0xa7, sizeof(output));
    constexpr UINT64 seed = 14695981039346656037ull;
    ok = SUCCEEDED(c->methods->get_layout(c, &output.layout)) && guards_intact(output) &&
         hash_bytes(seed, &output.layout, sizeof(output.layout)) == test.layout_hash && ok;
    UINT64 hash = seed;
    const bool compressed = test.format == DXGI_FORMAT_BC7_UNORM || test.format == DXGI_FORMAT_BC3_TYPELESS ||
                            test.format == DXGI_FORMAT_BC5_UNORM;
    const UINT block = compressed ? 4 : 1;
    for (UINT mip = 0; mip < desc.mips; ++mip) {
        UINT w = test.width >> mip, h = test.height >> mip;
        w = ((w ? w : 1) + block - 1) / block;
        h = ((h ? h : 1) + block - 1) / block;
        for (UINT y = 0; y < h; ++y) for (UINT x = 0; x < w; ++x) {
            UINT bit = 0xffff;
            const auto offset = c->methods->element_offset(c, 0, mip, x, y, 0, 0, &bit);
            ok = bit == 0 && offset < output.layout.bytes && ok;
            hash = hash_bytes(hash, &offset, sizeof(offset));
            ++checked;
        }
    }
    ok = (c->methods->release(c) == 0) && hash == test.address_hash && ok;
    if (!ok) std::fprintf(stderr, "XG address mismatch for %ux%u, format %u, layout %u\n",
                          test.width, test.height, static_cast<unsigned>(test.format), test.layout);
    return ok;
}
}

int wmain(int argc, wchar_t** argv) {
    const bool addresses = argc > 1 && std::wcscmp(argv[1], L"--addresses") == 0;
    const int next = addresses ? 2 : 1;
    if (argc != next && (argc != next+2 || std::wcscmp(argv[next], L"--report") || !*argv[next+1])) {
        std::fputs("usage: xg-texture.exe [--addresses] [--report FILE]\n", stderr);
        return 2;
    }
    for (const auto& sample : samples) if (!verify(sample)) return 1;
    const char* receipt = "xg-texture PASS descriptors=4 layout-queries=8 plane-records=4 releases=4";
    char extended[96];
    if (addresses) {
        UINT checked = 0;
        for (const auto& test : address_cases) if (!verify_addresses(test, checked)) return 1;
        std::snprintf(extended, sizeof(extended), "xg-texture PASS layouts=13 traced-descriptors=4 element-addresses=%u", checked);
        receipt = extended;
    }
    if (argc == next+2) {
        FILE* file;
        if (_wfopen_s(&file, argv[next+1], L"wb")) return 1;
        const int written = std::fprintf(file, "%s\n", receipt);
        if (std::fclose(file) || written < 0) return 1;
    }
    std::puts(receipt);
    return 0;
}
