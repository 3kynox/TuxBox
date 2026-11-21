//! CPU virtual memory requests observed in Frosty's binned allocator.
use core::{ffi::c_void, ptr};
use windows_sys::Win32::Foundation::{ERROR_NOT_SUPPORTED, SetLastError};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc,
};

use windows_sys::Win32::System::SystemServices::MEM_TOP_DOWN;

/// Reserve or commit CPU memory for the binned allocator and physical-page apertures.
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
    // Frosty's physical-page pool reserves an aperture, then backs individual
    // 64 KiB pages as needed. The D3D12 placement uploader reads these CPU pages.
    if profile == 3 {
        let host = match (allocation_type, protect) {
            (0x2040_2000, 0x2404 | 0x10404) => Some((MEM_RESERVE, PAGE_NOACCESS)),
            (0x2040_1000, 0x2404 | 0x10404) if !address.is_null() => {
                Some((MEM_COMMIT, PAGE_READWRITE))
            }
            _ => None,
        };
        if let Some((kind, protection)) = host {
            return unsafe { VirtualAlloc(address, bytes, kind, protection) };
        }
    }
    let operation = allocation_type & !MEM_TOP_DOWN;
    if profile != 1
        || !matches!(
            (operation, protect),
            (MEM_RESERVE, PAGE_NOACCESS) | (MEM_COMMIT, PAGE_READWRITE)
        )
    {
        let description = format!(
            "TuxBox: unsupported XMemVirtualAlloc address={address:p} bytes={bytes} type={allocation_type:#x} profile={profile:#x} protect={protect:#x}\0"
        );
        unsafe { OutputDebugStringA(description.as_ptr()) };
        // SAFETY: The error code is written to the current thread.
        unsafe { SetLastError(ERROR_NOT_SUPPORTED) };
        return ptr::null_mut();
    }

    // SAFETY: The caller supplies the address range. Windows validates the
    // reservation, rounds to host page boundaries, and sets the last error on failure.
    unsafe { VirtualAlloc(address, bytes, allocation_type, protect) }
}
