//! Compute commands on the Xbox graphics command list.
use super::{List, parent, recording};
use crate::device;
use core::ffi::c_void;
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_CS_DISPATCH_MAX_THREAD_GROUPS_PER_DIMENSION, D3D12_RESOURCE_DIMENSION_BUFFER,
    ID3D12Resource,
};

pub(super) unsafe extern "system" fn bind_root(list: &List, signature: *mut c_void) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    // SAFETY: the guest holds its signature during this call. Only a native
    // interface belonging to this list's device is passed to the driver.
    match unsafe { device::compute::root_signature(signature, parent(&list.parent)) } {
        Ok(root) => unsafe { list.native.SetComputeRootSignature(root) },
        Err(error) => state.error = error,
    }
}

pub(super) unsafe extern "system" fn constants(
    list: &List,
    index: u32,
    count: u32,
    data: *const c_void,
    offset: u32,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) || count == 0 {
        return;
    }
    if data.is_null() {
        state.error = E_INVALIDARG;
        return;
    }
    // SAFETY: the caller supplies count readable DWORDs for this command.
    unsafe {
        list.native
            .SetComputeRoot32BitConstants(index, count, data, offset)
    };
}

pub(super) unsafe extern "system" fn unordered_access(list: &List, index: u32, gpu_address: u64) {
    let mut state = recording(list);
    if state.can_record(&list.native) {
        // SAFETY: native GPU addresses are shared with this device's guest resources.
        unsafe {
            list.native
                .SetComputeRootUnorderedAccessView(index, gpu_address)
        };
    }
}

pub(super) unsafe extern "system" fn dispatch(list: &List, x: u32, y: u32, z: u32) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if x.max(y).max(z) > D3D12_CS_DISPATCH_MAX_THREAD_GROUPS_PER_DIMENSION {
        state.error = E_INVALIDARG;
        return;
    }
    // SAFETY: the native list remains open under its recording lock.
    state.sample_heap();
    unsafe { list.native.Dispatch(x, y, z) };
}

pub(super) fn contains_range(resource: &ID3D12Resource, offset: u64, bytes: u64) -> bool {
    // SAFETY: the resource is borrowed from a live guest child on this device.
    let desc = unsafe { resource.GetDesc() };
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && offset
            .checked_add(bytes)
            .is_some_and(|end| end <= desc.Width)
}

pub(super) unsafe extern "system" fn copy_buffer(
    list: &List,
    dst: *mut c_void,
    dst_offset: u64,
    src: *mut c_void,
    src_offset: u64,
    bytes: u64,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    let owner = parent(&list.parent);
    // SAFETY: resource validates each guest proxy and its owning device.
    let (Some(dst), Some(src)) = (unsafe { device::resource(dst, owner) }, unsafe {
        device::resource(src, owner)
    }) else {
        state.error = E_INVALIDARG;
        return;
    };
    if !contains_range(dst, dst_offset, bytes) || !contains_range(src, src_offset, bytes) {
        state.error = E_INVALIDARG;
        return;
    }
    // SAFETY: both native buffers belong to the list and contain the requested range.
    unsafe {
        list.native
            .CopyBufferRegion(dst, dst_offset, src, src_offset, bytes)
    };
}
