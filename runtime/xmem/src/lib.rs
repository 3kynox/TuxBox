//! CPU-backed Xbox default allocations. Callers retain ownership until the matching free.
mod physical_pages;
mod virtual_memory;

use core::{ffi::c_void, mem::size_of, ptr};

use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, SetLastError};
use windows_sys::Win32::System::Memory::{
    GetProcessHeap, HeapAlloc, HeapFree, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    VirtualAlloc, VirtualFree,
};
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const PLACEMENT: u64 = 0x3080_4000;

fn process_heap_attributes(attributes: u64) -> bool {
    // The two packaged D3D drivers use 0x92 for startup allocations, 0x40
    // for the blob object and 0x78 for its bytes. The packaged media decoders
    // allocate and free CPU storage with 0x04820000. All return 16-byte aligned memory.
    matches!(
        attributes,
        0x0480_009e | 0x0480_0092 | 0x0480_0040 | 0x0480_0078 | 0x0482_0000
    )
}

/// Allocate guest memory with the selected backing. A null return denotes failure.
#[unsafe(export_name = "XMemAllocDefault")]
pub extern "system" fn alloc_default(bytes: usize, attributes: u64) -> *mut c_void {
    if bytes == 0 {
        return ptr::null_mut();
    }
    match attributes {
        // SAFETY: The process heap is valid for this process; HeapAlloc checks the requested size.
        attr if process_heap_attributes(attr) => unsafe { HeapAlloc(GetProcessHeap(), 0, bytes) },
        // SAFETY: A null base lets Windows choose a fresh allocation-granularity-aligned region.
        PLACEMENT => unsafe {
            VirtualAlloc(ptr::null(), bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
        },
        _ => {
            // SAFETY: SetLastError accepts this Windows error code and only updates thread-local state.
            unsafe { SetLastError(ERROR_NOT_SUPPORTED) };
            ptr::null_mut()
        }
    }
}

/// Release a guest allocation using its original backing.
///
/// # Safety
/// For a supported attribute, a nonnull pointer must be the base of a live allocation
/// returned by `alloc_default` with that attribute. The caller must have exclusive
/// permission to free it, with no further use after release.
#[unsafe(export_name = "XMemFreeDefault")]
pub unsafe extern "system" fn free_default(memory: *mut c_void, attributes: u64) {
    if memory.is_null() {
        return;
    }
    match attributes {
        attr if process_heap_attributes(attr) => {
            // SAFETY: The caller supplies a live allocation from this process heap.
            unsafe { HeapFree(GetProcessHeap(), 0, memory) };
        }
        PLACEMENT => {
            // SAFETY: The caller supplies the reservation base; MEM_RELEASE requires size zero.
            unsafe { VirtualFree(memory, 0, MEM_RELEASE) };
        }
        // SAFETY: Recording an unsupported attribute does not access the caller's allocation.
        _ => unsafe { SetLastError(ERROR_NOT_SUPPORTED) },
    }
}

/// Return the host-backed title memory profile requested by Frosty (ordinal 16).
///
/// # Safety
/// With selector 0x20000001, a nonnull output must point to 120 writable bytes.
#[unsafe(export_name = "XMemGetWorkingSetStatistics")]
pub unsafe extern "system" fn working_set_statistics(selector: u32, out: *mut [u64; 15]) -> i32 {
    if selector != 0x2000_0001 || out.is_null() {
        let error = if out.is_null() {
            ERROR_INVALID_PARAMETER
        } else {
            ERROR_NOT_SUPPORTED
        };
        // SAFETY: This updates only the calling thread's error.
        unsafe { SetLastError(error) };
        return 0;
    }
    // SAFETY: The caller provides the complete v2 result. The console zeroes it before querying.
    unsafe { out.write_unaligned([0; 15]) };
    let mut memory = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: The initialized length describes the writable local buffer.
    if unsafe { GlobalMemoryStatusEx(&mut memory) } == 0 {
        return 0;
    }
    let cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    let mut process = PROCESS_MEMORY_COUNTERS_EX {
        cb,
        ..Default::default()
    };
    // SAFETY: This pseudo-handle identifies the calling process. The EX record has the
    // base record as its prefix; cb selects the extended size and private-commit counter.
    if unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut process).cast(), cb) } == 0 {
        return 0;
    }

    // Desktop has no dedicated Xbox partition. Limit additional headroom to both
    // host-available physical memory and commit available to this process.
    // Proton may only populate PagefileUsage. Windows defines both fields as the
    // process commit charge, including committed pages that are resident in RAM.
    let committed = if process.PrivateUsage != 0 {
        process.PrivateUsage
    } else {
        process.PagefileUsage
    };
    let used = (committed as u64).min(memory.ullTotalPhys);
    let headroom = memory.ullAvailPhys.min(memory.ullAvailPageFile);
    let budget = used.saturating_add(headroom).min(memory.ullTotalPhys);
    let mut result = [0; 15];
    result[0] = budget;
    result[1] = used;
    // The console repeats these fields for a single pool. Other console-only
    // counters stay zero on a host with no corresponding reported hardware.
    result[5] = budget;
    result[6] = used;
    result[11] = (process.PeakPagefileUsage as u64).max(used);
    // SAFETY: Same caller-provided result checked above. Publish the populated snapshot.
    unsafe { out.write_unaligned(result) };
    1
}
