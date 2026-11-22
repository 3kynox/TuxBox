// Differential guest for the Xbox-specific 3D layout and computer exports.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <initializer_list>
#include <cstring>

struct Desc3D { uint32_t width, height, depth, mips, format, usage, bind_flags, cpu_access, misc_flags, swizzle, pitch; };
struct Comp;
struct Methods {
    ULONG (WINAPI* add_ref)(Comp*);
    ULONG (WINAPI* release)(Comp*);
    HRESULT (WINAPI* layout)(Comp*, void*);
    void* other[3];
    uint64_t (WINAPI* offset)(Comp*, uint32_t, uint32_t, uint64_t, uint32_t, uint32_t, uint32_t, uint32_t*);
};
struct Comp { const Methods* methods; };
struct Layout { uint8_t before[32], body[0x1b40], after[32]; };
using Create = HRESULT (WINAPI*)(const Desc3D*, Comp**);
using Compute = HRESULT (WINAPI*)(const Desc3D*, void*);
static_assert(sizeof(Desc3D) == 44);

int main(int argc, char** argv) {
    HMODULE dll = LoadLibraryA("xg_subject.dll");
    if (!dll) return 1;
    auto create = reinterpret_cast<Create>(GetProcAddress(dll, "XGCreateTexture3DComputer"));
    auto compute = reinterpret_cast<Compute>(GetProcAddress(dll, "XGComputeTexture3DLayout"));
    if (!create || !compute) return 2;
    Desc3D sample{37, 13, 3, 3, 28, 3, 8, 0, 0, 0, 0};
    Layout unchanged; std::memset(&unchanged, 0xa5, sizeof(unchanged));
    Comp* sentinel = reinterpret_cast<Comp*>(static_cast<uintptr_t>(0x1234));
    if (compute(nullptr, unchanged.body) != E_INVALIDARG || compute(&sample, nullptr) != E_INVALIDARG ||
        create(nullptr, &sentinel) != E_INVALIDARG || sentinel != reinterpret_cast<Comp*>(static_cast<uintptr_t>(0x1234)) ||
        create(&sample, nullptr) != E_INVALIDARG) return 3;
    if (argc == 2 && !std::strcmp(argv[1], "unsupported")) {
        // This usage is outside TuxBox's verified subset.
        sample.usage = 2; sample.swizzle = 0;
        if (compute(&sample, unchanged.body) != E_NOTIMPL || create(&sample, &sentinel) != E_NOTIMPL || sentinel) return 4;
        for (uint8_t b : unchanged.body) if (b != 0xa5) return 5;
        std::puts("xg-3d unsupported descriptor rejected"); return 0;
    }
    const uint32_t formats[] = {1,2,3,4,9,10,11,12,13,14,15,16,17,18,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,41,42,43,48,49,50,51,52,53,54,56,57,58,59,60,61,62,63,64,65,87,88,90,91,92,93};
    FILE* file = nullptr; if (fopen_s(&file, "texture3d-results.bin", "wb") || !file) return 6;
    struct Shape { uint32_t width, height; };
    constexpr Shape shapes[] = {{1,1}, {1,13}, {13,1}, {13,37}, {37,13}, {128,65}, {2048,1}, {1,2048}};
    unsigned cases = 0, addresses = 0;
    for (uint32_t format : formats) for (uint32_t usage : {0u,1u,3u})
    for (Shape shape : shapes) for (uint32_t levels : {1u,3u}) for (uint32_t depth : {1u,2u,3u,8u,2048u}) {
        if (shape.width == 1 && shape.height == 1 && levels != 1) continue;
        Desc3D desc{shape.width,shape.height,depth,levels,format,usage,8,0,0,0,0};
        Layout direct, from_computer;
        std::memset(&direct, 0xa5, sizeof(direct)); std::memset(&from_computer, 0xa5, sizeof(from_computer));
        Comp* computer = nullptr;
        HRESULT a = compute(&desc, direct.body), b = create(&desc, &computer);
        if (a != S_OK || b != S_OK || !computer || computer->methods->layout(computer, from_computer.body) != S_OK ||
            std::memcmp(direct.body, from_computer.body, sizeof(direct.body))) return 7;
        for (auto value : direct.before) if (value != 0xa5) return 8;
        for (auto value : direct.after) if (value != 0xa5) return 8;
        for (auto value : from_computer.before) if (value != 0xa5) return 8;
        for (auto value : from_computer.after) if (value != 0xa5) return 8;
        if (std::fwrite(direct.body, sizeof(direct.body), 1, file) != 1) return 9;
        for (uint32_t mip = 0; mip < levels; ++mip) {
            uint32_t w = shape.width >> mip, h = shape.height >> mip, d = depth >> mip;
            if (!w) w = 1;
            if (!h) h = 1;
            if (!d) d = 1;
            for (uint32_t z : {0u, d/2, d-1, d}) {
                uint32_t bit = 0xffffffffu;
                uint64_t off = computer->methods->offset(computer, 0, mip, w-1, h-1, z, 0, &bit);
                if ((z < d) != (off != UINT64_MAX) || bit ||
                    std::fwrite(&off, sizeof(off), 1, file) != 1 ||
                    std::fwrite(&bit, sizeof(bit), 1, file) != 1) return 10;
                ++addresses;
            }
        }
        if (computer->methods->release(computer) != 0) return 11;
        ++cases;
    }
    if (std::fclose(file)) return 12;
    std::printf("xg-3d PASS %u layouts %u element addresses\n", cases, addresses);
    return 0;
}
