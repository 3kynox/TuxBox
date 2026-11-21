//! Xbox graphics-list query slots 28–30 and queue clock slots 16–17.
use super::{List, Queue, parent, recording};
use crate::device::{queries::range, resource};
use core::ffi::c_void;
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{HRESULT, Interface};

pub(super) unsafe extern "system" fn query<const BEGIN: bool>(
    list: &List,
    object: *mut c_void,
    kind: D3D12_QUERY_TYPE,
    index: u32,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    let heap = unsafe { range(object, parent(&list.parent), kind, index, 1) };
    let Some((heap, _)) = heap else {
        state.error = E_INVALIDARG;
        return;
    };
    if BEGIN {
        if kind == D3D12_QUERY_TYPE_TIMESTAMP {
            state.error = E_INVALIDARG;
            return;
        }
        unsafe { list.native.BeginQuery(heap, kind, index) };
    } else {
        unsafe { list.native.EndQuery(heap, kind, index) };
    }
}

pub(super) unsafe extern "system" fn resolve(
    list: &List,
    object: *mut c_void,
    kind: D3D12_QUERY_TYPE,
    start: u32,
    count: u32,
    destination: *mut c_void,
    offset: u64,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    let owner = parent(&list.parent);
    let heap = unsafe { range(object, owner, kind, start, count) };
    let dst = unsafe { resource(destination, owner) };
    let (Some((heap, stride)), Some(dst)) = (heap, dst) else {
        state.error = E_INVALIDARG;
        return;
    };
    let desc = unsafe { dst.GetDesc() };
    if !offset.is_multiple_of(8)
        || desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
        || offset
            .checked_add(u64::from(count) * stride)
            .is_none_or(|end| end > desc.Width)
    {
        state.error = E_INVALIDARG;
        return;
    }
    unsafe {
        list.native
            .ResolveQueryData(heap, kind, start, count, dst, offset)
    };
}

pub(super) unsafe extern "system" fn frequency(queue: &Queue, hz: *mut u64) -> HRESULT {
    if hz.is_null() {
        return E_INVALIDARG;
    }
    let host = &queue.native;
    unsafe { (host.vtable().GetTimestampFrequency)(host.as_raw(), hz) }
}

pub(super) unsafe extern "system" fn calibrate(
    queue: &Queue,
    gpu: *mut u64,
    cpu: *mut u64,
) -> HRESULT {
    if gpu.is_null() || cpu.is_null() {
        return E_INVALIDARG;
    }
    let host = &queue.native;
    unsafe { (host.vtable().GetClockCalibration)(host.as_raw(), gpu, cpu) }
}
