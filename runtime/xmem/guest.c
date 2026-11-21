#include "xmem.h"
#include <stdio.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <psapi.h>
#include <wchar.h>

static FILE *report;
static int failure(int line, const char *check)
{
    fprintf(stderr, "xmem-default failed at line %d: %s\n", line, check);
    if (report) fprintf(report, "xmem-default failed at line %d: %s\n", line, check);
    return 1;
}
#define REQUIRE(check) do { if (!(check)) return failure(__LINE__, #check); } while (0)

static int app_local_provider(void)
{
    wchar_t executable[MAX_PATH], dll[MAX_PATH];
    HMODULE loaded = GetModuleHandleW(L"xmem.dll");
    DWORD n = GetModuleFileNameW(NULL, executable, MAX_PATH);
    if (!loaded || !n || n >= MAX_PATH) return 0;
    n = GetModuleFileNameW(loaded, dll, MAX_PATH);
    if (!n || n >= MAX_PATH) return 0;
    wchar_t *exe_tail = wcsrchr(executable, L'\\');
    wchar_t *dll_tail = wcsrchr(dll, L'\\');
    if (!exe_tail || !dll_tail || _wcsicmp(dll_tail + 1, L"xmem.dll")) return 0;
    *exe_tail = *dll_tail = 0;
    return _wcsicmp(executable, dll) == 0;
}

static int working_set(void)
{
    _Static_assert(sizeof(XMEM_TITLE_STATISTICS_V2) == 120, "v2 statistics size");
    _Static_assert(offsetof(XMEM_TITLE_STATISTICS_V2, pool_budget) == 40, "single pool offset");
    _Static_assert(offsetof(XMEM_TITLE_STATISTICS_V2, peak_committed) == 88, "peak offset");
    struct {
        UINT64 guard_before;
        XMEM_TITLE_STATISTICS_V2 stats;
        UINT64 guard_after;
    } output;
    memset(&output, 0xa5, sizeof output);
    XMEM_TITLE_STATISTICS_V2 untouched = output.stats;
    REQUIRE(!XMemGetWorkingSetStatistics(0x10000001u, &output.stats));
    REQUIRE(GetLastError() == ERROR_NOT_SUPPORTED && !memcmp(&output.stats, &untouched, sizeof untouched));
    REQUIRE(!XMemGetWorkingSetStatistics(XMEM_TITLE_WORKING_SET_V2, NULL));
    REQUIRE(GetLastError() == ERROR_INVALID_PARAMETER);

    // As in the title, read real process-wide values, including memory acquired outside XMem.
    const SIZE_T size = 8u << 20;
    volatile BYTE *pages = VirtualAlloc(NULL, size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    REQUIRE(pages);
    for (SIZE_T i = 0; i < size; i += 4096) pages[i] = 0x35;
    REQUIRE(XMemGetWorkingSetStatistics(XMEM_TITLE_WORKING_SET_V2, &output.stats));
    MEMORYSTATUSEX host = {0};
    host.dwLength = sizeof host;
    PROCESS_MEMORY_COUNTERS_EX process = {0};
    process.cb = sizeof process;
    REQUIRE(GlobalMemoryStatusEx(&host));
    REQUIRE(K32GetProcessMemoryInfo(GetCurrentProcess(), (PROCESS_MEMORY_COUNTERS*)&process, sizeof process));
    REQUIRE(output.guard_before == 0xa5a5a5a5a5a5a5a5ULL && output.guard_after == output.guard_before);
    REQUIRE(output.stats.physical_budget <= host.ullTotalPhys && output.stats.physical_budget >= output.stats.committed);
    REQUIRE(output.stats.pool_budget == output.stats.physical_budget && output.stats.pool_committed == output.stats.committed);
    REQUIRE(output.stats.committed >= size && output.stats.peak_committed >= output.stats.committed);
    // The driver may charge a few pages between the two independent snapshots.
    UINT64 commit = process.PrivateUsage ? process.PrivateUsage : process.PagefileUsage;
    UINT64 expected = commit < host.ullTotalPhys ? commit : host.ullTotalPhys;
    UINT64 drift = expected > output.stats.committed ? expected - output.stats.committed : output.stats.committed - expected;
    REQUIRE(drift < 2u * 1024 * 1024);
    BYTE zero[32] = {0};
    REQUIRE(!memcmp(output.stats.reserved1, zero, sizeof output.stats.reserved1));
    REQUIRE(!memcmp(output.stats.reserved2, zero, sizeof output.stats.reserved2));
    REQUIRE(!memcmp(output.stats.reserved3, zero, sizeof output.stats.reserved3));
    REQUIRE(VirtualFree((void*)pages, 0, MEM_RELEASE));
    return 0;
}

static int virtual_reserve_commit(void)
{
    SYSTEM_INFO info;
    GetSystemInfo(&info);
    SIZE_T page = info.dwPageSize;
    MEMORY_BASIC_INFORMATION area;
    BYTE *base = XMemVirtualAlloc(NULL, 3 * page, MEM_RESERVE | MEM_TOP_DOWN, 1, PAGE_NOACCESS);
    REQUIRE(base && !((UINT_PTR)base % info.dwAllocationGranularity));
    REQUIRE(VirtualQuery(base, &area, sizeof area) == sizeof area);
    REQUIRE(area.State == MEM_RESERVE && area.AllocationBase == base && area.RegionSize >= 3 * page);
    volatile BYTE *middle = XMemVirtualAlloc(base + page, page, MEM_COMMIT, 1, PAGE_READWRITE);
    REQUIRE(middle == base + page);
    REQUIRE(VirtualQuery(base + page, &area, sizeof area) == sizeof area);
    REQUIRE(area.State == MEM_COMMIT && area.Protect == PAGE_READWRITE && area.AllocationBase == base);
    middle[0] = 0x73;
    middle[page - 1] = 0xa7;
    REQUIRE(middle[0] == 0x73 && middle[page - 1] == 0xa7);
    REQUIRE(VirtualQuery(base, &area, sizeof area) == sizeof area && area.State == MEM_RESERVE);
    REQUIRE(VirtualQuery(base + 2 * page, &area, sizeof area) == sizeof area && area.State == MEM_RESERVE);
    REQUIRE(VirtualFree(base, 0, MEM_RELEASE));
    REQUIRE(VirtualQuery(base, &area, sizeof area) == sizeof area && area.State == MEM_FREE);

    BYTE *direct = XMemVirtualAlloc(NULL, page, MEM_COMMIT, 1, PAGE_READWRITE);
    REQUIRE(direct && VirtualQuery(direct, &area, sizeof area) == sizeof area);
    REQUIRE(area.State == MEM_COMMIT && area.AllocationBase == direct && area.Protect == PAGE_READWRITE);
    REQUIRE(VirtualFree(direct, 0, MEM_RELEASE));
    REQUIRE(!XMemVirtualAlloc(NULL, page, MEM_RESERVE, 2, PAGE_NOACCESS) && GetLastError() == ERROR_NOT_SUPPORTED);
    const DWORD protections[] = {0x2404, 0x10404};
    const SIZE_T page64k = 65536;
    for (size_t i = 0; i < sizeof protections / sizeof protections[0]; ++i) {
        BYTE *aperture = XMemVirtualAlloc(NULL, 2 * page64k, MEM_RESERVE | 0x20400000u, 3, protections[i]);
        REQUIRE(aperture && VirtualQuery(aperture, &area, sizeof area) == sizeof area);
        REQUIRE(area.State == MEM_RESERVE && area.RegionSize >= 2 * page64k);
        BYTE *committed = XMemVirtualAlloc(aperture + page64k, page64k, MEM_COMMIT | 0x20400000u, 3, protections[i]);
        REQUIRE(committed == aperture + page64k);
        REQUIRE(VirtualQuery(committed, &area, sizeof area) == sizeof area && area.Protect == PAGE_READWRITE);
        committed[0] = 0x69;
        committed[page64k - 1] = 0x17;
        REQUIRE(committed[0] == 0x69 && committed[page64k - 1] == 0x17);
        REQUIRE(VirtualQuery(aperture, &area, sizeof area) == sizeof area && area.State == MEM_RESERVE);
        REQUIRE(VirtualFree(aperture, 0, MEM_RELEASE));
    }
    // The console interprets this flag pair as a larger page mode.
    REQUIRE(!XMemVirtualAlloc(NULL, page, MEM_COMMIT | 0x20400000u, 1, PAGE_READWRITE) &&
            GetLastError() == ERROR_NOT_SUPPORTED);
    return 0;
}

typedef void *(WINAPI *Allocation)(SIZE_T, UINT64);
typedef void (WINAPI *Release)(void *, UINT64);

static int allocation_pair(Allocation allocate, Release release)
{
    const UINT64 heap_attributes[] = {0x04800092, 0x04800040, 0x04800078, 0x04820000};
    for (size_t i = 0; i < sizeof heap_attributes / sizeof heap_attributes[0]; ++i) {
        BYTE *memory = allocate(257, heap_attributes[i]);
        REQUIRE(memory && ((UINT_PTR)memory & 15) == 0);
        REQUIRE(HeapValidate(GetProcessHeap(), 0, memory));
        memset(memory, 0xa5, 257);
        REQUIRE(memory[0] == 0xa5 && memory[256] == 0xa5);
        release(memory, heap_attributes[i]);
        REQUIRE(!HeapValidate(GetProcessHeap(), 0, memory));
    }
    enum { small_size = 12, placed_size = 65536 };
    volatile BYTE *small = allocate(small_size, XMEM_ORDINARY_ATTRIBUTES);
    volatile BYTE *placed = allocate(placed_size, XMEM_PLACEMENT_ATTRIBUTES);
    REQUIRE(small && placed);
    REQUIRE(((UINT_PTR)small & 15) == 0 && ((UINT_PTR)placed & 65535) == 0);
    REQUIRE(HeapValidate(GetProcessHeap(), 0, (void *)small));
    MEMORY_BASIC_INFORMATION area;
    REQUIRE(VirtualQuery((void *)placed, &area, sizeof area) == sizeof area);
    REQUIRE(area.State == MEM_COMMIT && area.AllocationBase == placed && area.RegionSize >= placed_size);

    for (unsigned i = 0; i < small_size; ++i) small[i] = (BYTE)(0xb1u + i);
    for (unsigned i = 0; i < placed_size; ++i) placed[i] = (BYTE)(i ^ (i >> 8));
    for (unsigned i = 0; i < small_size; ++i) REQUIRE(small[i] == (BYTE)(0xb1u + i));
    for (unsigned i = 0; i < placed_size; ++i) REQUIRE(placed[i] == (BYTE)(i ^ (i >> 8)));

    release((void *)small, XMEM_ORDINARY_ATTRIBUTES);
    REQUIRE(!HeapValidate(GetProcessHeap(), 0, (void *)small));
    release((void *)placed, XMEM_PLACEMENT_ATTRIBUTES);
    REQUIRE(VirtualQuery((void *)placed, &area, sizeof area) == sizeof area && area.State == MEM_FREE);
    REQUIRE(!allocate(0, XMEM_ORDINARY_ATTRIBUTES));
    REQUIRE(!allocate(1, 0) && GetLastError() == ERROR_NOT_SUPPORTED);
    release(NULL, XMEM_ORDINARY_ATTRIBUTES);

    return 0;
}

static BOOL page_state(BYTE *at, DWORD state)
{
    MEMORY_BASIC_INFORMATION info;
    return VirtualQuery(at, &info, sizeof info) == sizeof info && info.State == state;
}

static int physical_pages(void)
{
    enum { page = 65536 };
    const UINT64 flags = 0x20400000ULL, guard = 0xbfc3711234ULL;
    UINT64 storage[4] = {guard, 0, 0, guard}, *tokens = &storage[1];
    SIZE_T count = 2;
    HMODULE dll = GetModuleHandleW(L"xmem.dll");
    REQUIRE(GetProcAddress(dll, "XMemAllocatePhysicalPages") == GetProcAddress(dll, MAKEINTRESOURCEA(3)));
    REQUIRE(GetProcAddress(dll, "XMemMapPhysicalPages") == GetProcAddress(dll, MAKEINTRESOURCEA(21)));
    REQUIRE(GetProcAddress(dll, "XMemFreePhysicalPages") == GetProcAddress(dll, MAKEINTRESOURCEA(8)));
    if (!XMemAllocatePhysicalPages(flags, &count, tokens)) {
        DWORD error = GetLastError();
        if (report) fprintf(report, "XMemAllocatePhysicalPages failed error=%lu returned_pages=%zu\n", error, count);
        return failure(__LINE__, "allocate two physical pages");
    }
    REQUIRE(count == 2 && tokens[0] != tokens[1] && storage[0] == guard && storage[3] == guard);
    BYTE *base = VirtualAlloc(NULL, page * 4, MEM_RESERVE, PAGE_NOACCESS);
    REQUIRE(base && XMemMapPhysicalPages(base + page, count, tokens) == base + page);
    REQUIRE(page_state(base, MEM_RESERVE) && page_state(base + 3 * page, MEM_RESERVE));
    REQUIRE(!XMemFreePhysicalPages(count, tokens) && GetLastError() == ERROR_BUSY);
    for (UINT i = 0; i < page; ++i) {
        base[page + i] = (BYTE)(i ^ (i >> 8));
        base[2 * page + i] = (BYTE)~(i ^ (i >> 8));
    }
    REQUIRE(!XMemMapPhysicalPages(base, 2, NULL));
    REQUIRE(page_state(base + page, MEM_COMMIT));
    REQUIRE(XMemMapPhysicalPages(base + page, count, NULL) == base + page);
    REQUIRE(page_state(base + page, MEM_RESERVE) && page_state(base + page * 2, MEM_RESERVE));
    UINT64 reversed[] = {tokens[1], tokens[0]}, duplicate[] = {tokens[0], tokens[0]};
    UINT64 invalid[] = {tokens[0], UINT64_MAX};
    REQUIRE(!XMemFreePhysicalPages(2, invalid));
    REQUIRE(!XMemFreePhysicalPages(2, duplicate));
    REQUIRE(!XMemMapPhysicalPages(base, 2, duplicate) && page_state(base, MEM_RESERVE));
    REQUIRE(!XMemMapPhysicalPages(base + 1, 1, tokens));
    REQUIRE(XMemMapPhysicalPages(base, count, reversed) == base);
    for (UINT i = 0; i < page; ++i) {
        REQUIRE(base[i] == (BYTE)~(i ^ (i >> 8)));
        REQUIRE(base[page + i] == (BYTE)(i ^ (i >> 8)));
    }
    REQUIRE(XMemMapPhysicalPages(base, 1, NULL) == base);
    REQUIRE(XMemFreePhysicalPages(1, &reversed[0]));
    REQUIRE(!XMemMapPhysicalPages(base + 2 * page, 1, &reversed[0]));
    REQUIRE(!XMemFreePhysicalPages(1, &reversed[0]));
    REQUIRE(page_state(base + page, MEM_COMMIT));
    REQUIRE(XMemMapPhysicalPages(base + page, 1, NULL) == base + page);
    REQUIRE(XMemFreePhysicalPages(1, &reversed[1]));
    REQUIRE(VirtualAlloc(base, page, MEM_COMMIT, PAGE_READWRITE) == base);
    REQUIRE(!XMemMapPhysicalPages(base, 1, NULL) && page_state(base, MEM_COMMIT));
    REQUIRE(VirtualFree(base, 0, MEM_RELEASE) && page_state(base, MEM_FREE));
    REQUIRE(storage[0] == guard && storage[3] == guard);
    REQUIRE(!XMemAllocatePhysicalPages(0x20000000, &count, tokens) && !count && GetLastError() == ERROR_NOT_SUPPORTED);
    return 0;
}

int wmain(int argc, wchar_t **argv)
{
    BOOL pages_only = argc >= 2 && !wcscmp(argv[1], L"--pages");
    if (pages_only) { --argc; ++argv; }
    if (!(argc == 1 || (argc == 3 && wcscmp(argv[1], L"--report") == 0))) {
        fputs("usage: xmem-smoke.exe [--pages] [--report FILE]\n", stderr);
        return 2;
    }
    if (argc == 3 && _wfopen_s(&report, argv[2], L"wb") != 0) return 2;
    REQUIRE(app_local_provider());
    if (pages_only) {
        REQUIRE(!physical_pages());
        const char result[] = "xmem-pages PASS cpu-pages=2 remapped-bytes=131072 stale-and-foreign-rejected=1\n";
        if (report && (fputs(result, report) == EOF || fclose(report))) return 2;
        fputs(result, stdout);
        return 0;
    }
    REQUIRE(!working_set());
    REQUIRE(!virtual_reserve_commit());
    HMODULE module = GetModuleHandleW(L"xmem.dll");
    REQUIRE(GetProcAddress(module, "XMemAlloc") == GetProcAddress(module, MAKEINTRESOURCEA(1)));
    REQUIRE(GetProcAddress(module, "XMemFree") == GetProcAddress(module, MAKEINTRESOURCEA(6)));
    REQUIRE(GetProcAddress(module, "XMemAllocDefault") == GetProcAddress(module, MAKEINTRESOURCEA(2)));
    REQUIRE(GetProcAddress(module, "XMemFreeDefault") == GetProcAddress(module, MAKEINTRESOURCEA(7)));
    REQUIRE(!allocation_pair(XMemAlloc, XMemFree));
    REQUIRE(!allocation_pair(XMemAllocDefault, XMemFreeDefault));

    const char result[] = "xmem-default: PASS (allocator pairs 1/6 and 2/7, ordinary and 64 KiB allocations, host working set v2, virtual reserve/commit)\n";
    if (report) {
        int wrote = fputs(result, report);
        if (fclose(report) || wrote == EOF) return 2;
    }
    fputs(result, stdout);
    return 0;
}
