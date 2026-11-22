// Temporary diagnostic proxy; the supplied reference DLL is never embedded.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>
#include <stdio.h>

typedef struct {
    uint32_t dimension;
    uint64_t alignment, width;
    uint32_t height;
    uint16_t slices, mips;
    uint32_t format, samples, quality, layout, flags;
} Desc;
_Static_assert(sizeof(Desc) == 56, "unexpected x64 descriptor layout");

typedef HRESULT (WINAPI *Create)(const Desc *, void **);
static volatile LONG calls;

HRESULT WINAPI XGCreateTextureComputer(const Desc *desc, void **out) {
    DWORD incoming = GetLastError();
    HMODULE module = GetModuleHandleW(L"xg_reference.dll");
    if (!module) module = LoadLibraryW(L"xg_reference.dll");
    Create create = module ? (Create)(uintptr_t)GetProcAddress(module, "XGCreateTextureComputer") : NULL;
    if (!create) { SetLastError(ERROR_PROC_NOT_FOUND); return E_FAIL; }
    SetLastError(incoming);
    HRESULT hr = create(desc, out);
    DWORD last_error = GetLastError();
    LONG n = InterlockedIncrement(&calls);
    if (n <= 16384 && desc) {
        char text[512];
        int count = snprintf(text, sizeof(text),
            "XG_REFERENCE_TRACE call=%ld hr=%08lx dimension=%u alignment=%llu width=%llu height=%u slices=%u mips=%u format=%u samples=%u quality=%u layout=%u flags=%08x\n",
            n, (unsigned long)hr, desc->dimension, (unsigned long long)desc->alignment,
            (unsigned long long)desc->width, desc->height, desc->slices, desc->mips,
            desc->format, desc->samples, desc->quality, desc->layout, desc->flags);
        if (count > 0) OutputDebugStringA(text);
    } else if (n == 16385) {
        OutputDebugStringA("XG_REFERENCE_TRACE limit=16384 reached; further calls omitted\n");
    }
    SetLastError(last_error);
    return hr;
}
