//! CPU virtual memory requests observed in Frosty's binned allocator.
use core::{ffi::c_void, ptr};
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, HANDLE, SetLastError};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;
use windows_sys::Win32::System::Memory::{
    GetWriteWatch, MEM_COMMIT, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_NOACCESS, PAGE_READWRITE,
    PAGE_READONLY, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_GUARD, VirtualAlloc,
};

use windows_sys::Win32::System::SystemServices::{MEM_TOP_DOWN, MEM_WRITE_WATCH};

/// Console query result layout; its partition metadata is not yet supported.
#[repr(C)]
pub struct VirtualInfo {
    memory: MEMORY_BASIC_INFORMATION,
    console_flags: u64,
    console_tag: u64,
}

#[repr(C)]
pub struct XmemOption {
    id: u32,
    reserved: u32,
    value: u64,
}

#[derive(Default)]
struct WatchState {
    suspension_requested: bool,
    suspended: bool,
}

static WATCH_STATE: Mutex<WatchState> = Mutex::new(WatchState {
    suspension_requested: false,
    suspended: false,
});

/// Apply the observed write-watch suspension option. Other options need their
/// own console behavior and are rejected rather than silently acknowledged.
///
/// # Safety
/// `option` must be null or point to at least 12 readable bytes.
#[unsafe(export_name = "XMemSetOption")]
pub unsafe extern "system" fn set_option(option: *const XmemOption) {
    if option.is_null() {
        unsafe { SetLastError(ERROR_INVALID_PARAMETER) };
        return;
    }
    let id = unsafe { option.cast::<u32>().read_unaligned() };
    if id != 0x10000 {
        unsafe { SetLastError(ERROR_NOT_SUPPORTED) };
        return;
    }
    let value = unsafe { option.cast::<u8>().add(8).cast::<u32>().read_unaligned() };
    let mut state = WATCH_STATE.lock().unwrap_or_else(|e| e.into_inner());
    // The console reads the low 32 bits of the option payload.
    state.suspension_requested = value != 0;
    if state.suspension_requested {
        state.suspended = true;
    }
}

/// Resume write-watch queries after the suspension option is cleared.
#[unsafe(export_name = "XMemResumeWriteWatch")]
pub extern "system" fn resume_write_watch() -> i32 {
    let mut state = WATCH_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if state.suspension_requested {
        return 0;
    }
    state.suspended = false;
    1
}

/// Query dirty pages for a desktop write-watch allocation.
///
/// # Safety
/// The base must identify a write-watch reservation in this process. Output
/// pointers and their capacity must satisfy the Windows GetWriteWatch ABI.
#[unsafe(export_name = "XMemGetWriteWatch")]
pub unsafe extern "system" fn get_write_watch(
    flags: u32,
    base: *const c_void,
    bytes: usize,
    addresses: *mut *mut c_void,
    count: *mut usize,
    granularity: *mut u32,
) -> u32 {
    if WATCH_STATE.lock().unwrap_or_else(|e| e.into_inner()).suspended {
        // The console returns ERROR_CANTOPEN (1003) without modifying outputs.
        return 1003;
    }
    // The console forwards all six arguments to the host when not suspended.
    unsafe { GetWriteWatch(flags, base, bytes, addresses, count, granularity) }
}

/// Reject virtual-memory queries until their console metadata is supported.
///
/// # Safety
/// `information` must be null or point to 64 writable bytes.
#[unsafe(export_name = "XMemVirtualQuery")]
pub unsafe extern "system" fn virtual_query(
    _process: HANDLE,
    _address: *const c_void,
    information: *mut VirtualInfo,
) -> i32 {
    if information.is_null() {
        unsafe { SetLastError(ERROR_INVALID_PARAMETER) };
        return 0;
    }
    // The console augments State, RegionSize and the last 16 bytes with
    // partition, access and allocation-tag information. Reporting host-only
    // information as a successful query can mislead callers about a mapping.
    // Leave the complete output untouched until that metadata is available.
    unsafe { SetLastError(ERROR_NOT_SUPPORTED) };
    0
}

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
    let base_protect = protect & !PAGE_GUARD;
    let guard_allowed = protect & PAGE_GUARD == 0 || base_protect != PAGE_NOACCESS;
    let plain = guard_allowed && matches!(base_protect, PAGE_NOACCESS | PAGE_READONLY | PAGE_READWRITE |
        PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE)
        && (operation == MEM_RESERVE || operation == MEM_COMMIT || operation == MEM_RESERVE | MEM_COMMIT);
    let watched = (operation == MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH && protect == PAGE_READWRITE)
        || (operation == MEM_RESERVE | MEM_WRITE_WATCH && protect == PAGE_NOACCESS);
    if profile != 1 || !(plain || watched) {
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
