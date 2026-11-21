use core::ffi::c_void;
use windows::Win32::Foundation::{E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::ID3D12Debug;
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_UNSUPPORTED;
use windows::core::{GUID, HRESULT, Interface};

// The Xbox debug interface has one flag-setting method. Its object stores a
// second table for QueryInterface, AddRef and Release at byte offset eight.
#[repr(C)]
struct DebugInterface {
    methods: &'static DebugMethods,
    unknown: &'static DebugUnknown,
}

#[repr(C)]
struct DebugMethods {
    set_flags: extern "system" fn(*const DebugInterface, u32) -> HRESULT,
}

#[repr(C)]
struct DebugUnknown {
    query:
        unsafe extern "system" fn(*const DebugInterface, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: extern "system" fn(*const DebugInterface) -> u32,
    release: extern "system" fn(*const DebugInterface) -> u32,
}

static DEBUG: DebugInterface = DebugInterface {
    methods: &DebugMethods { set_flags },
    unknown: &DebugUnknown {
        query: debug_query,
        add_ref,
        release,
    },
};

extern "system" fn set_flags(_: *const DebugInterface, flags: u32) -> HRESULT {
    crate::set_process_debug_flags(flags)
}

extern "system" fn add_ref(_: *const DebugInterface) -> u32 {
    1
}

extern "system" fn release(_: *const DebugInterface) -> u32 {
    0
}

unsafe extern "system" fn debug_query(
    _: *const DebugInterface,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: Both interfaces take the same IID and output pointer contract.
    unsafe { get_debug_interface(iid, out) }
}

/// Returns the process-wide Xbox debug object.
///
/// # Safety
/// The caller supplies a readable IID and a writable output pointer on success.
#[unsafe(export_name = "D3D12GetDebugInterface")]
pub unsafe extern "system" fn get_debug_interface(
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if iid.is_null() {
        return E_POINTER;
    }
    // SAFETY: The caller supplies a readable IID. The console leaves the
    // caller's output unchanged for every other interface, including IUnknown.
    if unsafe { *iid } != ID3D12Debug::IID {
        return E_NOINTERFACE;
    }
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: The output is writable. DEBUG and both tables live for the process.
    unsafe { out.write((&raw const DEBUG).cast_mut().cast()) };
    S_OK
}

// All four legacy DXGIX exports alias this result in both console DLLs.
#[unsafe(no_mangle)]
pub extern "system" fn tuxbox_dxgix_unsupported() -> HRESULT {
    DXGI_ERROR_UNSUPPORTED
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemoryRange {
    address: *mut u8,
    bytes: usize,
}

#[repr(C)]
struct ClearWorkspace {
    commands: *mut c_void,
    command_bytes: u32,
    scratch: *mut c_void,
    scratch_bytes: u32,
}

// In the Series DLL bit 0 selects a synchronous CPU clear. The default mode
// sets up a GPU operation, even when some ranges are cleared on the CPU.
#[unsafe(export_name = "D3D12ZeroMemoryX")]
unsafe extern "system" fn zero_memory(
    count: u32,
    ranges: *const MemoryRange,
    workspace: *const ClearWorkspace,
    flags: u32,
) -> HRESULT {
    if count == 0 {
        return S_OK;
    }
    if ranges.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: the guest supplies count readable records, separate from the
    // memory being cleared. Validate the entire batch before changing bytes.
    let ranges = unsafe { core::slice::from_raw_parts(ranges, count as usize) };
    for &MemoryRange { address, bytes } in ranges {
        if !address.addr().is_multiple_of(16)
            || !bytes.is_multiple_of(16)
            || (bytes != 0 && address.is_null())
            || bytes > isize::MAX as usize
            || address.addr().checked_add(bytes).is_none()
        {
            return E_INVALIDARG;
        }
    }
    // SAFETY: a non-null workspace points to the complete 32-byte Xbox record.
    if let Some(work) = unsafe { workspace.as_ref() }
        && (work.commands.is_null()
            || work.command_bytes < 0x10000
            || work.scratch.is_null()
            || work.scratch_bytes < 0x100)
    {
        return E_INVALIDARG;
    }
    if flags & 1 == 0 {
        return E_NOTIMPL;
    }
    for &MemoryRange { address, bytes } in ranges {
        if bytes != 0 {
            // SAFETY: CPU-mode ranges refer to writable guest memory of the
            // given length. Overlapping ranges are cleared in guest order.
            unsafe { address.write_bytes(0, bytes) };
        }
    }
    S_OK
}
