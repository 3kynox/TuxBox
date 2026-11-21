//! Guest COM identity and ownership. The desktop object never escapes the proxy.
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};
use std::alloc::{Layout, alloc};
use windows::Win32::Foundation::{E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::ID3D12Object;
use windows::Win32::System::Diagnostics::Debug::{OutputDebugStringA, RaiseFailFastException};
use windows::core::{GUID, HRESULT, IUnknown, Interface, PCWSTR, s};

pub(crate) type Slot = unsafe extern "system" fn();
pub(crate) type Slots<const L: usize> = [Slot; L];

// Only the vtable pointer is public ABI. Drop the queue's presentation state
// (which may wait on pending GPU work) before releasing its native and parent.
#[repr(C)]
pub(crate) struct Proxy<N, S = ()> {
    vtable: *const Slot,
    refs: AtomicU32,
    interfaces: &'static [GUID],
    pub(crate) state: S,
    pub(crate) native: N,
    pub(crate) parent: Option<IUnknown>,
}

// Unmapped Xbox slots can return structures or use private calling conventions.
// No single return value is valid for all of them.
pub(crate) unsafe extern "system" fn unsupported_slot() -> ! {
    unsafe {
        OutputDebugStringA(s!("Project TuxBox: unsupported D3D12.x vtable slot\n"));
        RaiseFailFastException(None, None, 0);
    }
    std::process::abort()
}

/// The caller must match the slot table to N and S, and supply a writable output.
pub(crate) unsafe fn allocate<N, S>(
    table: &'static [Slot],
    interfaces: &'static [GUID],
    native: N,
    parent: Option<&IUnknown>,
    state: S,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // Fallible allocation is part of the COM contract. The final Release
    // reclaims this allocation with Box, using the same global allocator.
    let object = unsafe { alloc(Layout::new::<Proxy<N, S>>()).cast::<Proxy<N, S>>() };
    if object.is_null() {
        unsafe { out.write(ptr::null_mut()) };
        return E_OUTOFMEMORY;
    }
    unsafe {
        object.write(Proxy {
            vtable: table.as_ptr(),
            refs: AtomicU32::new(1),
            interfaces,
            state,
            native,
            parent: parent.cloned(),
        });
        out.write(object.cast());
    }
    S_OK
}

/// A non-null object must be a live COM interface for the duration of the borrow.
/// The table belongs to the exact N and S used at allocation.
pub(crate) unsafe fn unwrap<'a, N, S>(
    object: *mut c_void,
    device: *mut c_void,
    table: &'static [Slot],
) -> Option<&'a Proxy<N, S>> {
    if object.is_null() || device.is_null() {
        return None;
    }
    // Foreign COM objects only guarantee that first word. Check identity
    // before inspecting any other field as a TuxBox proxy.
    if unsafe { object.cast::<*const Slot>().read_unaligned() } != table.as_ptr() {
        return None;
    }
    let proxy = unsafe { &*object.cast::<Proxy<N, S>>() };
    (proxy.parent.as_ref()?.as_raw() == device).then_some(proxy)
}

pub(crate) unsafe fn check_output<N: Interface>(
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { out.write(ptr::null_mut()) };
    if iid.is_null() {
        return E_POINTER;
    }
    if unsafe { iid.read_unaligned() } == N::IID {
        S_OK
    } else {
        E_NOINTERFACE
    }
}

unsafe extern "system" fn add_ref<N, S>(this: *mut Proxy<N, S>) -> u32 {
    unsafe { (*this).refs.fetch_add(1, Ordering::Relaxed) + 1 }
}

unsafe extern "system" fn release<N, S>(this: *mut Proxy<N, S>) -> u32 {
    let refs = unsafe { (*this).refs.fetch_sub(1, Ordering::AcqRel) } - 1;
    if refs == 0 {
        // Only the thread that dropped the last reference can destroy the proxy.
        drop(unsafe { Box::from_raw(this) });
    }
    refs
}

unsafe extern "system" fn query<N, S>(
    this: *mut Proxy<N, S>,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { out.write(ptr::null_mut()) };
    if iid.is_null() {
        return E_POINTER;
    }
    let iid = unsafe { iid.read_unaligned() };
    if iid != IUnknown::IID
        && iid != ID3D12Object::IID
        && !unsafe { (*this).interfaces }.contains(&iid)
    {
        return E_NOINTERFACE;
    }
    unsafe {
        add_ref(this);
        out.write(this.cast());
    }
    S_OK
}

// Each desktop interface used here derives from ID3D12Object. Use its typed
// vtable to preserve raw HRESULTs and caller-owned buffers exactly.
unsafe extern "system" fn get_private<N, S>(
    this: *mut Proxy<N, S>,
    key: *const GUID,
    size: *mut u32,
    data: *mut c_void,
) -> HRESULT
where
    for<'a> &'a N: Into<&'a ID3D12Object>,
{
    let native = unsafe { &(*this).native }.into();
    unsafe { (native.vtable().GetPrivateData)(native.as_raw(), key, size, data) }
}

unsafe extern "system" fn set_private<N, S>(
    this: *mut Proxy<N, S>,
    key: *const GUID,
    size: u32,
    data: *const c_void,
) -> HRESULT
where
    for<'a> &'a N: Into<&'a ID3D12Object>,
{
    let native = unsafe { &(*this).native }.into();
    unsafe { (native.vtable().SetPrivateData)(native.as_raw(), key, size, data) }
}

unsafe extern "system" fn set_interface<N, S>(
    this: *mut Proxy<N, S>,
    key: *const GUID,
    data: *mut c_void,
) -> HRESULT
where
    for<'a> &'a N: Into<&'a ID3D12Object>,
{
    let native = unsafe { &(*this).native }.into();
    unsafe { (native.vtable().SetPrivateDataInterface)(native.as_raw(), key, data) }
}

unsafe extern "system" fn set_name<N, S>(this: *mut Proxy<N, S>, name: PCWSTR) -> HRESULT
where
    for<'a> &'a N: Into<&'a ID3D12Object>,
{
    let native = unsafe { &(*this).native }.into();
    unsafe { (native.vtable().SetName)(native.as_raw(), name) }
}

unsafe extern "system" fn get_device<N, S>(
    this: *mut Proxy<N, S>,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // Only child tables carry this slot. Query the guest's parent, retaining
    // its identity across device and child calls.
    let Some(parent) = (unsafe { &(*this).parent }) else {
        return E_POINTER;
    };
    unsafe { parent.query(iid, out) }
}

pub(crate) const fn object_slots<N, S, const L: usize>() -> Slots<L>
where
    for<'a> &'a N: Into<&'a ID3D12Object>,
{
    let mut slots = [crate::slot!(unsupported_slot); L];
    slots[0] = crate::slot!(query::<N, S>);
    slots[1] = crate::slot!(add_ref::<N, S>);
    slots[2] = crate::slot!(release::<N, S>);
    slots[3] = crate::slot!(get_private::<N, S>);
    slots[4] = crate::slot!(set_private::<N, S>);
    slots[5] = crate::slot!(set_interface::<N, S>);
    slots[6] = crate::slot!(set_name::<N, S>);
    slots
}

pub(crate) const fn child_slots<N, S, const L: usize>() -> Slots<L>
where
    for<'a> &'a N: Into<&'a ID3D12Object>,
{
    let mut slots = object_slots::<N, S, L>();
    slots[7] = crate::slot!(get_device::<N, S>);
    slots
}
