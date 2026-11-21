//! Guest command lists and queues. Embedded guest COM pointers are translated
//! before a batch reaches the desktop API.

use core::{ffi::c_void, mem, ptr, slice};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, MutexGuard};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, RECT, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringA;
use windows::core::{HRESULT, IUnknown, Interface, s};

use crate::device;
use crate::present::Presentation;
use crate::proxy::{Proxy, Slots, allocate, child_slots, unwrap};

struct Recording {
    closed: bool,
    error: HRESULT,
}

impl Recording {
    fn can_record(&mut self) -> bool {
        if self.closed {
            self.error = E_FAIL;
        }
        self.error.is_ok()
    }
}

type List = Proxy<ID3D12GraphicsCommandList, Mutex<Recording>>;

fn recording(list: &List) -> MutexGuard<'_, Recording> {
    list.state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

struct QueueState {
    kind: D3D12_COMMAND_LIST_TYPE,
    // ExecuteCommandLists is void. Report a rejected batch on the next fence
    // signal or present, and never signal completion for missing work.
    error: AtomicI32,
    presentation: Presentation,
}

type Queue = Proxy<ID3D12CommandQueue, QueueState>;

fn parent(device: &Option<IUnknown>) -> *mut c_void {
    device.as_ref().map_or(ptr::null_mut(), Interface::as_raw)
}

// Validate and translate the entire batch before submission. Small batches use
// stack storage and larger ones can report E_OUTOFMEMORY to the guest.
fn translate_batch<T: Default, G>(
    guest: &[G],
    translate: impl Fn(&G) -> Result<T, HRESULT>,
    submit: impl FnOnce(&[T]),
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
        *dst = translate(src)?;
    }
    submit(host);
    Ok(())
}

unsafe extern "system" fn get_type(list: &List) -> D3D12_COMMAND_LIST_TYPE {
    unsafe { list.native.GetType() }
}

unsafe extern "system" fn reset(
    list: &List,
    allocator: *mut c_void,
    initial: *mut c_void,
) -> HRESULT {
    if !initial.is_null() {
        return E_NOTIMPL;
    }
    let Some(allocator) = (unsafe { device::allocator(allocator, parent(&list.parent)) }) else {
        return E_INVALIDARG;
    };
    let mut state = recording(list);
    if !state.closed {
        return E_FAIL;
    }
    let native = &list.native;
    let hr =
        unsafe { (native.vtable().Reset)(native.as_raw(), allocator.as_raw(), ptr::null_mut()) };
    if hr.is_ok() {
        state.closed = false;
        state.error = S_OK;
    }
    hr
}

unsafe extern "system" fn close(list: &List) -> HRESULT {
    let mut state = recording(list);
    if state.closed {
        return E_FAIL;
    }
    let native = &list.native;
    let hr = unsafe { (native.vtable().Close)(native.as_raw()) };
    state.closed = true;
    if hr.is_err() {
        state.error = hr;
    }
    state.error
}

// The COM pointer in a barrier or copy location is borrowed by the desktop
// call. Raw fields allow us to replace it without taking an owned reference.
#[repr(C)]
#[derive(Default)]
struct TransitionBarrier {
    kind: D3D12_RESOURCE_BARRIER_TYPE,
    flags: D3D12_RESOURCE_BARRIER_FLAGS,
    resource: *mut c_void,
    subresource: u32,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CopyLocation {
    resource: *mut c_void,
    kind: D3D12_TEXTURE_COPY_TYPE,
    data: D3D12_TEXTURE_COPY_LOCATION_0,
}

const _: () = {
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
    if !state.can_record() || count == 0 {
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
        |barrier| {
            if barrier.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION
                || barrier.Flags != D3D12_RESOURCE_BARRIER_FLAG_NONE
            {
                return Err(E_NOTIMPL);
            }
            let mut host = unsafe { ptr::from_ref(barrier).cast::<TransitionBarrier>().read() };
            let resource = unsafe { device::resource(host.resource, parent(&list.parent)) }
                .ok_or(E_INVALIDARG)?;
            host.resource = resource.as_raw();
            Ok(host)
        },
        |host| unsafe {
            (list.native.vtable().ResourceBarrier)(
                list.native.as_raw(),
                count,
                host.as_ptr().cast(),
            );
        },
    );
    if let Err(error) = result {
        state.error = error;
    }
}

unsafe extern "system" fn clear_rtv(
    list: &List,
    handle: D3D12_CPU_DESCRIPTOR_HANDLE,
    color: *const f32,
    count: u32,
    rects: *const RECT,
) {
    let mut state = recording(list);
    if !state.can_record() {
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

unsafe fn copy_location(
    location: *const CopyLocation,
    device: *mut c_void,
) -> Option<CopyLocation> {
    let location = unsafe { location.as_ref() }?;
    if !matches!(
        location.kind,
        D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX | D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT
    ) {
        return None;
    }
    let resource = unsafe { device::resource(location.resource, device) }?;
    Some(CopyLocation {
        resource: resource.as_raw(),
        ..*location
    })
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
    if !state.can_record() {
        return;
    }
    if flags != 0 {
        state.error = E_NOTIMPL;
        return;
    }
    let device = parent(&list.parent);
    let (Some(dst), Some(src)) = (unsafe { copy_location(dst, device) }, unsafe {
        copy_location(src, device)
    }) else {
        state.error = E_INVALIDARG;
        return;
    };
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

static LIST_SLOTS: Slots<149> = {
    let mut slots = child_slots::<ID3D12GraphicsCommandList, Mutex<Recording>, 149>();
    slots[8] = crate::slot!(get_type);
    slots[9] = crate::slot!(close);
    slots[10] = crate::slot!(reset);
    slots[26] = crate::slot!(transition);
    slots[52] = crate::slot!(clear_rtv);
    slots[83] = crate::slot!(copy_texture);
    slots
};

fn reject(queue: &Queue, error: HRESULT) {
    let _ =
        queue
            .state
            .error
            .compare_exchange(S_OK.0, error.0, Ordering::AcqRel, Ordering::Acquire);
    unsafe {
        OutputDebugStringA(s!(
            "Project TuxBox: direct queue rejected a guest command-list batch\n"
        ))
    };
}

unsafe extern "system" fn execute(queue: &Queue, count: u32, lists: *const *mut c_void) {
    if queue.state.error.load(Ordering::Acquire) < 0 || count == 0 {
        return;
    }
    if lists.is_null() {
        reject(queue, E_INVALIDARG);
        return;
    }
    let guest = unsafe { slice::from_raw_parts(lists, count as usize) };
    let result = translate_batch(
        guest,
        |&item| {
            let list: &List =
                unsafe { unwrap(item, parent(&queue.parent), &LIST_SLOTS) }.ok_or(E_INVALIDARG)?;
            let state = recording(list);
            if !state.closed
                || state.error.is_err()
                || unsafe { list.native.GetType() } != queue.state.kind
            {
                return Err(E_INVALIDARG);
            }
            Ok(list.native.as_raw())
        },
        |host| unsafe {
            (queue.native.vtable().ExecuteCommandLists)(
                queue.native.as_raw(),
                count,
                host.as_ptr(),
            );
        },
    );
    if let Err(error) = result {
        reject(queue, error);
    }
}

unsafe extern "system" fn signal(queue: &Queue, fence: *mut c_void, value: u64) -> HRESULT {
    let hr = HRESULT(queue.state.error.load(Ordering::Acquire));
    if hr.is_err() {
        return hr;
    }
    let Some(fence) = (unsafe { device::fence(fence, parent(&queue.parent)) }) else {
        return E_INVALIDARG;
    };
    unsafe { (queue.native.vtable().Signal)(queue.native.as_raw(), fence.as_raw(), value) }
}

#[repr(C)]
struct PresentPlane {
    token: u64,
    resource_count: u32,
    padding: u32,
    resources: *const *mut c_void,
}

const _: () = assert!(mem::offset_of!(PresentPlane, resources) == 16);

unsafe extern "system" fn present(
    queue: &Queue,
    count: u32,
    planes: *const PresentPlane,
    params: *const c_void,
) -> HRESULT {
    let hr = HRESULT(queue.state.error.load(Ordering::Acquire));
    if hr.is_err() {
        return hr;
    }
    if queue.state.kind != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return E_NOTIMPL;
    }
    if count == 0 || planes.is_null() {
        return E_INVALIDARG;
    }
    if count != 1 || !params.is_null() {
        return E_NOTIMPL;
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
    queue.state.presentation.present(&queue.native, source)
}

static QUEUE_SLOTS: Slots<64> = {
    let mut slots = child_slots::<ID3D12CommandQueue, QueueState, 64>();
    slots[10] = crate::slot!(execute);
    slots[14] = crate::slot!(signal);
    slots[30] = crate::slot!(present);
    slots
};

pub(crate) unsafe fn wrap_command_list(
    native: ID3D12GraphicsCommandList,
    device: &IUnknown,
    out: *mut *mut c_void,
) -> HRESULT {
    let state = Mutex::new(Recording {
        closed: true,
        error: S_OK,
    });
    unsafe {
        allocate(
            &LIST_SLOTS,
            &[
                ID3D12DeviceChild::IID,
                ID3D12CommandList::IID,
                ID3D12GraphicsCommandList::IID,
            ],
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
        kind: unsafe { native.GetDesc() }.Type,
        error: AtomicI32::new(S_OK.0),
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
