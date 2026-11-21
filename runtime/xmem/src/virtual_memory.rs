//! CPU virtual memory requests observed in Frosty's binned allocator.
use core::{ffi::c_void, ptr};
use windows_sys::Win32::Foundation::{ERROR_NOT_SUPPORTED, SetLastError};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc,
};

use windows_sys::Win32::System::SystemServices::MEM_TOP_DOWN;

/// Reserve address space or commit CPU-readable/writable pages for profile 1.
///
/// # Safety
/// A nonnull address must identify a range the caller is entitled to reserve or
/// commit in this process. The caller owns successful reservations and releases
/// them through VirtualFree.
#[unsafe(export_name = "XMemVirtualAlloc")]
pub unsafe extern "system" fn virtual_alloc(
    address: *mut c_void,
    bytes: usize,
    allocation_type: u32,
    profile: u64,
    protect: u32,
) -> *mut c_void {
    // Frosty's BinnedAllocFromOS passes protection in its fifth argument.
    // Large allocations in that routine also request top-down placement.
    let operation = allocation_type & !MEM_TOP_DOWN;
    if profile != 1
        || !matches!(
            (operation, protect),
            (MEM_RESERVE, PAGE_NOACCESS) | (MEM_COMMIT, PAGE_READWRITE)
        )
    {
        // SAFETY: The error code is written to the current thread.
        unsafe { SetLastError(ERROR_NOT_SUPPORTED) };
        return ptr::null_mut();
    }

    // SAFETY: The caller supplies the address range. Windows validates the
    // reservation, rounds to host page boundaries, and sets the last error on failure.
    unsafe { VirtualAlloc(address, bytes, allocation_type, protect) }
}
