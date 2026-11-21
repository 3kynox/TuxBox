use core::ffi::c_void;
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
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
