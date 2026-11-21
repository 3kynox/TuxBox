//! Guest device-factory ABI for d3d12_xs.dll.

mod commands;
mod device;
mod present;
mod proxy;

// Guest vtables mix signatures. Each entry is called by its x64 ABI slot.
macro_rules! slot {
    ($method:expr) => {
        unsafe { core::mem::transmute::<*const (), $crate::proxy::Slot>($method as *const ()) }
    };
}
pub(crate) use slot;

use core::ffi::c_void;
use core::ptr;
use windows::Win32::Foundation::{E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL, E_POINTER};
use windows::Win32::Graphics::Direct3D12::ID3D12Device;
use windows::core::{GUID, HRESULT, Interface};

/// Data export imported by retail titles. Windows GUID fields use native
/// layout, including little-endian storage of the first three integers.
#[unsafe(export_name = "IID_ID3D12Device")]
pub static DEVICE_IID: GUID = ID3D12Device::IID;

// Release/default values of the two dynamic fields at the audited Fortnite
// call site. No other version or flag combination has been exercised.
const PROFILE: [u32; 16] = [
    0x20002,
    0,
    u32::MAX,
    0,
    0,
    0,
    u32::MAX,
    u32::MAX,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
];

/// Creates the supported guest device, returning one owned COM reference.
///
/// # Safety
/// A non-null adapter must be a live DXGI adapter. Non-null IID and output
/// pointers must denote a readable GUID and writable pointer respectively.
/// A non-null parameter record must have a readable version DWORD. Version
/// 0x20002 requires 64 readable bytes. Inputs must remain valid throughout
/// the call and must not overlap the output pointer.
#[unsafe(export_name = "D3D12XboxCreateDevice")]
pub unsafe extern "system" fn create_device(
    adapter: *mut c_void,
    params: *const c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: The caller supplies a writable output pointer. Clear it even
    // when subsequent interface negotiation or parameter validation fails.
    unsafe { out.write(ptr::null_mut()) };
    if iid.is_null() {
        return E_POINTER;
    }
    // SAFETY: The caller supplies a readable GUID. Compare Windows GUID fields
    // without casting guest storage to an aligned Rust reference.
    let requested = unsafe { iid.read_unaligned() };
    if requested != DEVICE_IID {
        return E_NOINTERFACE;
    }
    if params.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: Only the version DWORD is guaranteed readable for unknown records.
    if unsafe { params.cast::<u32>().read_unaligned() } != PROFILE[0] {
        return E_NOTIMPL;
    }
    // SAFETY: This version defines a 16-DWORD record, which need not be aligned.
    if unsafe { params.cast::<[u32; 16]>().read_unaligned() } != PROFILE {
        return E_NOTIMPL;
    }
    // SAFETY: The host helper accepts a null or borrowed adapter and writes
    // a single owned guest device reference to the caller's cleared output.
    unsafe { device::create(adapter, out) }
}
