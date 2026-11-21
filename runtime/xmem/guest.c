#include "xmem.h"
#include <stdio.h>
#include <stddef.h>
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
    // The console interprets this flag pair as a larger page mode.
    REQUIRE(!XMemVirtualAlloc(NULL, page, MEM_COMMIT | 0x20400000u, 1, PAGE_READWRITE) &&
            GetLastError() == ERROR_NOT_SUPPORTED);
    return 0;
}

int wmain(int argc, wchar_t **argv)
{
    if (!(argc == 1 || (argc == 3 && wcscmp(argv[1], L"--report") == 0))) {
        fputs("usage: xmem-smoke.exe [--report FILE]\n", stderr);
        return 2;
    }
    if (argc == 3 && _wfopen_s(&report, argv[2], L"wb") != 0) return 2;
    REQUIRE(app_local_provider());
    REQUIRE(!working_set());
    REQUIRE(!virtual_reserve_commit());
    enum { small_size = 12, placed_size = 65536 };
    volatile BYTE *small = XMemAllocDefault(small_size, XMEM_ORDINARY_ATTRIBUTES);
    volatile BYTE *placed = XMemAllocDefault(placed_size, XMEM_PLACEMENT_ATTRIBUTES);
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

    XMemFreeDefault((void *)small, XMEM_ORDINARY_ATTRIBUTES);
    REQUIRE(!HeapValidate(GetProcessHeap(), 0, (void *)small));
    XMemFreeDefault((void *)placed, XMEM_PLACEMENT_ATTRIBUTES);
    REQUIRE(VirtualQuery((void *)placed, &area, sizeof area) == sizeof area && area.State == MEM_FREE);
    REQUIRE(!XMemAllocDefault(0, XMEM_ORDINARY_ATTRIBUTES));
    REQUIRE(!XMemAllocDefault(1, 0) && GetLastError() == ERROR_NOT_SUPPORTED);
    XMemFreeDefault(NULL, XMEM_ORDINARY_ATTRIBUTES);

    const char result[] = "xmem-default: PASS (ordinary and 64 KiB allocations, host working set v2, virtual reserve/commit)\n";
    if (report) {
        int wrote = fputs(result, report);
        if (fclose(report) || wrote == EOF) return 2;
    }
    fputs(result, stdout);
    return 0;
}
