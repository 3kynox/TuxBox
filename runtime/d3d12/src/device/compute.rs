//! Wrapped root signatures and the Xbox extended compute pipeline factory.
use super::{Device, Out, identity, wrap_pageable};
use crate::proxy::{Proxy, Slots, allocate, check_output, child_slots, unwrap};
use core::{ffi::c_void, mem::ManuallyDrop, slice};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{GUID, HRESULT, Interface};

static ROOT: Slots<8> = child_slots::<ID3D12RootSignature, (), 8>();
// Inline console SetPipelineState copies 32 bytes starting at +0x10 from
// this object to the guest command list. Desktop command lists use slot 25
// with an empty packet buffer, so no Xbox command packets are advertised.
#[repr(C)]
#[derive(Default)]
pub(crate) struct PipelineData {
    reserved: usize,
    packets: [u8; 32],
    pub(crate) stencil_slot: Option<u32>,
}

const _: () = assert!(
    core::mem::offset_of!(Proxy<ID3D12PipelineState, PipelineData>, state)
        + core::mem::offset_of!(PipelineData, packets)
        == 0x10
);
pub(super) static PIPELINE: Slots<9> = child_slots::<ID3D12PipelineState, PipelineData, 9>();

// A null root signature is valid for a shader with an embedded signature.
// The caller keeps every non-null guest object alive for the returned borrow.
pub(crate) unsafe fn root_signature<'a>(
    guest: *mut c_void,
    device: *mut c_void,
) -> Result<Option<&'a ID3D12RootSignature>, HRESULT> {
    if guest.is_null() {
        return Ok(None);
    }
    // SAFETY: unwrap checks both the guest vtable and device identity.
    unsafe { unwrap::<ID3D12RootSignature, ()>(guest, device, &ROOT) }
        .map(|object| Some(&object.native))
        .ok_or(E_INVALIDARG)
}

pub(crate) unsafe fn pipeline<'a>(
    guest: *mut c_void,
    device: *mut c_void,
) -> Result<Option<&'a Proxy<ID3D12PipelineState, PipelineData>>, HRESULT> {
    if guest.is_null() {
        return Ok(None);
    }
    // SAFETY: as with root_signature, the caller owns the live guest reference.
    unsafe { unwrap::<ID3D12PipelineState, PipelineData>(guest, device, &PIPELINE) }
        .map(Some)
        .ok_or(E_INVALIDARG)
}

pub(super) unsafe extern "system" fn create_root_signature(
    this: &Device,
    node: u32,
    blob: *const u8,
    length: usize,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    // SAFETY: the guest supplies a readable IID and writable output slot.
    let hr = unsafe { check_output::<ID3D12RootSignature>(iid, out) };
    if hr.is_err() {
        return hr;
    }
    if blob.is_null() || length == 0 || length > isize::MAX as usize {
        return E_INVALIDARG;
    }
    // SAFETY: serialized signature bytes are borrowed only for this call.
    let bytes = unsafe { slice::from_raw_parts(blob, length) };
    match unsafe {
        this.native
            .CreateRootSignature::<ID3D12RootSignature>(node, bytes)
    } {
        // SAFETY: the returned guest child owns both its native interface and device.
        Ok(native) => unsafe {
            allocate(
                &ROOT,
                &[ID3D12DeviceChild::IID, ID3D12RootSignature::IID],
                native,
                Some(&identity(this)),
                (),
                out,
            )
        },
        Err(error) => error.code(),
    }
}

// Slot 62 takes a compute descriptor followed by an array of Xbox extensions.
// Portable DXIL and a standard serialized root signature work with zero extensions.
pub(super) unsafe extern "system" fn create_pipeline_x(
    this: &Device,
    desc: *const D3D12_COMPUTE_PIPELINE_STATE_DESC,
    extension_count: u32,
    _extensions: *const c_void,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    // SAFETY: the guest supplies a readable IID and writable output slot.
    let hr = unsafe { check_output::<ID3D12PipelineState>(iid, out) };
    if hr.is_err() {
        return hr;
    }
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_INVALIDARG;
    };
    if extension_count != 0 || desc.CachedPSO.CachedBlobSizeInBytes != 0 {
        return E_NOTIMPL;
    }
    if desc.CS.pShaderBytecode.is_null()
        || desc.CS.BytecodeLength == 0
        || desc.CS.BytecodeLength > isize::MAX as usize
    {
        return E_INVALIDARG;
    }
    let guest = desc
        .pRootSignature
        .as_ref()
        .map_or(core::ptr::null_mut(), Interface::as_raw);
    // SAFETY: the descriptor borrows its root signature for pipeline creation.
    let root = match unsafe { root_signature(guest, identity(this).as_raw()) } {
        Ok(root) => root,
        Err(error) => return error,
    };
    // SAFETY: the guest retains its shader bytes until this synchronous call returns.
    let bytes =
        unsafe { slice::from_raw_parts(desc.CS.pShaderBytecode.cast(), desc.CS.BytecodeLength) };
    let translation = super::shader_cache::prepared(bytes);
    let code = translation.as_deref().unwrap_or(bytes);
    let mut host = D3D12_COMPUTE_PIPELINE_STATE_DESC {
        pRootSignature: ManuallyDrop::new(root.cloned()),
        CS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: code.as_ptr().cast(),
            BytecodeLength: code.len(),
        },
        NodeMask: desc.NodeMask,
        Flags: desc.Flags,
        ..Default::default()
    };
    // SAFETY: native creation consumes the descriptor synchronously. The owned
    // clone in the C layout must be released explicitly on success and failure.
    let create = |descriptor: &D3D12_COMPUTE_PIPELINE_STATE_DESC| unsafe {
        this.native
            .CreateComputePipelineState::<ID3D12PipelineState>(descriptor)
    };
    let mut native = create(&host);
    if translation.is_some() && native.is_err() {
        // Structural checks cannot validate DXIL instructions or root bindings.
        // Let the adapter handle the original guest shader if it rejects the cache.
        host.CS = desc.CS;
        native = create(&host);
    }
    unsafe { ManuallyDrop::drop(&mut host.pRootSignature) };
    if let Err(error) = &native {
        crate::diagnostics::compute_failure(bytes, error.code().0);
    }
    unsafe { wrap_pageable(native, &PIPELINE, this, PipelineData::default(), out) }
}
