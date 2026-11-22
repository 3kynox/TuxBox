//! Descriptor heap binding and compute/graphics root tables in the Xbox list.
use super::{List, parent, recording};
use crate::device;
use core::{ffi::c_void, ptr, slice};
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Interface;

pub(super) unsafe extern "system" fn bind_heaps(
    list: &List,
    count: u32,
    heaps: *const *mut c_void,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if count > 2 || (count != 0 && heaps.is_null()) {
        state.error = E_INVALIDARG;
        return;
    }
    let guest = if count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(heaps, count as usize) }
    };
    let mut native = [ptr::null_mut(); 2];
    let mut retained = [None, None];
    let mut previous = None;
    let mut resource_heap = None;
    for (index, &object) in guest.iter().enumerate() {
        let Some(heap) = (unsafe { device::descriptor_heap(object, parent(&list.parent)) }) else {
            state.error = E_INVALIDARG;
            return;
        };
        let desc = &heap.state.desc;
        if desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE || previous == Some(desc.Type) {
            state.error = E_INVALIDARG;
            return;
        }
        if let Some(views) = &heap.state.placed_views {
            resource_heap = Some(views.range.clone());
        }
        previous = Some(desc.Type);
        native[index] = heap.native.as_raw();
        retained[index] = Some(heap.native.clone());
    }
    state.resource_heap = resource_heap;
    state.bindings.heaps(&retained[..count as usize]);
    unsafe {
        (list.native.vtable().SetDescriptorHeaps)(list.native.as_raw(), count, native.as_ptr())
    };
}

pub(super) unsafe extern "system" fn table<const COMPUTE: bool>(
    list: &List,
    index: u32,
    base: D3D12_GPU_DESCRIPTOR_HANDLE,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    state.bindings.table::<COMPUTE>(index, base);
    if COMPUTE {
        unsafe { list.native.SetComputeRootDescriptorTable(index, base) };
    } else {
        unsafe { list.native.SetGraphicsRootDescriptorTable(index, base) };
    }
}
