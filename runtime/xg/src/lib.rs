//! The x64 texture address computer used by XGCreateTextureComputer.

use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};
use std::alloc::{Layout as Allocation, alloc};
use tuxbox_xg_layout::{Desc, Layout, Plane};
use windows_sys::Win32::Foundation::{E_INVALIDARG, E_OUTOFMEMORY, E_POINTER, S_OK};

#[repr(C)]
pub struct Computer {
    methods: &'static Methods,
    refs: AtomicU32,
    plane: Plane,
}

type Unsupported = unsafe extern "system" fn() -> !;

#[repr(C)]
struct Methods {
    add_ref: unsafe extern "system" fn(*mut Computer) -> u32,
    release: unsafe extern "system" fn(*mut Computer) -> u32,
    layout: unsafe extern "system" fn(&Computer, *mut Layout) -> i32,
    unused: [Unsupported; 3],
    offset: unsafe extern "system" fn(&Computer, u32, u32, u64, u32, u32, u32, *mut u32) -> u64,
    other: [Unsupported; 9],
}

unsafe extern "system" fn unsupported() -> ! {
    std::process::abort()
}

static METHODS: Methods = Methods {
    add_ref,
    release,
    layout: get_layout,
    unused: [unsupported; 3],
    offset: element_offset,
    other: [unsupported; 9],
};

unsafe extern "system" fn add_ref(this: *mut Computer) -> u32 {
    unsafe { (*this).refs.fetch_add(1, Ordering::Relaxed) + 1 }
}

unsafe extern "system" fn release(this: *mut Computer) -> u32 {
    let left = unsafe { (*this).refs.fetch_sub(1, Ordering::AcqRel) } - 1;
    if left == 0 {
        // The final owner reclaims the global allocation made by the factory.
        drop(unsafe { Box::from_raw(this) });
    }
    left
}

unsafe extern "system" fn get_layout(this: &Computer, out: *mut Layout) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    // The guest supplies one complete layout. Inactive planes and reserved
    // fields have a defined zero representation, irrespective of prior contents.
    unsafe {
        out.write_bytes(0, 1);
        (*out).bytes = this.plane.size;
        (*out).alignment = this.plane.alignment;
        (*out).mips = this.plane.mip_count();
        (*out).plane_count = 1;
        (*out).planes[0] = this.plane;
        (*out).dimension = 3;
    }
    S_OK
}

unsafe extern "system" fn element_offset(
    this: &Computer,
    plane: u32,
    mip: u32,
    x: u64,
    y: u32,
    slice: u32,
    sample: u32,
    bit: *mut u32,
) -> u64 {
    if !bit.is_null() {
        // All supported formats address whole bytes.
        unsafe { bit.write(0) };
    }
    if plane != 0 || sample != 0 {
        return u64::MAX;
    }
    this.plane
        .element_offset_in_slice(slice, mip, x, y)
        .unwrap_or(u64::MAX)
}

/// Returns an owned texture computer for a supported single-plane descriptor.
///
/// # Safety
/// Non-null inputs must describe a readable x64 resource descriptor and a
/// disjoint writable output pointer. Release the returned object through slot 1.
#[unsafe(export_name = "XGCreateTextureComputer")]
unsafe extern "system" fn create(desc: *const Desc, out: *mut *mut Computer) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { out.write(ptr::null_mut()) };
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_INVALIDARG;
    };
    let plane = match Plane::compute(desc) {
        Ok(p) => p,
        Err(hr) => return hr,
    };
    let object = unsafe { alloc(Allocation::new::<Computer>()).cast::<Computer>() };
    if object.is_null() {
        return E_OUTOFMEMORY;
    }
    unsafe {
        object.write(Computer {
            methods: &METHODS,
            refs: AtomicU32::new(1),
            plane,
        });
        out.write(object);
    }
    S_OK
}
