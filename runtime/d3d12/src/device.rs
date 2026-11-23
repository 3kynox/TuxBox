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
use crate::proxy::{Proxy, Slot, Slots, allocate, check_output, child_slots, object_slots, unwrap};

mod allocation;
pub(crate) mod blas;
pub(crate) mod compute;
mod compute_scene;
pub(crate) mod depth;
mod descriptor_copy;
mod dxgi;
mod features;
pub(crate) mod frame_events;
mod graphics;
mod heap;
pub(crate) mod indirect;
pub(crate) mod media;
pub(crate) mod placement;
pub(crate) mod queries;
pub(crate) mod ray_build;
mod ray_compile;
mod ray_pipeline;
pub(crate) mod rectangle;
mod shader_cache;
mod state_object;
pub(crate) mod stencil;
mod stream;
mod views;

type Device = Proxy<ID3D12Device, dxgi::DxgiDevice>;
pub(crate) enum Backing {
    Native,
    ConsoleLayout(D3D12_TEXTURE_LAYOUT, D3D12_RESOURCE_FLAGS),
    Placed(std::sync::Arc<placement::Texture>),
    Depth(Box<depth::Pair>),
}
impl Backing {
    pub(crate) fn placed(&self) -> Option<&std::sync::Arc<placement::Texture>> {
        match self {
            Self::Placed(texture) => Some(texture),
            _ => None,
        }
    }
    pub(crate) fn depth(&self) -> Option<&depth::Pair> {
        match self {
            Self::Depth(pair) => Some(pair),
            _ => None,
        }
    }
}
pub(crate) struct ResourceState {
    backing: Backing,
    blas_buffer: Option<blas::Registration>,
    stencil: std::sync::OnceLock<std::sync::Arc<stencil::Surface>>,
}
impl From<Backing> for ResourceState {
    fn from(backing: Backing) -> Self {
        Self {
            backing,
            blas_buffer: None,
            stencil: Default::default(),
        }
    }
}
impl core::ops::Deref for ResourceState {
    type Target = Backing;
    fn deref(&self) -> &Backing {
        &self.backing
    }
}
type Resource = Proxy<ID3D12Resource, ResourceState>;
type Out = *mut *mut c_void;

pub(crate) unsafe fn ray_state<'a>(
    guest: *mut c_void,
    owner: *mut c_void,
) -> Option<&'a ID3D12StateObject> {
    unsafe { state_object::native(guest, owner) }
}

fn identity(device: &Device) -> InterfaceRef<'_, IUnknown> {
    // SAFETY: A device proxy begins with the device table, whose first three
    // slots implement IUnknown. The borrowed identity cannot outlive device.
    unsafe { InterfaceRef::from_raw(NonNull::from(device).cast()) }
}

unsafe fn wrap_pageable<N: Interface, S>(
    created: Result<N>,
    table: &'static [Slot],
    parent: &Device,
    state: S,
    out: Out,
) -> HRESULT {
    match created {
        Ok(native) => unsafe {
            allocate(
                table,
                &[ID3D12DeviceChild::IID, ID3D12Pageable::IID, N::IID],
                native,
                Some(&identity(parent)),
                state,
                out,
            )
        },
        Err(error) => error.code(),
    }
}

pub(crate) type MediaResources = std::sync::Arc<std::sync::Mutex<Vec<ID3D12Resource>>>;
pub(crate) type Allocator = Proxy<ID3D12CommandAllocator, MediaResources>;

unsafe extern "system" fn reset_allocator(this: &Allocator) -> HRESULT {
    let host = &this.native;
    let mut sources = this.state.lock().unwrap_or_else(|e| e.into_inner());
    let status = unsafe { (host.vtable().Reset)(host.as_raw()) };
    if status.is_ok() {
        sources.clear();
    }
    status
}

static ALLOCATOR: Slots<16> = {
    let mut slots = child_slots::<ID3D12CommandAllocator, MediaResources, 16>();
    slots[8] = crate::slot!(reset_allocator);
    slots
};

unsafe extern "system" fn map_resource(
    this: &Resource,
    subresource: u32,
    read: *const D3D12_RANGE,
    data: Out,
) -> HRESULT {
    let host = &this.native;
    unsafe { (host.vtable().Map)(host.as_raw(), subresource, read, data) }
}

unsafe extern "system" fn unmap_resource(
    this: &Resource,
    subresource: u32,
    written: *const D3D12_RANGE,
) {
    let host = &this.native;
    unsafe { (host.vtable().Unmap)(host.as_raw(), subresource, written) }
}

static RESOURCE: Slots<32> = {
    let mut slots = child_slots::<ID3D12Resource, ResourceState, 32>();
    slots[8] = crate::slot!(map_resource);
    slots[9] = crate::slot!(unmap_resource);
    slots[10] = crate::slot!(resource_desc);
    slots[11] = crate::slot!(gpu_address);
    slots[14] = crate::slot!(heap_properties);
    slots
};

unsafe extern "system" fn resource_desc(
    this: &Resource,
    out: *mut D3D12_RESOURCE_DESC,
) -> *mut D3D12_RESOURCE_DESC {
    unsafe {
        out.write(match &this.state.backing {
            Backing::Depth(pair) => pair.guest,
            Backing::Placed(texture) => texture.desc,
            Backing::Native => this.native.GetDesc(),
            Backing::ConsoleLayout(layout, flags) => D3D12_RESOURCE_DESC {
                Layout: *layout,
                Flags: *flags,
                ..this.native.GetDesc()
            },
        })
    };
    out
}

unsafe extern "system" fn heap_properties(
    this: &Resource,
    props: *mut D3D12_HEAP_PROPERTIES,
    flags: *mut D3D12_HEAP_FLAGS,
) -> HRESULT {
    let native = &this.native;
    unsafe { (native.vtable().GetHeapProperties)(native.as_raw(), props, flags) }
}

unsafe extern "system" fn gpu_address(this: &Resource) -> u64 {
    // SAFETY: The guest resource holds its native interface until Release.
    this.state.placed().map_or_else(
        || unsafe { this.native.GetGPUVirtualAddress() },
        |p| p.address,
    )
}

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
pub(crate) struct DescriptorHeapState {
    pub(crate) desc: D3D12_DESCRIPTOR_HEAP_DESC,
    pub(crate) placed_views: Option<placement::HeapViews<placement::Texture>>,
    _depth_views: Option<placement::HeapViews<stencil::Surface>>,
    _scene_views: Option<placement::DescriptorRange<blas::SceneView>>,
}
type DescriptorHeap = Proxy<ID3D12DescriptorHeap, DescriptorHeapState>;

unsafe extern "system" fn cpu_handle(this: &DescriptorHeap) -> usize {
    unsafe { this.native.GetCPUDescriptorHandleForHeapStart().ptr }
}

unsafe extern "system" fn gpu_handle(this: &DescriptorHeap) -> u64 {
    unsafe { this.native.GetGPUDescriptorHandleForHeapStart().ptr }
}

// FD3D12SubAllocatedOnlineHeap::AllocateBlock passes writable storage in RDX.
unsafe extern "system" fn heap_desc(
    this: &DescriptorHeap,
    out: *mut D3D12_DESCRIPTOR_HEAP_DESC,
) -> *mut D3D12_DESCRIPTOR_HEAP_DESC {
    unsafe { out.write(this.state.desc) };
    out
}

static HEAP: Slots<16> = {
    let mut slots = child_slots::<ID3D12DescriptorHeap, DescriptorHeapState, 16>();
    slots[8] = crate::slot!(heap_desc);
    slots[9] = crate::slot!(cpu_handle);
    slots[10] = crate::slot!(gpu_handle);
    slots
};

pub(crate) unsafe fn descriptor_heap<'a>(
    object: *mut c_void,
    device: *mut c_void,
) -> Option<&'a DescriptorHeap> {
    unsafe { unwrap(object, device, &HEAP) }
}

/// Borrows a native resource while the caller holds the guest object alive.
///
/// # Safety
/// Non-null objects must have a readable COM vtable pointer and stay alive for
/// the returned borrow. `device` is the expected guest device identity.
pub(crate) unsafe fn resource<'a>(
    object: *mut c_void,
    device: *mut c_void,
) -> Option<&'a ID3D12Resource> {
    unsafe { resource_proxy(object, device) }.map(|proxy| &proxy.native)
}

pub(crate) unsafe fn resource_proxy<'a>(
    object: *mut c_void,
    device: *mut c_void,
) -> Option<&'a Resource> {
    unsafe { unwrap(object, device, &RESOURCE) }
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
) -> Option<&'a Allocator> {
    unsafe { unwrap(object, device, &ALLOCATOR) }
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
    if !supported_queue_kind(kind) {
        return E_NOTIMPL;
    }
    unsafe {
        wrap_pageable(
            this.native
                .CreateCommandAllocator::<ID3D12CommandAllocator>(kind),
            &ALLOCATOR,
            this,
            MediaResources::default(),
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
    if !matches!(
        desc.Type,
        D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV
            | D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER
            | D3D12_DESCRIPTOR_HEAP_TYPE_RTV
            | D3D12_DESCRIPTOR_HEAP_TYPE_DSV
    ) || !matches!(
        desc.Flags,
        D3D12_DESCRIPTOR_HEAP_FLAG_NONE | D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
    ) || desc.NodeMask > 1
    {
        return E_NOTIMPL;
    }
    if desc.Flags == D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
        && matches!(
            desc.Type,
            D3D12_DESCRIPTOR_HEAP_TYPE_RTV | D3D12_DESCRIPTOR_HEAP_TYPE_DSV
        )
    {
        return E_INVALIDARG;
    }
    let native = match unsafe {
        this.native
            .CreateDescriptorHeap::<ID3D12DescriptorHeap>(desc)
    } {
        Ok(native) => native,
        Err(error) => return error.code(),
    };
    let placed_views = this.state.placements.views.heap(
        &this.native,
        &native,
        desc,
        D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
    );
    let state = DescriptorHeapState {
        desc: *desc,
        placed_views,
        _scene_views: this.state.blas.views.heap(
            &this.native, &native, desc, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        ),
        _depth_views: this.state.depth_views.heap(
            &this.native,
            &native,
            desc,
            D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
        ),
    };
    unsafe { wrap_pageable(Ok(native), &HEAP, this, state, out) }
}

unsafe extern "system" fn increment_size(this: &Device, kind: D3D12_DESCRIPTOR_HEAP_TYPE) -> u32 {
    unsafe { this.native.GetDescriptorHandleIncrementSize(kind) }
}

pub(crate) const BUFFER_READ_BITS: i32 =
    D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER.0 | D3D12_RESOURCE_STATE_INDEX_BUFFER.0;

pub(crate) fn host_state(state: D3D12_RESOURCE_STATES, texture: bool) -> D3D12_RESOURCE_STATES {
    // Frosty's SetSRVs emits these two tagged read states, including on compute lists.
    let bits = match state.0 {
        0x1200_00c0 => 0xc0,
        0x1200_0040 => 0x40,
        other => other,
    };
    D3D12_RESOURCE_STATES(if texture {
        bits & !BUFFER_READ_BITS
    } else {
        bits
    })
}

unsafe extern "system" fn create_resource<const EXTENDED: bool>(
    this: &Device,
    properties: *const D3D12_HEAP_PROPERTIES,
    flags: D3D12_HEAP_FLAGS,
    guest_desc: *const D3D12_RESOURCE_DESC,
    initial: D3D12_RESOURCE_STATES,
    clear: *const D3D12_CLEAR_VALUE,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12Resource>(iid, out) };
    if status.is_err() {
        return status;
    }
    let (Some(heap), Some(desc)) = (unsafe { (properties.as_ref(), guest_desc.as_ref()) }) else {
        return E_INVALIDARG;
    };
    if !matches!(flags, D3D12_HEAP_FLAG_NONE | D3D12_HEAP_FLAG_ALLOW_DISPLAY)
        || !matches!(
            heap.Type,
            D3D12_HEAP_TYPE_DEFAULT | D3D12_HEAP_TYPE_READBACK | D3D12_HEAP_TYPE_UPLOAD
        )
        || !matches!(
            desc.Dimension,
            D3D12_RESOURCE_DIMENSION_BUFFER
                | D3D12_RESOURCE_DIMENSION_TEXTURE1D
                | D3D12_RESOURCE_DIMENSION_TEXTURE2D
                | D3D12_RESOURCE_DIMENSION_TEXTURE3D
        )
    {
        return E_NOTIMPL;
    }
    if flags == D3D12_HEAP_FLAG_ALLOW_DISPLAY
        && (heap.Type != D3D12_HEAP_TYPE_DEFAULT
            || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D)
    {
        return E_INVALIDARG;
    }
    let mut host_desc = *desc;
    if EXTENDED {
        // Slot 76 extends the resource description at byte 52 (desktop padding).
        // The Scarlett upscaler writes DCC options there and a zero qword at 56.
        const _: () = assert!(core::mem::offset_of!(D3D12_RESOURCE_DESC, Flags) == 48);
        let bytes = guest_desc.cast::<u8>();
        let (dcc, extension) = unsafe {
            (
                bytes.add(52).cast::<u32>().read_unaligned(),
                bytes.add(56).cast::<u64>().read_unaligned(),
            )
        };
        if !matches!(dcc, 0 | 1 | 3) || extension != 0 {
            return E_NOTIMPL;
        }
        const DCC_FLAGS: i32 = 0x00c0_0000;
        if desc.Layout.0 == 0x101 || desc.Flags.0 & DCC_FLAGS != 0 {
            // A private committed texture exposes its logical contents through
            // D3D12. The host driver chooses its tiling and compression.
            if flags != D3D12_HEAP_FLAG_NONE
                || heap.Type != D3D12_HEAP_TYPE_DEFAULT
                || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
                || desc.Flags.contains(D3D12_RESOURCE_FLAG_ALLOW_CROSS_ADAPTER)
            {
                return E_NOTIMPL;
            }
            if desc.Layout.0 == 0x101 {
                host_desc.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
            }
            host_desc.Flags.0 &= !DCC_FLAGS;
        }
    }
    let primary = depth::primary(desc);
    if primary.is_some()
        && (flags != D3D12_HEAP_FLAG_NONE
            || heap.Type != D3D12_HEAP_TYPE_DEFAULT
            || initial.contains(D3D12_RESOURCE_STATE_UNORDERED_ACCESS))
    {
        return E_NOTIMPL;
    }
    let mut native = None;
    if let Err(error) = unsafe {
        this.native.CreateCommittedResource::<ID3D12Resource>(
            heap,
            flags,
            primary.as_ref().unwrap_or(&host_desc),
            host_state(initial, desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER),
            Some(clear),
            &mut native,
        )
    } {
        return error.code();
    }
    unsafe { depth::wrap::<EXTENDED>(this, heap, desc, primary.as_ref(), initial, native, out) }
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
            (),
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

fn supported_queue_kind(kind: D3D12_COMMAND_LIST_TYPE) -> bool {
    matches!(
        kind,
        D3D12_COMMAND_LIST_TYPE_DIRECT
            | D3D12_COMMAND_LIST_TYPE_COMPUTE
            | D3D12_COMMAND_LIST_TYPE_COPY
    )
}

fn unsupported_queue(slot: u32, words: [u32; 4]) -> HRESULT {
    use windows::Win32::System::Diagnostics::Debug::OutputDebugStringA;
    use windows::core::PCSTR;
    let message = format!(
        "Project TuxBox: unsupported queue descriptor at slot {slot}, words {words:x?}\n\0"
    );
    unsafe { OutputDebugStringA(PCSTR(message.as_ptr())) };
    E_NOTIMPL
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
    if !supported_queue_kind(desc.Type)
        || !matches!(
            D3D12_COMMAND_QUEUE_PRIORITY(desc.Priority),
            D3D12_COMMAND_QUEUE_PRIORITY_NORMAL
                | D3D12_COMMAND_QUEUE_PRIORITY_HIGH
                | D3D12_COMMAND_QUEUE_PRIORITY_GLOBAL_REALTIME
        )
        || !matches!(
            desc.Flags,
            D3D12_COMMAND_QUEUE_FLAG_NONE | D3D12_COMMAND_QUEUE_FLAG_DISABLE_GPU_TIMEOUT
        )
        || desc.NodeMask > 1
    {
        return unsupported_queue(
            8,
            [
                desc.Type.0 as u32,
                desc.Priority as u32,
                desc.Flags.0 as u32,
                desc.NodeMask,
            ],
        );
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
        return unsupported_queue(
            48,
            [
                desc.kind.0 as u32,
                desc.options,
                desc.priority_flag,
                desc.reserved,
            ],
        );
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
    let compact = unsafe { iid.as_ref() } == Some(&media::DMA_IID);
    let host_iid = ID3D12GraphicsCommandList::IID;
    let status = unsafe {
        check_output::<ID3D12GraphicsCommandList>(if compact { &host_iid } else { iid }, out)
    };
    if status.is_err() {
        return status;
    }
    if compact
        && unsafe { desc.as_ref() }.is_none_or(|desc| desc.kind != D3D12_COMMAND_LIST_TYPE_COPY)
    {
        return windows::Win32::Foundation::E_NOINTERFACE;
    }
    let parent = identity(this);
    let (Some(desc), Some(allocator)) =
        (unsafe { (desc.as_ref(), allocator(guest_allocator, parent.as_raw())) })
    else {
        return E_INVALIDARG;
    };
    // Electra's compact DMA list at RVA 0x19849c3 selects the default node
    // with {COPY,0,0,0}. Both default and explicit node one target node zero.
    let valid_node = desc.node_mask == 1 || (compact && desc.node_mask == 0);
    if !supported_queue_kind(desc.kind) || desc.reserved1 != 0 || !valid_node || desc.reserved3 != 0
    {
        return E_NOTIMPL;
    }
    // SAFETY: the caller keeps its optional pipeline alive throughout creation.
    let initial = match unsafe { compute::pipeline(initial, parent.as_raw()) } {
        Ok(initial) => initial,
        Err(error) => return error,
    };
    let list: ID3D12GraphicsCommandList = match unsafe {
        this.native.CreateCommandList(
            desc.node_mask,
            desc.kind,
            &allocator.native,
            initial.map(|p| &p.native),
        )
    } {
        Ok(list) => list,
        Err(error) => return error.code(),
    };
    // Hold an empty native recording ready for a close-first guest, releasing
    // the allocator so reset-first callers can rewind it before their list.
    if let Err(error) = unsafe { list.Close() } {
        return error.code();
    }
    unsafe {
        wrap_command_list(
            list,
            allocator,
            initial,
            &parent,
            compact,
            out,
        )
    }
}

pub(crate) type DisposeCallback = unsafe extern "system" fn(*mut c_void, *mut c_void);

/// Accept the controlling guest device identity. Other COM object layouts
/// cannot contain the device's callback slot.
pub(crate) unsafe fn set_dispose_callback(
    device: *mut c_void, callback: Option<DisposeCallback>, context: *mut c_void,
) -> HRESULT {
    if device.is_null() || unsafe { device.cast::<*const Slot>().read_unaligned() } != DEVICE.as_ptr() {
        return E_INVALIDARG;
    }
    let state = unsafe { &(*device.cast::<Device>()).state };
    *state.disposal.lock().unwrap_or_else(|e| e.into_inner()) =
        callback.map(|callback| (callback, context.expose_provenance()));
    HRESULT(0)
}

unsafe extern "system" fn release_device(this: *mut Device) -> u32 {
    unsafe { crate::proxy::release_with(this, || {
        let hook = (*this).state.disposal.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some((callback, context)) = hook {
            callback(this.cast(), core::ptr::with_exposed_provenance_mut(context));
        }
    }) }
}

static DEVICE: Slots<160> = {
    let mut slots = object_slots::<ID3D12Device, dxgi::DxgiDevice, 160>();
    slots[0] = crate::slot!(dxgi::query);
    slots[2] = crate::slot!(release_device);
    slots[8] = crate::slot!(create_queue);
    slots[9] = crate::slot!(create_allocator);
    slots[13] = crate::slot!(features::query);
    slots[14] = crate::slot!(create_heap);
    slots[15] = crate::slot!(increment_size);
    slots[16] = crate::slot!(compute::create_root_signature);
    slots[17] = crate::slot!(views::cbv);
    slots[18] = crate::slot!(views::srv);
    slots[19] = crate::slot!(views::uav);
    slots[20] = crate::slot!(views::rtv);
    slots[21] = crate::slot!(views::dsv);
    slots[22] = crate::slot!(views::sampler);
    slots[23] = crate::slot!(descriptor_copy::ranges);
    slots[24] = crate::slot!(descriptor_copy::simple);
    slots[25] = crate::slot!(allocation::info);
    slots[27] = crate::slot!(create_resource::<false>);
    slots[28] = crate::slot!(heap::create);
    slots[29] = crate::slot!(heap::place);
    slots[36] = crate::slot!(create_fence);
    slots[38] = crate::slot!(allocation::footprints);
    slots[39] = crate::slot!(queries::create_heap);
    slots[41] = crate::slot!(indirect::create_signature);
    slots[43] = crate::slot!(adapter_luid);
    slots[44] = crate::slot!(heap::place_virtual);
    slots[48] = crate::slot!(create_queue_x);
    slots[60] = crate::slot!(graphics::create_pipeline);
    slots[62] = crate::slot!(compute::create_pipeline_x);
    slots[67] = crate::slot!(compute::deserialize_pipeline_x);
    slots[73] = crate::slot!(create_list_x);
    slots[76] = crate::slot!(create_resource::<true>);
    slots[78] = crate::slot!(frame_events::interval);
    slots[79] = crate::slot!(frame_events::schedule);
    slots[80] = crate::slot!(frame_events::wait);
    slots[81] = crate::slot!(frame_events::statistics);
    slots[85] = crate::slot!(heap::residency_priority);
    slots[86] = crate::slot!(stream::create_pipeline);
    slots[101] = crate::slot!(ray_pipeline::create);
    slots[102] = crate::slot!(ray_build::prebuild);
    slots[106] = crate::slot!(ray_pipeline::add);
    slots[115] = crate::slot!(state_object::deserialize);
    slots
};

pub(crate) const DEVICE_INTERFACES: &[GUID] = &[
    ID3D12Device::IID,
    ID3D12Device1::IID,
    ID3D12Device2::IID,
    ID3D12Device3::IID,
    ID3D12Device4::IID,
    ID3D12Device5::IID,
    ID3D12Device6::IID,
    ID3D12Device7::IID,
    state_object::DEVICE8,
];

/// # Safety
/// `adapter` must be null or a borrowed DXGI adapter. The caller must supply
/// a cleared, writable output pointer for the new guest device reference.
pub(crate) unsafe fn create(adapter: *mut c_void, requested: GUID, out: Out) -> HRESULT {
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
    crate::diagnostics::device_created(&native);
    // SAFETY: the caller holds the selected adapter through factory return.
    let pipeline_device = native.cast::<ID3D12Device2>().ok();
    let interfaces: &'static [GUID] = if native.cast::<ID3D12Device8>().is_ok() {
        DEVICE_INTERFACES
    } else if native.cast::<ID3D12Device7>().is_ok() {
        &DEVICE_INTERFACES[..8]
    } else if native.cast::<ID3D12Device6>().is_ok() {
        &DEVICE_INTERFACES[..7]
    } else if native.cast::<ID3D12Device5>().is_ok() {
        &DEVICE_INTERFACES[..6]
    } else if pipeline_device.is_some() {
        &[ID3D12Device::IID, ID3D12Device1::IID, ID3D12Device2::IID]
    } else {
        &[ID3D12Device::IID]
    };
    if !interfaces.contains(&requested) {
        return windows::Win32::Foundation::E_NOINTERFACE;
    }
    let state = match dxgi::DxgiDevice::new(
        unsafe { IUnknown::from_raw_borrowed(&adapter) },
        pipeline_device,
    ) {
        Ok(state) => state,
        Err(err) => return err.code(),
    };
    unsafe { allocate(&DEVICE, interfaces, native, None, state, out) }
}
