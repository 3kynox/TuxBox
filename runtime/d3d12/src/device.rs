//! Xbox device and pageable-object slots for the x64 guest ABI.
//!
//! Each entry borrows its receiver for the duration of the COM call. The guest
//! holds live references to its arguments and provides storage for output
//! pointers. Only the guest device identity is handed back to child objects.

use core::ffi::c_void;
use core::ptr::NonNull;
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, E_POINTER, HANDLE, LUID};
use windows::Win32::Graphics::{Direct3D::D3D_FEATURE_LEVEL_12_0, Direct3D12::*};
use windows::core::{GUID, HRESULT, IUnknown, Interface, InterfaceRef, Result};

use crate::commands::{wrap_command_list, wrap_command_queue};
use crate::proxy::{
    Proxy, Slot, Slots, allocate, check_output, child_slots, object_slots, unsupported_slot, unwrap,
};

type Device = Proxy<ID3D12Device>;
type Out = *mut *mut c_void;

fn identity(device: &Device) -> InterfaceRef<'_, IUnknown> {
    // SAFETY: A device proxy begins with the device table, whose first three
    // slots implement IUnknown. The borrowed identity cannot outlive device.
    unsafe { InterfaceRef::from_raw(NonNull::from(device).cast()) }
}

unsafe fn wrap_pageable<N: Interface>(
    created: Result<N>,
    table: &'static [Slot],
    parent: &Device,
    out: Out,
) -> HRESULT {
    match created {
        Ok(native) => unsafe {
            allocate(
                table,
                &[ID3D12DeviceChild::IID, ID3D12Pageable::IID, N::IID],
                native,
                Some(&identity(parent)),
                (),
                out,
            )
        },
        Err(error) => error.code(),
    }
}

unsafe extern "system" fn reset_allocator(this: &Proxy<ID3D12CommandAllocator>) -> HRESULT {
    let host = &this.native;
    unsafe { (host.vtable().Reset)(host.as_raw()) }
}

static ALLOCATOR: Slots<16> = {
    let mut slots = child_slots::<ID3D12CommandAllocator, (), 16>();
    slots[8] = crate::slot!(reset_allocator);
    slots
};

unsafe extern "system" fn map_resource(
    this: &Proxy<ID3D12Resource>,
    subresource: u32,
    read: *const D3D12_RANGE,
    data: Out,
) -> HRESULT {
    let host = &this.native;
    unsafe { (host.vtable().Map)(host.as_raw(), subresource, read, data) }
}

unsafe extern "system" fn unmap_resource(
    this: &Proxy<ID3D12Resource>,
    subresource: u32,
    written: *const D3D12_RANGE,
) {
    let host = &this.native;
    unsafe { (host.vtable().Unmap)(host.as_raw(), subresource, written) }
}

static RESOURCE: Slots<32> = {
    let mut slots = child_slots::<ID3D12Resource, (), 32>();
    slots[8] = crate::slot!(map_resource);
    slots[9] = crate::slot!(unmap_resource);
    slots
};

unsafe extern "system" fn completed(this: &Proxy<ID3D12Fence>) -> u64 {
    unsafe { this.native.GetCompletedValue() }
}

unsafe extern "system" fn event_on_completion(
    this: &Proxy<ID3D12Fence>,
    value: u64,
    event: HANDLE,
) -> HRESULT {
    let host = &this.native;
    unsafe { (host.vtable().SetEventOnCompletion)(host.as_raw(), value, event) }
}

unsafe extern "system" fn signal_fence(this: &Proxy<ID3D12Fence>, value: u64) -> HRESULT {
    let host = &this.native;
    unsafe { (host.vtable().Signal)(host.as_raw(), value) }
}

static FENCE: Slots<16> = {
    let mut slots = child_slots::<ID3D12Fence, (), 16>();
    slots[8] = crate::slot!(completed);
    slots[9] = crate::slot!(event_on_completion);
    slots[10] = crate::slot!(signal_fence);
    slots
};

// Scarlett returns the address directly in RAX. The native method writes a
// D3D12_CPU_DESCRIPTOR_HANDLE to a hidden return buffer.
unsafe extern "system" fn cpu_handle(this: &Proxy<ID3D12DescriptorHeap>) -> usize {
    unsafe { this.native.GetCPUDescriptorHandleForHeapStart().ptr }
}

static HEAP: Slots<16> = {
    let mut slots = child_slots::<ID3D12DescriptorHeap, (), 16>();
    slots[9] = crate::slot!(cpu_handle);
    slots
};

/// Borrows a native resource while the caller holds the guest object alive.
///
/// # Safety
/// Non-null objects must have a readable COM vtable pointer and stay alive for
/// the returned borrow. `device` is the expected guest device identity.
pub(crate) unsafe fn resource<'a>(
    object: *mut c_void,
    device: *mut c_void,
) -> Option<&'a ID3D12Resource> {
    unsafe { unwrap::<ID3D12Resource, ()>(object, device, &RESOURCE) }.map(|proxy| &proxy.native)
}

/// Borrows a native fence under the same ownership contract as [`resource`].
pub(crate) unsafe fn fence<'a>(
    object: *mut c_void,
    device: *mut c_void,
) -> Option<&'a ID3D12Fence> {
    unsafe { unwrap::<ID3D12Fence, ()>(object, device, &FENCE) }.map(|proxy| &proxy.native)
}

/// Borrows a native allocator under the same ownership contract as [`resource`].
pub(crate) unsafe fn allocator<'a>(
    object: *mut c_void,
    device: *mut c_void,
) -> Option<&'a ID3D12CommandAllocator> {
    unsafe { unwrap::<ID3D12CommandAllocator, ()>(object, device, &ALLOCATOR) }
        .map(|proxy| &proxy.native)
}

unsafe extern "system" fn create_allocator(
    this: &Device,
    kind: D3D12_COMMAND_LIST_TYPE,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12CommandAllocator>(iid, out) };
    if status.is_err() {
        return status;
    }
    if kind != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return E_NOTIMPL;
    }
    unsafe {
        wrap_pageable(
            this.native
                .CreateCommandAllocator::<ID3D12CommandAllocator>(kind),
            &ALLOCATOR,
            this,
            out,
        )
    }
}

unsafe extern "system" fn create_heap(
    this: &Device,
    desc: *const D3D12_DESCRIPTOR_HEAP_DESC,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12DescriptorHeap>(iid, out) };
    if status.is_err() {
        return status;
    }
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_INVALIDARG;
    };
    if desc.NumDescriptors == 0 {
        return E_INVALIDARG;
    }
    if desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_RTV
        || desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_NONE
        || desc.NodeMask > 1
    {
        return E_NOTIMPL;
    }
    unsafe {
        wrap_pageable(
            this.native
                .CreateDescriptorHeap::<ID3D12DescriptorHeap>(desc),
            &HEAP,
            this,
            out,
        )
    }
}

unsafe extern "system" fn increment_size(this: &Device, kind: D3D12_DESCRIPTOR_HEAP_TYPE) -> u32 {
    unsafe { this.native.GetDescriptorHandleIncrementSize(kind) }
}

unsafe extern "system" fn create_rtv(
    this: &Device,
    object: *mut c_void,
    desc: *const D3D12_RENDER_TARGET_VIEW_DESC,
    handle: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    let Some(native) = (unsafe { resource(object, identity(this).as_raw()) }) else {
        unsafe { unsupported_slot() }
    };
    if handle.ptr == 0 {
        unsafe { unsupported_slot() };
    }
    unsafe {
        this.native
            .CreateRenderTargetView(native, Some(desc), handle)
    }
}

unsafe extern "system" fn create_resource(
    this: &Device,
    properties: *const D3D12_HEAP_PROPERTIES,
    flags: D3D12_HEAP_FLAGS,
    desc: *const D3D12_RESOURCE_DESC,
    initial: D3D12_RESOURCE_STATES,
    clear: *const D3D12_CLEAR_VALUE,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12Resource>(iid, out) };
    if status.is_err() {
        return status;
    }
    let (Some(heap), Some(desc)) = (unsafe { (properties.as_ref(), desc.as_ref()) }) else {
        return E_INVALIDARG;
    };
    if flags != D3D12_HEAP_FLAG_NONE
        || !matches!(
            heap.Type,
            D3D12_HEAP_TYPE_DEFAULT | D3D12_HEAP_TYPE_READBACK
        )
        || !matches!(
            desc.Dimension,
            D3D12_RESOURCE_DIMENSION_TEXTURE2D | D3D12_RESOURCE_DIMENSION_BUFFER
        )
    {
        return E_NOTIMPL;
    }
    let mut native = None;
    if let Err(error) = unsafe {
        this.native.CreateCommittedResource::<ID3D12Resource>(
            heap,
            flags,
            desc,
            initial,
            Some(clear),
            &mut native,
        )
    } {
        return error.code();
    }
    unsafe { wrap_pageable(native.ok_or_else(|| E_POINTER.into()), &RESOURCE, this, out) }
}

unsafe extern "system" fn create_fence(
    this: &Device,
    initial: u64,
    flags: D3D12_FENCE_FLAGS,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12Fence>(iid, out) };
    if status.is_err() {
        return status;
    }
    if flags != D3D12_FENCE_FLAG_NONE {
        return E_NOTIMPL;
    }
    unsafe {
        wrap_pageable(
            this.native.CreateFence::<ID3D12Fence>(initial, flags),
            &FENCE,
            this,
            out,
        )
    }
}

// The guest base slot uses a hidden LUID output pointer after the receiver.
unsafe extern "system" fn adapter_luid(this: &Device, result: *mut LUID) -> *mut LUID {
    unsafe { result.write(this.native.GetAdapterLuid()) };
    result
}

unsafe fn make_queue(this: &Device, desc: &D3D12_COMMAND_QUEUE_DESC, out: Out) -> HRESULT {
    match unsafe { this.native.CreateCommandQueue(desc) } {
        Ok(native) => unsafe { wrap_command_queue(native, &identity(this), out) },
        Err(error) => error.code(),
    }
}

unsafe extern "system" fn create_queue(
    this: &Device,
    desc: *const D3D12_COMMAND_QUEUE_DESC,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12CommandQueue>(iid, out) };
    if status.is_err() {
        return status;
    }
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_INVALIDARG;
    };
    if desc.Type != D3D12_COMMAND_LIST_TYPE_DIRECT
        || desc.Priority != D3D12_COMMAND_QUEUE_PRIORITY_NORMAL.0
        || desc.Flags != D3D12_COMMAND_QUEUE_FLAG_NONE
        || desc.NodeMask > 1
    {
        return E_NOTIMPL;
    }
    unsafe { make_queue(this, desc, out) }
}

// Slot 48's field order differs from the desktop queue descriptor.
#[repr(C)]
struct ComputeQueueDesc {
    kind: D3D12_COMMAND_LIST_TYPE,
    options: u32,
    priority_flag: u32,
    reserved: u32,
}

unsafe extern "system" fn create_queue_x(
    this: &Device,
    desc: *const ComputeQueueDesc,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12CommandQueue>(iid, out) };
    if status.is_err() {
        return status;
    }
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_INVALIDARG;
    };
    if desc.kind != D3D12_COMMAND_LIST_TYPE_COMPUTE
        || desc.options != 0
        || desc.priority_flag != 0
        || desc.reserved != 0
    {
        return E_NOTIMPL;
    }
    let native = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_COMPUTE,
        ..Default::default()
    };
    unsafe { make_queue(this, &native, out) }
}

#[repr(C)]
struct ListDesc {
    kind: D3D12_COMMAND_LIST_TYPE,
    reserved1: u32,
    node_mask: u32,
    reserved3: u32,
}

const _: () = assert!(size_of::<ListDesc>() == 16 && size_of::<ComputeQueueDesc>() == 16);

unsafe extern "system" fn create_list_x(
    this: &Device,
    desc: *const ListDesc,
    guest_allocator: *mut c_void,
    initial: *mut c_void,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12GraphicsCommandList>(iid, out) };
    if status.is_err() {
        return status;
    }
    let parent = identity(this);
    let (Some(desc), Some(allocator)) =
        (unsafe { (desc.as_ref(), allocator(guest_allocator, parent.as_raw())) })
    else {
        return E_INVALIDARG;
    };
    if !initial.is_null()
        || desc.kind != D3D12_COMMAND_LIST_TYPE_DIRECT
        || desc.reserved1 != 0
        || desc.node_mask != 1
        || desc.reserved3 != 0
    {
        return E_NOTIMPL;
    }
    let list: ID3D12GraphicsCommandList = match unsafe {
        this.native
            .CreateCommandList(desc.node_mask, desc.kind, allocator, None)
    } {
        Ok(list) => list,
        Err(error) => return error.code(),
    };
    // Desktop CreateCommandList starts open. The Xbox caller resets its list
    // before recording, so return it already closed.
    if let Err(error) = unsafe { list.Close() } {
        return error.code();
    }
    unsafe { wrap_command_list(list, &parent, out) }
}

static DEVICE: Slots<160> = {
    let mut slots = object_slots::<ID3D12Device, (), 160>();
    slots[8] = crate::slot!(create_queue);
    slots[9] = crate::slot!(create_allocator);
    slots[14] = crate::slot!(create_heap);
    slots[15] = crate::slot!(increment_size);
    slots[20] = crate::slot!(create_rtv);
    slots[27] = crate::slot!(create_resource);
    slots[36] = crate::slot!(create_fence);
    slots[43] = crate::slot!(adapter_luid);
    slots[48] = crate::slot!(create_queue_x);
    slots[73] = crate::slot!(create_list_x);
    slots
};

/// # Safety
/// `adapter` must be null or a borrowed DXGI adapter. The caller must supply
/// a cleared, writable output pointer for the new guest device reference.
pub(crate) unsafe fn create(adapter: *mut c_void, out: Out) -> HRESULT {
    let mut native = None;
    // SAFETY: The public factory validated its arguments. A null adapter
    // selects D3D12's default, otherwise it remains borrowed for this call.
    if let Err(error) = unsafe {
        D3D12CreateDevice::<_, ID3D12Device>(
            IUnknown::from_raw_borrowed(&adapter),
            D3D_FEATURE_LEVEL_12_0,
            &mut native,
        )
    } {
        return error.code();
    }
    let Some(native) = native else {
        return E_POINTER;
    };
    unsafe { allocate(&DEVICE, &[ID3D12Device::IID], native, None, (), out) }
}
