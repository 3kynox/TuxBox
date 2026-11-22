//! Standalone immutable GPU shadow and fence-guarded retirement control.
//! Not linked into TuxBox or exposed as a title-facing API.
#![allow(dead_code)]
#[path = "../../../blas/validate_traversal.rs"]
mod validate_traversal;

use core::{ffi::c_void, mem::ManuallyDrop, ptr, slice};
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::DXGI_SAMPLE_DESC};
use windows::core::Interface;

struct Shadow {
    buffer: ID3D12Resource,
    size: u64,
    fences: Vec<(ID3D12Fence, u64)>,
}

// The control harness serializes access to an opaque handle. Device, fence,
// input span, and output pointer must be valid for each call. It must register
// every GPU use with a private fence before submission and retry release until
// all holds have completed; an untracked use is outside this control's scope.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tb_shadow_create(
    device: *mut c_void,
    data: *const u8,
    len: usize,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() {
        return -1;
    }
    unsafe { out.write(ptr::null_mut()) };
    if device.is_null() || data.is_null() || len > 64 * 1024 * 1024 || len < 128 {
        return -1;
    }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(len).is_err() {
        return -3;
    }
    bytes.extend_from_slice(unsafe { slice::from_raw_parts(data, len) });
    if validate_traversal::validate(&bytes).is_err() {
        return -2;
    }
    let body = &bytes[64..];
    let native = ManuallyDrop::new(unsafe { ID3D12Device::from_raw(device) });
    let props = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_UPLOAD,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: body.len() as u64,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut buffer = None;
    if unsafe {
        native.CreateCommittedResource(
            &props,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut buffer,
        )
    }
    .is_err()
    {
        return -3;
    }
    let Some(buffer): Option<ID3D12Resource> = buffer else {
        return -3;
    };
    let mut mapped = ptr::null_mut();
    if unsafe { buffer.GetGPUVirtualAddress() } == 0 {
        return -3;
    }
    if unsafe { buffer.Map(0, Some(&D3D12_RANGE::default()), Some(&mut mapped)) }.is_err() {
        return -4;
    }
    if mapped.is_null() {
        unsafe { buffer.Unmap(0, Some(&D3D12_RANGE::default())) };
        return -4;
    }
    unsafe {
        ptr::copy_nonoverlapping(body.as_ptr(), mapped.cast::<u8>(), body.len());
        buffer.Unmap(
            0,
            Some(&D3D12_RANGE {
                Begin: 0,
                End: body.len(),
            }),
        );
    }
    let result = Box::new(Shadow {
        buffer,
        size: body.len() as u64,
        fences: Vec::new(),
    });
    unsafe {
        out.write(Box::into_raw(result).cast());
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tb_shadow_info(
    handle: *const c_void,
    address: *mut u64,
    size: *mut u64,
) -> i32 {
    if handle.is_null() || address.is_null() || size.is_null() {
        return -1;
    }
    let shadow = unsafe { &*handle.cast::<Shadow>() };
    unsafe {
        address.write(shadow.buffer.GetGPUVirtualAddress());
        size.write(shadow.size);
    }
    0
}

// Register before queue execution. Handle operations and submission must be
// externally serialized. The caller must use private, monotonic completion
// fences whose values cannot be signaled or rewound by the guest.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tb_shadow_hold(
    handle: *mut c_void,
    fence: *mut c_void,
    value: u64,
) -> i32 {
    if handle.is_null() || fence.is_null() || value == 0 || value == u64::MAX {
        return -1;
    }
    let shadow = unsafe { &mut *handle.cast::<Shadow>() };
    if shadow.fences.try_reserve(1).is_err() {
        return -2;
    }
    let fence = ManuallyDrop::new(unsafe { ID3D12Fence::from_raw(fence) });
    shadow.fences.push(((*fence).clone(), value));
    0
}

// On incomplete or device-removed fences keep ownership and report pending.
// A successful result consumes the handle; it must not be used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tb_shadow_try_release(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let shadow = unsafe { &*handle.cast::<Shadow>() };
    if shadow.fences.iter().any(|(fence, value)| {
        let completed = unsafe { fence.GetCompletedValue() };
        completed == u64::MAX || completed < *value
    }) {
        return 0;
    }
    unsafe {
        drop(Box::from_raw(handle.cast::<Shadow>()));
    }
    1
}
