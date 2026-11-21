//! Guest command lists and queues. Embedded guest COM pointers are translated
//! before a batch reaches the desktop API.

use core::{ffi::c_void, mem, ops::Range, ptr, slice};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, RECT, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringA;
use windows::core::{HRESULT, IUnknown, Interface, s};

use crate::device;
use crate::device::placement::{self, Texture};
use crate::present::Presentation;
use crate::proxy::{Proxy, Slots, allocate, child_slots, unwrap};

mod compute;
mod depth;
mod descriptors;
mod graphics;
mod media;
mod queries;

enum ListState {
    Fresh(ID3D12CommandAllocator, Option<ID3D12PipelineState>),
    Open,
    Closed,
}

struct Recording {
    media: Option<media::Recording>,
    placed_reads: Vec<Arc<Texture>>,
    placed_writes: Vec<Arc<Texture>>,
    resource_heap: Option<Range<usize>>,
    sampled_heaps: Vec<Range<usize>>,
    phase: ListState,
    error: HRESULT,
}

impl Recording {
    fn prepare_copy(
        &mut self,
        destination: Option<&Arc<Texture>>,
        source: Option<&Arc<Texture>>,
    ) -> Result<(), HRESULT> {
        for (accesses, texture) in [
            (&mut self.placed_reads, source),
            (&mut self.placed_writes, destination),
        ] {
            if let Some(texture) = texture
                && !accesses.iter().any(|known| Arc::ptr_eq(known, texture))
            {
                accesses.try_reserve(1).map_err(|_| E_OUTOFMEMORY)?;
                accesses.push(Arc::clone(texture));
            }
        }
        Ok(())
    }

    fn sample_heap(&mut self) {
        if let Some(heap) = &self.resource_heap
            && !self.sampled_heaps.contains(heap)
        {
            self.sampled_heaps.push(heap.clone());
        }
    }

    fn can_record(&mut self, native: &ID3D12GraphicsCommandList) -> bool {
        if self.error.is_err() {
            return false;
        }
        match &self.phase {
            ListState::Fresh(allocator, initial) => {
                // The factory closed the empty native list to release its allocator.
                // Reopen with its original bindings when the first operation records.
                if let Err(error) = unsafe { native.Reset(allocator, initial.as_ref()) } {
                    self.error = error.code();
                    return false;
                }
                self.phase = ListState::Open;
            }
            ListState::Open => {}
            ListState::Closed => self.error = E_FAIL,
        }
        self.error.is_ok()
    }
}

// Xbox callers can emit native command packets when space is available.
// Frosty's topology path compares +0x18 with +0x30 at RVA 0x57f02fe.
// Empty bounds select the Xbox vtable path, which records desktop commands.
#[repr(C)]
struct ListData {
    reserved0: [usize; 2],
    write_cursor: usize,
    reserved1: [usize; 2],
    packet_end: usize,
    reserved2: [u8; 0x540 - 0x38],
    // The guest writes these pipeline packet words before it checks the
    // packet bounds and calls slot 25. Keep them separate from host state.
    pipeline_packets: core::cell::UnsafeCell<[u8; 32]>,
    recording: Mutex<Recording>,
    extended: OnceLock<Option<ID3D12GraphicsCommandList1>>,
    enhanced_barriers: OnceLock<Option<ID3D12GraphicsCommandList7>>,
    mesh: OnceLock<Option<ID3D12GraphicsCommandList6>>,
}

type List = Proxy<ID3D12GraphicsCommandList, ListData>;
const _: () =
    assert!(mem::offset_of!(List, state) + mem::offset_of!(ListData, pipeline_packets) == 0x540);
const _: () =
    assert!(mem::offset_of!(List, state) + mem::offset_of!(ListData, write_cursor) == 0x18);
const _: () = assert!(mem::offset_of!(List, state) + mem::offset_of!(ListData, packet_end) == 0x30);

fn recording(list: &List) -> MutexGuard<'_, Recording> {
    list.state
        .recording
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

// FD3D12CommandListFence::Signal reads the type at queue+0x10 before
// selecting the flush flags. The queue's Xbox prefix precedes private state.
#[repr(C)]
struct QueueState {
    reserved: usize,
    kind: D3D12_COMMAND_LIST_TYPE,
    // ExecuteCommandLists is void. Report a rejected batch on the next fence
    // signal or present, and never signal completion for missing work.
    placement: placement::QueueAccess,
    submission: Mutex<()>,
    presentation: Presentation,
}

type Queue = Proxy<ID3D12CommandQueue, QueueState>;
const _: () = assert!(mem::offset_of!(Queue, state) + mem::offset_of!(QueueState, kind) == 0x10);

fn parent(device: &Option<IUnknown>) -> *mut c_void {
    device.as_ref().map_or(ptr::null_mut(), Interface::as_raw)
}

// Validate and translate the entire batch before submission. Small batches use
// stack storage and larger ones can report E_OUTOFMEMORY to the guest.
fn translate_batch<T: Default, G, C>(
    guest: &[G],
    mut context: C,
    translate: impl Fn(&G, &mut C) -> Result<T, HRESULT>,
    submit: impl FnOnce(&[T], C) -> Result<(), HRESULT>,
) -> Result<(), HRESULT> {
    let mut stack: [T; 8];
    let mut heap = Vec::new();
    let host = if guest.len() <= 8 {
        stack = std::array::from_fn(|_| T::default());
        &mut stack[..guest.len()]
    } else {
        heap.try_reserve_exact(guest.len())
            .map_err(|_| E_OUTOFMEMORY)?;
        heap.resize_with(guest.len(), T::default);
        heap.as_mut_slice()
    };
    for (dst, src) in host.iter_mut().zip(guest) {
        *dst = translate(src, &mut context)?;
    }
    submit(host, context)
}

unsafe extern "system" fn get_type(list: &List) -> D3D12_COMMAND_LIST_TYPE {
    unsafe { list.native.GetType() }
}

unsafe extern "system" fn reset(
    list: &List,
    allocator: *mut c_void,
    initial: *mut c_void,
) -> HRESULT {
    let device = parent(&list.parent);
    let Some(allocator) = (unsafe { device::allocator(allocator, device) }) else {
        return E_INVALIDARG;
    };
    // SAFETY: the guest holds the initial pipeline through this reset.
    let initial = match unsafe { device::compute::pipeline(initial, device) } {
        Ok(pipeline) => pipeline,
        Err(error) => return error,
    };
    let mut state = recording(list);
    if matches!(state.phase, ListState::Open) {
        return E_FAIL;
    }
    let native = &list.native;
    let hr = unsafe {
        (native.vtable().Reset)(
            native.as_raw(),
            allocator.native.as_raw(),
            initial.map_or(ptr::null_mut(), Interface::as_raw),
        )
    };
    if hr.is_ok() {
        state.phase = ListState::Open;
        if state.media.is_some() {
            state.media = Some(media::Recording::new(allocator));
        }
        state.placed_reads.clear();
        state.placed_writes.clear();
        state.resource_heap = None;
        state.sampled_heaps.clear();
        state.error = S_OK;
    }
    hr
}

unsafe extern "system" fn close(list: &List) -> HRESULT {
    let mut state = recording(list);
    match state.phase {
        ListState::Closed => return E_FAIL,
        ListState::Fresh(..) => {} // The initial empty recording is already closed on the host.
        ListState::Open => {
            if let Err(error) = unsafe { list.native.Close() } {
                state.error = error.code();
            }
        }
    }
    state.phase = ListState::Closed;
    state.error
}

// The COM pointer in a barrier or copy location is borrowed by the desktop
// call. Raw fields allow us to replace it without taking an owned reference.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TransitionBarrier {
    kind: D3D12_RESOURCE_BARRIER_TYPE,
    flags: D3D12_RESOURCE_BARRIER_FLAGS,
    resource: *mut c_void,
    subresource: u32,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
}

// Both members start with the guest tag and flags. Aliasing barriers carry
// two independently optional resources, with no change of resource state.
#[repr(C)]
#[derive(Clone, Copy)]
struct AliasingBarrier {
    kind: D3D12_RESOURCE_BARRIER_TYPE,
    flags: D3D12_RESOURCE_BARRIER_FLAGS,
    before: *mut c_void,
    after: *mut c_void,
}

#[repr(C)]
union RecordedBarrier {
    kind: D3D12_RESOURCE_BARRIER_TYPE,
    transition: TransitionBarrier,
    aliasing: AliasingBarrier,
}

impl Default for RecordedBarrier {
    fn default() -> Self {
        Self {
            transition: TransitionBarrier::default(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CopyLocation {
    resource: *mut c_void,
    kind: D3D12_TEXTURE_COPY_TYPE,
    data: D3D12_TEXTURE_COPY_LOCATION_0,
}

const _: () = {
    assert!(size_of::<RecordedBarrier>() == size_of::<D3D12_RESOURCE_BARRIER>());
    assert!(
        mem::offset_of!(AliasingBarrier, before)
            == mem::offset_of!(D3D12_RESOURCE_BARRIER, Anonymous)
    );
    assert!(
        mem::offset_of!(AliasingBarrier, after)
            == mem::offset_of!(AliasingBarrier, before)
                + mem::offset_of!(D3D12_RESOURCE_ALIASING_BARRIER, pResourceAfter)
    );
    assert!(size_of::<TransitionBarrier>() == size_of::<D3D12_RESOURCE_BARRIER>());
    assert!(align_of::<TransitionBarrier>() == align_of::<D3D12_RESOURCE_BARRIER>());
    assert!(
        mem::offset_of!(TransitionBarrier, resource)
            == mem::offset_of!(D3D12_RESOURCE_BARRIER, Anonymous)
    );
    assert!(size_of::<CopyLocation>() == size_of::<D3D12_TEXTURE_COPY_LOCATION>());
    assert!(align_of::<CopyLocation>() == align_of::<D3D12_TEXTURE_COPY_LOCATION>());
    assert!(
        mem::offset_of!(CopyLocation, data)
            == mem::offset_of!(D3D12_TEXTURE_COPY_LOCATION, Anonymous)
    );
    assert!(size_of::<CopyLocation>() == 48 && mem::offset_of!(CopyLocation, data) == 16);
};

unsafe extern "system" fn transition(
    list: &List,
    count: u32,
    barriers: *const D3D12_RESOURCE_BARRIER,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) || count == 0 {
        return;
    }
    if barriers.is_null() {
        state.error = E_INVALIDARG;
        return;
    }
    // The guest supplies count readable barriers. Read a transition payload
    // only after checking the union tag.
    let guest = unsafe { slice::from_raw_parts(barriers, count as usize) };
    let result = translate_batch(
        guest,
        Vec::new(),
        |barrier, handoffs| {
            if barrier.Type == D3D12_RESOURCE_BARRIER_TYPE_ALIASING
                && barrier.Flags == D3D12_RESOURCE_BARRIER_FLAG_NONE
            {
                let alias = unsafe { &barrier.Anonymous.Aliasing };
                let native = |guest: &Option<ID3D12Resource>| match guest {
                    Some(object) => {
                        unsafe { device::resource(object.as_raw(), parent(&list.parent)) }
                            .map(Interface::as_raw)
                            .ok_or(E_INVALIDARG)
                    }
                    None => Ok(ptr::null_mut()),
                };
                return Ok(RecordedBarrier {
                    aliasing: AliasingBarrier {
                        kind: barrier.Type,
                        flags: barrier.Flags,
                        before: native(&alias.pResourceBefore)?,
                        after: native(&alias.pResourceAfter)?,
                    },
                });
            }
            if barrier.Type == D3D12_RESOURCE_BARRIER_TYPE_UAV
                && barrier.Flags == D3D12_RESOURCE_BARRIER_FLAG_NONE
            {
                // A null UAV orders all UAV accesses. Only the first pointer of
                // this union member is initialized by the guest.
                let guest = unsafe { &barrier.Anonymous.UAV.pResource };
                let resource = match guest.as_ref() {
                    Some(object) => {
                        let r = unsafe {
                            device::resource_proxy(object.as_raw(), parent(&list.parent))
                        }
                        .ok_or(E_INVALIDARG)?;
                        r.state
                            .depth()
                            .map_or(&r.native, |pair| &pair.image)
                            .as_raw()
                    }
                    None => ptr::null_mut(),
                };
                return Ok(RecordedBarrier {
                    transition: TransitionBarrier {
                        kind: barrier.Type,
                        flags: barrier.Flags,
                        resource,
                        ..Default::default()
                    },
                });
            }
            if barrier.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION
                || !matches!(
                    barrier.Flags,
                    D3D12_RESOURCE_BARRIER_FLAG_NONE
                        | D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY
                        | D3D12_RESOURCE_BARRIER_FLAG_END_ONLY
                )
            {
                crate::diagnostics::recording_error(format_args!(
                    "resource barrier type {} flags {:#x}",
                    barrier.Type.0, barrier.Flags.0
                ));
                return Err(E_NOTIMPL);
            }
            let mut host = unsafe { ptr::from_ref(barrier).cast::<TransitionBarrier>().read() };
            let resource = unsafe { device::resource_proxy(host.resource, parent(&list.parent)) }
                .ok_or(E_INVALIDARG)?;
            let different = host.before != host.after;
            let needs_dimension = (host.before.0 | host.after.0) & device::BUFFER_READ_BITS != 0;
            let texture = needs_dimension
                && unsafe { resource.native.GetDesc().Dimension }
                    != D3D12_RESOURCE_DIMENSION_BUFFER;
            host.before = device::host_state(host.before, texture);
            host.after = device::host_state(host.after, texture);
            if let Some(pair) = resource.state.depth()
                && (host.before == D3D12_RESOURCE_STATE_UNORDERED_ACCESS
                    || host.after == D3D12_RESOURCE_STATE_UNORDERED_ACCESS)
            {
                if !matches!(
                    host.subresource,
                    0 | 1 | D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES
                ) {
                    return Err(E_INVALIDARG);
                }
                if host.flags != D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY && different {
                    handoffs.try_reserve(1).map_err(|_| E_OUTOFMEMORY)?;
                    let index = unsafe { ptr::from_ref(barrier).offset_from(barriers) } as usize;
                    handoffs.push((index, &resource.native, pair));
                }
                host.resource = ptr::null_mut();
            } else if resource.state.placed().is_some() {
                if !placement::supported_state(host.before)
                    || !placement::supported_state(host.after)
                {
                    crate::diagnostics::recording_error(format_args!(
                        "placed texture transition {:#x} to {:#x}",
                        host.before.0, host.after.0
                    ));
                    return Err(E_NOTIMPL);
                }
                // Read promotions can accumulate implicitly. An explicit barrier
                // orders copy writes against other uses within the same batch.
                host.resource = if different
                    && (host.before == D3D12_RESOURCE_STATE_COPY_DEST
                        || host.after == D3D12_RESOURCE_STATE_COPY_DEST)
                {
                    resource.native.as_raw()
                } else {
                    ptr::null_mut()
                };
            } else if different && host.before == host.after {
                host.resource = ptr::null_mut();
            } else {
                host.resource = resource.native.as_raw();
            }
            Ok(RecordedBarrier { transition: host })
        },
        |host, handoffs| unsafe {
            let mut handoffs = handoffs.into_iter().peekable();
            let mut first = 0;
            let submit = |run: &[RecordedBarrier]| {
                if !run.is_empty() {
                    (list.native.vtable().ResourceBarrier)(
                        list.native.as_raw(),
                        run.len() as u32,
                        run.as_ptr().cast(),
                    );
                }
            };
            for (index, barrier) in host.iter().enumerate() {
                if barrier.kind == D3D12_RESOURCE_BARRIER_TYPE_TRANSITION
                    && barrier.transition.resource.is_null()
                {
                    submit(&host[first..index]);
                    if let Some((_, resource, pair)) = handoffs.next_if(|(at, _, _)| *at == index) {
                        depth::handoff(&list.native, resource, pair, &barrier.transition);
                    }
                    first = index + 1;
                }
            }
            submit(&host[first..]);
            Ok(())
        },
    );
    if let Err(error) = result {
        state.error = error;
    }
}

fn enhanced_barriers(native: &ID3D12GraphicsCommandList) -> Option<ID3D12GraphicsCommandList7> {
    if !matches!(
        unsafe { native.GetType() },
        D3D12_COMMAND_LIST_TYPE_DIRECT | D3D12_COMMAND_LIST_TYPE_COMPUTE
    ) {
        return None;
    }
    let mut device: Option<ID3D12Device> = None;
    unsafe { native.GetDevice(&mut device) }.ok()?;
    let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS12::default();
    unsafe {
        device?.CheckFeatureSupport(
            D3D12_FEATURE_D3D12_OPTIONS12,
            ptr::from_mut(&mut options).cast(),
            size_of_val(&options) as u32,
        )
    }
    .ok()?;
    if options.EnhancedBarriersSupported.as_bool() {
        native.cast().ok()
    } else {
        None
    }
}

unsafe extern "system" fn flush_texture_cache(list: &List, mask: u32, address: u64, bytes: u64) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    // FlushTextureCache covers the GPU address space. RHIFlushTextureCacheBOP
    // supplies one allocation's VA and extent. A global barrier orders either
    // range without changing the resource's layout or dereferencing a guest VA.
    let valid = match mask {
        0xc0_0000 => address == 0 && bytes == 0xff_ffff_ff00,
        0x3000 => address != 0 && bytes != 0 && address.checked_add(bytes).is_some(),
        _ => {
            crate::diagnostics::recording_error(format_args!(
                "texture cache flush mask {mask:#x} address {address:#x} bytes {bytes:#x}"
            ));
            state.error = E_NOTIMPL;
            return;
        }
    };
    if !valid {
        state.error = E_INVALIDARG;
        return;
    }
    let Some(native) = list
        .state
        .enhanced_barriers
        .get_or_init(|| enhanced_barriers(&list.native))
    else {
        crate::diagnostics::recording_error(format_args!("enhanced global barriers unavailable"));
        state.error = E_NOTIMPL;
        return;
    };
    let barrier = D3D12_GLOBAL_BARRIER {
        SyncBefore: D3D12_BARRIER_SYNC_ALL,
        SyncAfter: D3D12_BARRIER_SYNC_ALL,
        AccessBefore: D3D12_BARRIER_ACCESS_COMMON,
        AccessAfter: D3D12_BARRIER_ACCESS_COMMON,
    };
    let group = D3D12_BARRIER_GROUP {
        Type: D3D12_BARRIER_TYPE_GLOBAL,
        NumBarriers: 1,
        Anonymous: D3D12_BARRIER_GROUP_0 {
            pGlobalBarriers: &barrier,
        },
    };
    // SAFETY: the native open list consumes the stack-backed group during Barrier.
    unsafe { native.Barrier(&[group]) };
}

unsafe extern "system" fn clear_rtv(
    list: &List,
    handle: D3D12_CPU_DESCRIPTOR_HANDLE,
    color: *const f32,
    count: u32,
    rects: *const RECT,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if handle.ptr == 0 || color.is_null() || (count != 0 && rects.is_null()) {
        state.error = E_INVALIDARG;
        return;
    }
    unsafe {
        (list.native.vtable().ClearRenderTargetView)(
            list.native.as_raw(),
            handle,
            color,
            count,
            rects,
        )
    };
}

// HandleDiscardResources at Frosty RVA 0x5787b84 supplies the native region
// layout (rect count and pointer, first subresource and subresource count).
unsafe extern "system" fn discard(
    list: &List,
    object: *mut c_void,
    region: *const D3D12_DISCARD_REGION,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    let Some(resource) = (unsafe { device::resource_proxy(object, parent(&list.parent)) }) else {
        state.error = E_INVALIDARG;
        return;
    };
    if (unsafe { region.as_ref() }).is_some_and(|r| r.NumRects != 0 && r.pRects.is_null()) {
        state.error = E_INVALIDARG;
        return;
    }
    // CPU-backed read-only textures acquire their contents at submission.
    // Keeping the old bytes of a discarded read-only region is permitted.
    if resource.state.placed().is_none() {
        unsafe {
            (list.native.vtable().DiscardResource)(
                list.native.as_raw(),
                resource.native.as_raw(),
                region,
            )
        };
    }
}

unsafe fn copy_location<'a>(
    location: *const CopyLocation,
    device: *mut c_void,
) -> Option<(CopyLocation, Option<&'a Arc<Texture>>)> {
    let location = unsafe { location.as_ref() }?;
    if !matches!(
        location.kind,
        D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX | D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT
    ) {
        return None;
    }
    let resource = unsafe { device::resource_proxy(location.resource, device) }?;
    Some((
        CopyLocation {
            resource: resource.native.as_raw(),
            ..*location
        },
        resource.state.placed(),
    ))
}

unsafe extern "system" fn copy_texture(
    list: &List,
    dst: *const CopyLocation,
    x: u32,
    y: u32,
    z: u32,
    src: *const CopyLocation,
    region: *const D3D12_BOX,
    flags: u32,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if flags != 0 {
        crate::diagnostics::recording_error(format_args!("CopyTextureRegion flags {flags:#x}"));
        state.error = E_NOTIMPL;
        return;
    }
    let device = parent(&list.parent);
    let (Some((dst, destination)), Some((src, source))) =
        (unsafe { copy_location(dst, device) }, unsafe {
            copy_location(src, device)
        })
    else {
        state.error = E_INVALIDARG;
        return;
    };
    if let Err(error) = state.prepare_copy(destination, source) {
        state.error = error;
        return;
    }
    unsafe {
        (list.native.vtable().CopyTextureRegion)(
            list.native.as_raw(),
            ptr::from_ref(&dst).cast(),
            x,
            y,
            z,
            ptr::from_ref(&src).cast(),
            region,
        );
    }
}

// Frosty's pool allocator and its buffer and texture copy routines all use
// slot 84 with destination, source and zero options.
unsafe extern "system" fn copy_resource(
    list: &List,
    dst: *mut c_void,
    src: *mut c_void,
    options: u32,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    if options != 0 {
        crate::diagnostics::recording_error(format_args!(
            "slot 84 CopyResource options {options:#x}"
        ));
        state.error = E_NOTIMPL;
        return;
    }
    let owner = parent(&list.parent);
    // SAFETY: each guest argument is a live COM object or null. Only resource
    // proxies owned by the list's device yield borrowed native interfaces.
    let (Some(dst), Some(src)) = (unsafe { device::resource_proxy(dst, owner) }, unsafe {
        device::resource_proxy(src, owner)
    }) else {
        state.error = E_INVALIDARG;
        return;
    };
    let (d, s) = unsafe { (dst.native.GetDesc(), src.native.GetDesc()) };
    if dst.native == src.native
        || d.Dimension != s.Dimension
        || d.DepthOrArraySize != s.DepthOrArraySize
        || d.MipLevels != s.MipLevels
        || d.SampleDesc != s.SampleDesc
        // D3D12 also allows reinterpret copies between some differently sized
        // block-compressed and uncompressed textures. Leave those to the driver.
        || (d.Format == s.Format && (d.Width != s.Width || d.Height != s.Height))
    {
        state.error = E_INVALIDARG;
        return;
    }
    if let Err(error) = state.prepare_copy(dst.state.placed(), src.state.placed()) {
        state.error = error;
        return;
    }
    // SAFETY: both native resources belong to this open command list's device.
    // The guest supplies their copy states via barriers and owns their lifetimes.
    unsafe { list.native.CopyResource(&dst.native, &src.native) };
    static FIRST_COPY: std::sync::Once = std::sync::Once::new();
    crate::diagnostics::log_once(&FIRST_COPY, || {
        format!(
            "recorded slot 84 CopyResource (dimension {}, {} x {} x {}, {} mips)",
            s.Dimension.0, s.Width, s.Height, s.DepthOrArraySize, s.MipLevels
        )
    });
}

// Frosty's RHIDispatchIndirectComputeShader calls slot 59 (RVA 0x57876df).
unsafe extern "system" fn execute_indirect(
    list: &List,
    signature: *mut c_void,
    max_count: u32,
    arguments: *mut c_void,
    argument_offset: u64,
    count_buffer: *mut c_void,
    count_offset: u64,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    let owner = parent(&list.parent);
    // Each object is borrowed from a live guest reference for this call.
    let (Some(signature), Some(arguments)) = (
        unsafe { device::indirect::signature(signature, owner) },
        unsafe { device::resource(arguments, owner) },
    ) else {
        state.error = E_INVALIDARG;
        return;
    };
    let bytes = u64::from(signature.state) * u64::from(max_count);
    if !(argument_offset | count_offset).is_multiple_of(4)
        || !compute::contains_range(arguments, argument_offset, bytes)
    {
        state.error = E_INVALIDARG;
        return;
    }
    let count = if count_buffer.is_null() {
        None
    } else {
        let Some(buffer) = (unsafe { device::resource(count_buffer, owner) })
            .filter(|buffer| compute::contains_range(buffer, count_offset, 4))
        else {
            state.error = E_INVALIDARG;
            return;
        };
        Some(buffer)
    };
    state.sample_heap();
    unsafe {
        list.native.ExecuteIndirect(
            &signature.native,
            max_count,
            arguments,
            argument_offset,
            count,
            count_offset,
        );
    }
}

static LIST_SLOTS: Slots<149> = {
    let mut slots = child_slots::<ID3D12GraphicsCommandList, ListData, 149>();
    slots[8] = crate::slot!(get_type);
    slots[9] = crate::slot!(close);
    slots[10] = crate::slot!(reset);
    slots[12] = crate::slot!(graphics::draw);
    slots[13] = crate::slot!(graphics::draw_indexed);
    slots[14] = crate::slot!(compute::dispatch);
    slots[15] = crate::slot!(compute::copy_buffer);
    slots[20] = crate::slot!(graphics::topology);
    slots[21] = crate::slot!(graphics::viewports);
    slots[22] = crate::slot!(graphics::scissors);
    slots[23] = crate::slot!(graphics::blend_factor);
    slots[24] = crate::slot!(graphics::stencil_ref);
    slots[25] = crate::slot!(graphics::pipeline);
    slots[26] = crate::slot!(transition);
    slots[28] = crate::slot!(queries::query::<true>);
    slots[29] = crate::slot!(queries::query::<false>);
    slots[30] = crate::slot!(queries::resolve);
    // The Xbox root binding slots after ResourceBarrier are shifted by four.
    slots[32] = crate::slot!(descriptors::bind_heaps);
    slots[33] = crate::slot!(compute::bind_root);
    slots[34] = crate::slot!(graphics::root);
    slots[35] = crate::slot!(descriptors::table::<true>);
    slots[36] = crate::slot!(descriptors::table::<false>);
    slots[39] = crate::slot!(compute::constants);
    slots[45] = crate::slot!(compute::unordered_access);
    slots[47] = crate::slot!(graphics::index_buffer);
    slots[48] = crate::slot!(graphics::vertex_buffers);
    slots[50] = crate::slot!(graphics::targets);
    slots[51] = crate::slot!(depth::clear);
    slots[52] = crate::slot!(clear_rtv);
    slots[53] = crate::slot!(depth::clear_uav::<false>);
    slots[54] = crate::slot!(depth::clear_uav::<true>);
    slots[55] = crate::slot!(discard);
    slots[59] = crate::slot!(execute_indirect);
    slots[67] = crate::slot!(flush_texture_cache);
    slots[68] = crate::slot!(graphics::depth_bounds);
    slots[83] = crate::slot!(copy_texture);
    slots[84] = crate::slot!(copy_resource);
    slots[127] = crate::slot!(graphics::dispatch_mesh);
    slots
};

fn reject(queue: &Queue, error: HRESULT) {
    let _ = queue.state.placement.error.compare_exchange(
        S_OK.0,
        error.0,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
    unsafe {
        OutputDebugStringA(s!(
            "Project TuxBox: direct queue rejected a guest command-list batch\n"
        ))
    };
}

unsafe extern "system" fn execute(queue: &Queue, count: u32, lists: *const *mut c_void) {
    let _submission = queue
        .state
        .submission
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if queue.state.placement.error.load(Ordering::Acquire) < 0 || count == 0 {
        return;
    }
    if lists.is_null() {
        reject(queue, E_INVALIDARG);
        return;
    }
    let guest = unsafe { slice::from_raw_parts(lists, count as usize) };
    let result = translate_batch(
        guest,
        (Vec::new(), Vec::new(), Vec::new()),
        |&item, (textures, writes, shader_views)| {
            let list: &List = unsafe {
                unwrap(item, parent(&queue.parent), &LIST_SLOTS)
                    .or_else(|| unwrap(item, parent(&queue.parent), &media::SLOTS))
            }
            .ok_or(E_INVALIDARG)?;
            let state = recording(list);
            if !matches!(state.phase, ListState::Closed)
                || state.error.is_err()
                || unsafe { list.native.GetType() } != queue.state.kind
            {
                return Err(E_INVALIDARG);
            }
            textures
                .try_reserve(state.placed_reads.len())
                .map_err(|_| E_OUTOFMEMORY)?;
            textures.extend(state.placed_reads.iter().cloned());
            writes
                .try_reserve(state.placed_writes.len())
                .map_err(|_| E_OUTOFMEMORY)?;
            writes.extend(state.placed_writes.iter().cloned());
            shader_views.extend(state.sampled_heaps.iter().cloned());
            Ok(list.native.as_raw())
        },
        |host, (mut textures, mut writes, shader_views)| {
            unsafe {
                placement::submit(
                    parent(&queue.parent),
                    &queue.native,
                    &queue.state.placement,
                    &mut textures,
                    &mut writes,
                    &shader_views,
                    || {
                        (queue.native.vtable().ExecuteCommandLists)(
                            queue.native.as_raw(),
                            count,
                            host.as_ptr(),
                        );
                        crate::diagnostics::submitted(queue.state.kind, count);
                    },
                )
            }
            .map_err(|error| error.code())
        },
    );
    if let Err(error) = result {
        reject(queue, error);
    }
}

unsafe extern "system" fn queue_fence<const WAIT: bool>(
    queue: &Queue,
    fence: *mut c_void,
    value: u64,
) -> HRESULT {
    let _submission = queue
        .state
        .submission
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let hr = HRESULT(queue.state.placement.error.load(Ordering::Acquire));
    if hr.is_err() {
        return hr;
    }
    let owner = parent(&queue.parent);
    let native;
    let fence = match unsafe { device::fence(fence, owner) } {
        Some(fence) => fence,
        None => {
            native = match unsafe { device::media::native_child::<ID3D12Fence>(fence, owner) } {
                Ok(fence) => fence,
                Err(error) => return error.code(),
            };
            &native
        }
    };
    let method = if WAIT {
        queue.native.vtable().Wait
    } else {
        queue.native.vtable().Signal
    };
    unsafe { method(queue.native.as_raw(), fence.as_raw(), value) }
}

// Frosty's direct/compute path requests its flush before the fence and the
// copy path passes zero. Native queue Signal orders completion of prior work.
unsafe extern "system" fn signal_x(
    queue: &Queue,
    fence: *mut c_void,
    value: u64,
    flush: u32,
) -> HRESULT {
    if !matches!(flush, 0 | 0x0800_0000) {
        return E_NOTIMPL;
    }
    unsafe { queue_fence::<false>(queue, fence, value) }
}

#[repr(C)]
struct PresentPlane {
    token: u64,
    resource_count: u32,
    padding: u32,
    resources: *const *mut c_void,
}

const _: () = assert!(mem::offset_of!(PresentPlane, resources) == 16);

// Frosty submits a 72-byte plane and a separate 32-byte parameter record
// at RVA 0x5764f32. Older TuxBox guests supply only the plane prefix.
#[repr(C)]
struct RetailPresentPlane {
    prefix: PresentPlane,
    options: [u32; 12],
}

#[repr(C)]
struct PresentParameters {
    immediate_threshold_percent: f32,
    options: [u32; 7],
}

const _: () = assert!(mem::size_of::<RetailPresentPlane>() == 72);
const _: () = assert!(mem::size_of::<PresentParameters>() == 32);

unsafe extern "system" fn present(
    queue: &Queue,
    count: u32,
    planes: *const PresentPlane,
    params: *const PresentParameters,
) -> HRESULT {
    let _submission = queue
        .state
        .submission
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let hr = HRESULT(queue.state.placement.error.load(Ordering::Acquire));
    if hr.is_err() {
        return hr;
    }
    if queue.state.kind != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return E_NOTIMPL;
    }
    if count == 0 || planes.is_null() {
        return E_INVALIDARG;
    }
    if count != 1 {
        return E_NOTIMPL;
    }
    if let Some(params) = unsafe { params.as_ref() } {
        // The threshold limits when a late frame may tear. The synchronized
        // swap chain always waits for VBlank and cannot exceed that limit.
        if !(0.0..=100.0).contains(&params.immediate_threshold_percent) {
            return E_INVALIDARG;
        }
        // SAFETY: A caller using present parameters supplies a complete SDK
        // plane record. Null-parameter guests retain their 24-byte prefix ABI.
        let full_plane = unsafe { &*planes.cast::<RetailPresentPlane>() };
        if params.options != [0; 7] || full_plane.options != [0; 12] {
            return E_NOTIMPL;
        }
    }
    let plane = unsafe { &*planes };
    if plane.resource_count == 0 || plane.resources.is_null() {
        return E_INVALIDARG;
    }
    if plane.resource_count != 1 {
        return E_NOTIMPL;
    }
    let Some(source) = (unsafe { device::resource(*plane.resources, parent(&queue.parent)) })
    else {
        return E_INVALIDARG;
    };
    let result = queue.state.presentation.present(&queue.native, source);
    if result == S_OK {
        unsafe { device::frame_events::display_flip(parent(&queue.parent), plane.token) };
    }
    result
}

static QUEUE_SLOTS: Slots<64> = {
    let mut slots = child_slots::<ID3D12CommandQueue, QueueState, 64>();
    slots[10] = crate::slot!(execute);
    slots[14] = crate::slot!(queue_fence::<false>);
    slots[15] = crate::slot!(queue_fence::<true>);
    slots[16] = crate::slot!(queries::frequency);
    slots[17] = crate::slot!(queries::calibrate);
    slots[23] = crate::slot!(signal_x);
    slots[30] = crate::slot!(present);
    slots
};

pub(crate) unsafe fn wrap_command_list(
    native: ID3D12GraphicsCommandList,
    allocator: &device::Allocator,
    initial: Option<&ID3D12PipelineState>,
    device: &IUnknown,
    compact: bool,
    out: *mut *mut c_void,
) -> HRESULT {
    let state = ListData {
        reserved0: [0; 2],
        write_cursor: 0,
        reserved1: [0; 2],
        packet_end: 0,
        reserved2: [0; 0x540 - 0x38],
        pipeline_packets: core::cell::UnsafeCell::new([0; 32]),
        extended: OnceLock::new(),
        enhanced_barriers: OnceLock::new(),
        mesh: OnceLock::new(),
        recording: Mutex::new(Recording {
            placed_reads: Vec::new(),
            placed_writes: Vec::new(),
            resource_heap: None,
            sampled_heaps: Vec::new(),
            media: compact.then(|| media::Recording::new(allocator)),
            phase: ListState::Fresh(allocator.native.clone(), initial.cloned()),
            error: S_OK,
        }),
    };
    unsafe {
        allocate(
            if compact { &media::SLOTS } else { &LIST_SLOTS },
            if compact {
                &[
                    ID3D12DeviceChild::IID,
                    ID3D12CommandList::IID,
                    device::media::DMA_IID,
                ]
            } else {
                &[
                    ID3D12DeviceChild::IID,
                    ID3D12CommandList::IID,
                    ID3D12GraphicsCommandList::IID,
                ]
            },
            native,
            Some(device),
            state,
            out,
        )
    }
}

pub(crate) unsafe fn wrap_command_queue(
    native: ID3D12CommandQueue,
    device: &IUnknown,
    out: *mut *mut c_void,
) -> HRESULT {
    let state = QueueState {
        reserved: 0,
        kind: unsafe { native.GetDesc() }.Type,
        placement: placement::QueueAccess::default(),
        submission: Mutex::new(()),
        presentation: Presentation::default(),
    };
    unsafe {
        allocate(
            &QUEUE_SLOTS,
            &[
                ID3D12DeviceChild::IID,
                ID3D12Pageable::IID,
                ID3D12CommandQueue::IID,
            ],
            native,
            Some(device),
            state,
            out,
        )
    }
}
