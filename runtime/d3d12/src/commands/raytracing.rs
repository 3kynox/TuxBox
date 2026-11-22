//! Xbox list slot 119 records AS copies and deserializes cooked console geometry.
use super::{List, Recording, recording};
use crate::device::blas;
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::{IUnknown, Result};

pub(super) fn retain(recording: &Recording, resources: &[&ID3D12Resource]) -> Result<()> {
    let mut held = recording
        .resources
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    held.try_reserve(resources.len())
        .map_err(|_| E_OUTOFMEMORY)?;
    held.extend(resources.iter().map(|r| (*r).clone()));
    Ok(())
}

fn record(
    state: &mut Recording,
    host: &ID3D12GraphicsCommandList4,
    owner: &IUnknown,
    destination: u64,
    source: u64,
    mode: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE,
) -> Result<()> {
    let alignment = u64::from(D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BYTE_ALIGNMENT);
    if source == 0
        || destination == 0
        || !source.is_multiple_of(alignment)
        || !destination.is_multiple_of(alignment)
    {
        return Err(E_INVALIDARG.into());
    }
    match mode {
        D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE_CLONE
        | D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE_COMPACT => {
            // SAFETY: owner is the controlling device retained by the list.
            let source_as = unsafe { blas::resolve(owner, source) };
            let (src, dst) = if let Some(backing) = source_as {
                let (dst, va) = unsafe { blas::copy_dest(owner, destination, &backing) }?;
                retain(state, &[&backing.resource, &dst])?;
                (unsafe { backing.resource.GetGPUVirtualAddress() }, va)
            } else if unsafe { blas::resolve(owner, destination) }.is_some() {
                return Err(E_NOTIMPL.into()); // unknown native source size into a relocated destination
            } else {
                (source, destination)
            };
            unsafe { host.CopyRaytracingAccelerationStructure(dst, src, mode) };
        }
        D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE_DESERIALIZE => {
            state
                .blas_inputs
                .try_reserve(1)
                .map_err(|_| E_OUTOFMEMORY)?;
            let result = unsafe { blas::deserialize(owner, source, destination) }?;
            retain(state, &[&result.source, &result.destination])?;
            state.blas_inputs.push(result.input);
            // SAFETY: the private build is complete, the copy fits the checked
            // guest suballocation or its dedicated backing. The guest allocator
            // retains both resources until all executions using it have completed.
            unsafe {
                host.CopyRaytracingAccelerationStructure(
                    result.destination_address,
                    result.source.GetGPUVirtualAddress(),
                    D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE_COMPACT,
                )
            };
        }
        _ => return Err(E_NOTIMPL.into()),
    }
    Ok(())
}

pub(super) unsafe extern "system" fn copy(
    list: &List,
    destination: u64,
    source: u64,
    mode: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    let Some(host) = &list.state.raytracing else {
        state.error = E_NOTIMPL;
        return;
    };
    let Some(owner) = &list.parent else {
        state.error = E_POINTER;
        return;
    };
    if let Err(error) = record(&mut state, host, owner, destination, source, mode) {
        crate::diagnostics::recording_error(format_args!(
            "slot 119 AS copy mode {}, source {source:#x}, destination {destination:#x}: {:?}",
            mode.0,
            error.code()
        ));
        state.error = error.code();
    }
}

// FD3D12RayTracingCompactionRequestHandler::Update passes a destination,
// count and array of BLAS GPU addresses to Xbox list slot 118 (RVA 0x57a95d5).
pub(super) unsafe extern "system" fn postbuild(
    list: &List,
    desc: *const D3D12_RAYTRACING_ACCELERATION_STRUCTURE_POSTBUILD_INFO_DESC,
    count: u32,
    addresses: *const u64,
) {
    let mut state = recording(list);
    if !state.can_record(&list.native) {
        return;
    }
    let result = (|| -> Result<()> {
        let desc = unsafe { desc.as_ref() }.ok_or(E_POINTER)?;
        let host = list.state.raytracing.as_ref().ok_or(E_NOTIMPL)?;
        let owner = list.parent.as_ref().ok_or(E_POINTER)?;
        if !matches!(
            desc.InfoType,
            D3D12_RAYTRACING_ACCELERATION_STRUCTURE_POSTBUILD_INFO_COMPACTED_SIZE
                | D3D12_RAYTRACING_ACCELERATION_STRUCTURE_POSTBUILD_INFO_CURRENT_SIZE
        ) {
            return Err(E_NOTIMPL.into());
        }
        if desc.DestBuffer == 0 || !desc.DestBuffer.is_multiple_of(8) {
            return Err(E_INVALIDARG.into());
        }
        if count == 0 {
            return Ok(());
        }
        if addresses.is_null() {
            return Err(E_POINTER.into());
        }
        let sources = unsafe { std::slice::from_raw_parts(addresses, count as usize) };
        let alignment = u64::from(D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BYTE_ALIGNMENT);
        if sources
            .iter()
            .any(|&va| va == 0 || !va.is_multiple_of(alignment))
        {
            return Err(E_INVALIDARG.into());
        }
        let mut patched = Vec::new();
        if unsafe { blas::has_relocations(owner) } {
            patched
                .try_reserve_exact(sources.len())
                .map_err(|_| E_OUTOFMEMORY)?;
            for &va in sources {
                if let Some(target) = unsafe { blas::resolve(owner, va) } {
                    retain(&state, &[&target.resource])?;
                    patched.push(unsafe { target.resource.GetGPUVirtualAddress() });
                } else {
                    patched.push(va);
                }
            }
        }
        let native = if patched.is_empty() {
            sources
        } else {
            &patched
        };
        unsafe { host.EmitRaytracingAccelerationStructurePostbuildInfo(desc, native) };
        Ok(())
    })();
    if let Err(error) = result {
        state.error = error.code();
        crate::diagnostics::recording_error(format_args!(
            "slot 118 AS size query: {:?}",
            error.code()
        ));
    }
}
