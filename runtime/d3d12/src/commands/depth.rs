//! Hand off the float depth plane between depth testing and unordered access
//! without moving or reinterpreting stencil. All commands use the guest list.
use super::{CopyLocation, List, TransitionBarrier, parent, recording};
use crate::device::{depth::Pair, resource_proxy};
use core::{ffi::c_void, ptr};
use windows::Win32::Foundation::{E_INVALIDARG, RECT};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Interface;

pub(super) unsafe extern "system" fn clear(
    list: &List,
    handle: D3D12_CPU_DESCRIPTOR_HANDLE,
    flags: D3D12_CLEAR_FLAGS,
    depth: f32,
    stencil: u8,
    count: u32,
    rects: *const RECT,
) {
    let mut recording = recording(list);
    if !recording.can_record(&list.native) {
        return;
    }
    if handle.ptr == 0
        || flags.0 == 0
        || flags.0 & !(D3D12_CLEAR_FLAG_DEPTH | D3D12_CLEAR_FLAG_STENCIL).0 != 0
        || (count != 0 && rects.is_null())
    {
        recording.error = E_INVALIDARG;
        return;
    }
    unsafe {
        (list.native.vtable().ClearDepthStencilView)(
            list.native.as_raw(),
            handle,
            flags,
            depth,
            stencil,
            count,
            rects,
        )
    };
}

pub(super) unsafe extern "system" fn clear_uav<const FLOAT: bool>(
    list: &List,
    gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
    cpu: D3D12_CPU_DESCRIPTOR_HANDLE,
    resource: *mut c_void,
    values: *const u32,
    count: u32,
    rects: *const RECT,
) {
    let mut recording = recording(list);
    if !recording.can_record(&list.native) {
        return;
    }
    let Some(resource) = (unsafe { resource_proxy(resource, parent(&list.parent)) }) else {
        recording.error = E_INVALIDARG;
        return;
    };
    if gpu.ptr == 0 || cpu.ptr == 0 || values.is_null() || (count != 0 && rects.is_null()) {
        recording.error = E_INVALIDARG;
        return;
    }
    let target = resource
        .state
        .depth()
        .map_or(&resource.native, |pair| &pair.image)
        .as_raw();
    let native = &list.native;
    unsafe {
        if FLOAT {
            (native.vtable().ClearUnorderedAccessViewFloat)(
                native.as_raw(),
                gpu,
                cpu,
                target,
                values.cast(),
                count,
                rects,
            )
        } else {
            (native.vtable().ClearUnorderedAccessViewUint)(
                native.as_raw(),
                gpu,
                cpu,
                target,
                values,
                count,
                rects,
            )
        }
    }
}

pub(super) unsafe fn handoff(
    list: &ID3D12GraphicsCommandList,
    depth: &ID3D12Resource,
    pair: &Pair,
    barrier: &TransitionBarrier,
) {
    let transition = |resource: &ID3D12Resource, subresource, before, after| {
        if before != after {
            let host = TransitionBarrier {
                kind: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                resource: resource.as_raw(),
                subresource,
                before,
                after,
            };
            unsafe {
                (list.vtable().ResourceBarrier)(list.as_raw(), 1, ptr::from_ref(&host).cast())
            };
        }
    };
    let copy = |image: &ID3D12Resource, footprint, to_buffer| {
        let texture = CopyLocation {
            resource: image.as_raw(),
            kind: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            data: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        let buffer = CopyLocation {
            resource: pair.transfer.as_raw(),
            kind: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            data: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: footprint,
            },
        };
        let (dst, src) = if to_buffer {
            (&buffer, &texture)
        } else {
            (&texture, &buffer)
        };
        unsafe {
            (list.vtable().CopyTextureRegion)(
                list.as_raw(),
                ptr::from_ref(dst).cast(),
                0,
                0,
                0,
                ptr::from_ref(src).cast(),
                ptr::null(),
            )
        };
    };

    let uav = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    let src = D3D12_RESOURCE_STATE_COPY_SOURCE;
    let dst = D3D12_RESOURCE_STATE_COPY_DEST;
    let entering = barrier.after == uav;
    if barrier.subresource != 0 {
        // There is no unordered stencil view. Park that hardware plane in
        // COMMON until the logical resource leaves unordered access.
        let common = D3D12_RESOURCE_STATE_COMMON;
        let (before, after) = if entering {
            (barrier.before, common)
        } else {
            (common, barrier.after)
        };
        transition(depth, 1, before, after);
        if barrier.subresource == 1 {
            return;
        }
    }
    if entering {
        transition(depth, 0, barrier.before, src);
        transition(&pair.image, 0, uav, dst);
        copy(depth, pair.depth_plane, true);
    } else {
        transition(&pair.image, 0, uav, src);
        transition(depth, 0, src, dst);
        copy(&pair.image, pair.image_plane, true);
    }
    transition(&pair.transfer, 0, dst, src);
    if entering {
        copy(&pair.image, pair.image_plane, false);
        transition(&pair.image, 0, dst, uav);
    } else {
        copy(depth, pair.depth_plane, false);
        transition(&pair.image, 0, src, uav);
        transition(depth, 0, dst, barrier.after);
    }
    transition(&pair.transfer, 0, src, dst);
}
