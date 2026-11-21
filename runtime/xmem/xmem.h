#ifndef PROJECT_TUXBOX_XMEM_H
#define PROJECT_TUXBOX_XMEM_H

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#define XMEM_ORDINARY_ATTRIBUTES 0x0480009eULL
#define XMEM_PLACEMENT_ATTRIBUTES 0x30804000ULL

__declspec(dllimport) void *WINAPI XMemAllocDefault(SIZE_T bytes, UINT64 attributes);
__declspec(dllimport) void WINAPI XMemFreeDefault(void *memory, UINT64 attributes);

// Selector used by the Frosty retail executable. The v2 result spans 120 bytes.
#define XMEM_TITLE_WORKING_SET_V2 0x20000001u
typedef struct XMEM_TITLE_STATISTICS_V2 {
    UINT64 physical_budget;
    UINT64 committed;
    BYTE reserved1[24];
    UINT64 pool_budget;
    UINT64 pool_committed;
    BYTE reserved2[32];
    UINT64 peak_committed;
    BYTE reserved3[24];
} XMEM_TITLE_STATISTICS_V2;
__declspec(dllimport) BOOL WINAPI XMemGetWorkingSetStatistics(UINT flags, XMEM_TITLE_STATISTICS_V2 *stats);

__declspec(dllimport) void *WINAPI XMemVirtualAlloc(void *address, SIZE_T bytes, DWORD allocation_type,
                                                  UINT64 profile, DWORD protection);

#endif
