//! Device memory heaps and resources sharing their native GPU allocation.
use super::{Device, Out, RESOURCE, identity, wrap_pageable};
use crate::proxy::{Proxy, Slots, check_output, child_slots, unwrap};
use core::ffi::c_void;
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, E_POINTER};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{GUID, HRESULT, Interface};

type Heap = Proxy<ID3D12Heap, D3D12_HEAP_DESC>;
unsafe extern "system" fn get_desc(heap: &Heap, out: *mut D3D12_HEAP_DESC) -> *mut D3D12_HEAP_DESC {
    unsafe { out.write(heap.state) };
    out
}
static HEAP: Slots<9> = {
    let mut table = child_slots::<ID3D12Heap, D3D12_HEAP_DESC, 9>();
    table[8] = crate::slot!(get_desc);
    table
};

pub(super) unsafe extern "system" fn create(
    device: &Device,
    desc: *const D3D12_HEAP_DESC,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12Heap>(iid, out) };
    if status.is_err() {
        return status;
    }
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_INVALIDARG;
    };
    if desc.SizeInBytes == 0 {
        return E_INVALIDARG;
    }
    if !matches!(
        desc.Properties.Type,
        D3D12_HEAP_TYPE_DEFAULT | D3D12_HEAP_TYPE_UPLOAD | D3D12_HEAP_TYPE_READBACK
    ) {
        return E_NOTIMPL;
    }
    let mut native = None;
    if let Err(error) = unsafe { device.native.CreateHeap::<ID3D12Heap>(desc, &mut native) } {
        return error.code();
    }
    let Some(native) = native else {
        return E_POINTER;
    };
    // GetDesc reflects the size and alignment the driver actually granted.
    let actual = unsafe { native.GetDesc() };
    unsafe { wrap_pageable(Ok(native), &HEAP, device, actual, out) }
}

pub(super) unsafe extern "system" fn place(
    device: &Device,
    heap: *mut c_void,
    offset: u64,
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
    let Some(heap): Option<&Heap> = (unsafe { unwrap(heap, identity(device).as_raw(), &HEAP) })
    else {
        return E_INVALIDARG;
    };
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_INVALIDARG;
    };
    if !matches!(
        desc.Dimension,
        D3D12_RESOURCE_DIMENSION_BUFFER
            | D3D12_RESOURCE_DIMENSION_TEXTURE1D
            | D3D12_RESOURCE_DIMENSION_TEXTURE2D
            | D3D12_RESOURCE_DIMENSION_TEXTURE3D
    ) {
        return E_NOTIMPL;
    }
    // A guest byte offset has to fit the host allocation, including padding.
    let primary = super::depth::primary(desc);
    if primary.is_some()
        && (heap.state.Properties.Type != D3D12_HEAP_TYPE_DEFAULT
            || initial.contains(D3D12_RESOURCE_STATE_UNORDERED_ACCESS))
    {
        return E_NOTIMPL;
    }
    let host = primary.as_ref().unwrap_or(desc);
    let info = unsafe {
        device
            .native
            .GetResourceAllocationInfo(0, core::slice::from_ref(host))
    };
    if info.SizeInBytes == u64::MAX
        || info.Alignment == 0
        || !offset.is_multiple_of(info.Alignment)
        || offset
            .checked_add(info.SizeInBytes)
            .is_none_or(|end| end > heap.state.SizeInBytes)
    {
        return E_INVALIDARG;
    }
    let mut native = None;
    if let Err(error) = unsafe {
        device.native.CreatePlacedResource::<_, ID3D12Resource>(
            &heap.native,
            offset,
            host,
            super::host_state(initial, desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER),
            Some(clear),
            &mut native,
        )
    } {
        return error.code();
    }
    unsafe {
        super::depth::wrap(
            device,
            &heap.state.Properties,
            desc,
            primary.as_ref(),
            native,
            out,
        )
    }
}

// Slot 44 places a tiled resource at a title-selected virtual address. An
// unrelated allocation would lose texture contents and subsequent page remaps.
pub(super) unsafe extern "system" fn place_virtual(
    device: &Device,
    address: u64,
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
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_INVALIDARG;
    };
    if address == 0 {
        return E_INVALIDARG;
    }
    let state = super::host_state(initial, desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER);
    if clear.is_null()
        && let Some(result) = unsafe { super::media::place(device, address, desc, state, out) }
    {
        return result;
    }
    let result = super::placement::Texture::new(&device.native, address, desc, state, clear);
    match result {
        Ok(texture) => {
            let native = texture.native.clone();
            return unsafe {
                wrap_pageable(
                    Ok(native),
                    &RESOURCE,
                    device,
                    super::Backing::Placed(texture),
                    out,
                )
            };
        }
        Err(error) if error.code() != E_NOTIMPL => return error.code(),
        Err(_) => (),
    }
    static REPORTED: std::sync::Once = std::sync::Once::new();
    REPORTED.call_once(|| {
        let message = format!("Project TuxBox: unsupported CreatePlacedResourceX at {address:#x}, dim={} {}x{}x{} mips={} format={} layout={:#x} flags={:#x} state={:#x}\n\0",
            desc.Dimension.0, desc.Width, desc.Height, desc.DepthOrArraySize, desc.MipLevels, desc.Format.0,
            desc.Layout.0, desc.Flags.0, initial.0);
        unsafe { windows::Win32::System::Diagnostics::Debug::OutputDebugStringA(windows::core::PCSTR(message.as_ptr())) };
    });
    E_NOTIMPL
}
