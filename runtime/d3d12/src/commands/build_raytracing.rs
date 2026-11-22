//! Xbox List6 batches 48-byte DXR build descriptions with 32 bytes of extensions.
use super::{List, Recording, raytracing::retain, recording};
use crate::device::{blas, ray_build};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::{Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess};
use windows::core::{GUID, IUnknown, Result};

pub(super) const LIST6: GUID = GUID::from_u128(0xc3827890_4024_474c_98cf_645b51dc4874);

#[repr(C)]
pub(super) struct Entry {
    build: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC,
    extension: [u64; 4],
}
const _: () = assert!(size_of::<Entry>() == 80);

// Upload-backed instance records can be edited after Close and before each Execute.
// Keep a separate upload so the guest always sees its original GPU addresses.
pub(super) enum Instances {
    Direct,
    Gpu(blas::instance_fixup::Instances),
    Upload {
        address: u64,
        count: usize,
        indirect: bool,
        patched: ID3D12Resource,
    },
}

const INSTANCE_BYTES: usize = size_of::<D3D12_RAYTRACING_INSTANCE_DESC>();
const AS_OFFSET: usize =
    core::mem::offset_of!(D3D12_RAYTRACING_INSTANCE_DESC, AccelerationStructure);
const _: () = assert!(INSTANCE_BYTES == 64 && AS_OFFSET == 56);

fn read_upload(owner: &IUnknown, address: u64, out: &mut [u8]) -> Result<()> {
    let (buffer, offset, available) = unsafe { blas::upload_range(owner, address)? };
    if available < out.len() as u64 {
        return Err(E_INVALIDARG.into());
    }
    let begin = offset as usize;
    let range = D3D12_RANGE {
        Begin: begin,
        End: begin + out.len(),
    };
    let mut data = core::ptr::null_mut();
    unsafe { buffer.Map(0, Some(&range), Some(&mut data))? };
    let mut copied = 0;
    let result = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            data.cast::<u8>().add(begin).cast(),
            out.as_mut_ptr().cast(),
            out.len(),
            Some(&mut copied),
        )
    };
    unsafe { buffer.Unmap(0, Some(&D3D12_RANGE::default())) };
    result?;
    if copied != out.len() {
        return Err(E_INVALIDARG.into());
    }
    Ok(())
}

impl Instances {
    pub(super) unsafe fn prepare(&self, owner: &IUnknown) -> Result<()> {
        if let Self::Gpu(instances) = self {
            return unsafe { instances.prepare(owner) };
        }
        let Self::Upload {
            address,
            count,
            indirect,
            patched,
        } = self
        else {
            // Foreign addresses and GPU arrays of pointers have no safely resolved
            // input range. Native traversal remains valid until a BLAS moves.
            if unsafe { blas::has_relocations(owner) } {
                return Err(E_NOTIMPL.into());
            }
            return Ok(());
        };
        let bytes = *count * INSTANCE_BYTES;
        let mut records = Vec::new();
        records
            .try_reserve_exact(bytes)
            .map_err(|_| E_OUTOFMEMORY)?;
        records.resize(bytes, 0u8);
        if *indirect {
            let mut pointers = Vec::new();
            let pointer_bytes = count * 8;
            pointers
                .try_reserve_exact(pointer_bytes)
                .map_err(|_| E_OUTOFMEMORY)?;
            pointers.resize(pointer_bytes, 0u8);
            read_upload(owner, *address, &mut pointers)?;
            for (ptr, record) in pointers
                .chunks_exact(8)
                .zip(records.chunks_exact_mut(INSTANCE_BYTES))
            {
                let va = u64::from_le_bytes(ptr.try_into().unwrap());
                if !va.is_multiple_of(u64::from(D3D12_RAYTRACING_INSTANCE_DESCS_BYTE_ALIGNMENT)) {
                    return Err(E_INVALIDARG.into());
                }
                read_upload(owner, va, record)?;
            }
        } else {
            read_upload(owner, *address, &mut records)?;
        }
        let relocated = unsafe { blas::relocated(owner) };
        for record in records.chunks_exact_mut(INSTANCE_BYTES) {
            let field = &mut record[AS_OFFSET..];
            let address = u64::from_le_bytes((*field).try_into().unwrap());
            if let Ok(index) = relocated.binary_search_by_key(&address, |&(guest, _)| guest) {
                field.copy_from_slice(&relocated[index].1.to_le_bytes());
            }
        }
        let mut dest = core::ptr::null_mut();
        unsafe {
            patched.Map(0, Some(&D3D12_RANGE::default()), Some(&mut dest))?;
            core::ptr::copy_nonoverlapping(records.as_ptr(), dest.cast(), bytes);
            patched.Unmap(
                0,
                Some(&D3D12_RANGE {
                    Begin: 0,
                    End: bytes,
                }),
            );
        }
        Ok(())
    }
}

fn translate(
    state: &mut Recording,
    owner: &IUnknown,
    host: &ID3D12GraphicsCommandList4,
    mut desc: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC,
) -> Result<D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC> {
    let relocated = unsafe { blas::resolve(owner, desc.DestAccelerationStructureData) };
    if relocated.is_some() || desc.ScratchAccelerationStructureData == 0 {
        let mut device = None::<ID3D12Device5>;
        unsafe { host.GetDevice(&mut device)? };
        let mut info = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO::default();
        unsafe {
            device
                .ok_or(E_NOTIMPL)?
                .GetRaytracingAccelerationStructurePrebuildInfo(&desc.Inputs, &mut info)
        };
        let scratch = if desc
            .Inputs
            .Flags
            .contains(D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PERFORM_UPDATE)
        {
            info.UpdateScratchDataSizeInBytes
        } else {
            info.ScratchDataSizeInBytes
        };
        if info.ResultDataMaxSizeInBytes == 0
            || (desc.ScratchAccelerationStructureData == 0 && scratch != 0)
        {
            return Err(E_INVALIDARG.into());
        }
        if let Some(target) = relocated {
            if info.ResultDataMaxSizeInBytes > target.bytes {
                return Err(E_NOTIMPL.into());
            }
            retain(state, &[&target.resource])?;
            desc.DestAccelerationStructureData = unsafe { target.resource.GetGPUVirtualAddress() };
        }
    }
    if desc.SourceAccelerationStructureData != 0
        && let Some(source) = unsafe { blas::resolve(owner, desc.SourceAccelerationStructureData) }
    {
        retain(state, &[&source.resource])?;
        desc.SourceAccelerationStructureData = unsafe { source.resource.GetGPUVirtualAddress() };
    }
    if desc.Inputs.Type == D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_TOP_LEVEL
        && desc.Inputs.NumDescs != 0
    {
        let address = unsafe { desc.Inputs.Anonymous.InstanceDescs };
        if desc.Inputs.NumDescs
            > D3D12_RAYTRACING_MAX_INSTANCES_PER_TOP_LEVEL_ACCELERATION_STRUCTURE
        {
            return Err(E_INVALIDARG.into());
        }
        let count = desc.Inputs.NumDescs as usize;
        state
            .instance_uploads
            .try_reserve(1)
            .map_err(|_| E_OUTOFMEMORY)?;
        let indirect = desc.Inputs.DescsLayout == D3D12_ELEMENTS_LAYOUT_ARRAY_OF_POINTERS;
        let stride = if indirect { 8 } else { INSTANCE_BYTES };
        let bytes = (count * stride) as u64;
        let instances = match unsafe { blas::upload_range(owner, address) } {
            Ok((source, _, available)) => {
                if available < bytes {
                    return Err(E_INVALIDARG.into());
                }
                let patched =
                    unsafe { blas::instance_buffer(owner, (count * INSTANCE_BYTES) as u64)? };
                retain(state, &[&source, &patched])?;
                desc.Inputs.Anonymous.InstanceDescs = unsafe { patched.GetGPUVirtualAddress() };
                desc.Inputs.DescsLayout = D3D12_ELEMENTS_LAYOUT_ARRAY;
                Instances::Upload {
                    address,
                    count,
                    indirect,
                    patched,
                }
            }
            Err(e) if e.code() != E_INVALIDARG => return Err(e),
            Err(_) => match unsafe { blas::default_instances(owner, address) } {
                Ok((source, available)) if !indirect => {
                    if available < bytes
                        || unsafe { source.GetDesc() }
                            .Flags
                            .contains(D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE)
                    {
                        return Err(E_INVALIDARG.into());
                    }
                    let patched = unsafe {
                        blas::instance_fixup::Instances::record(owner, host, address, count as u32)?
                    };
                    unsafe {
                        state.bindings.restore(host);
                    }
                    retain(state, &[&source, patched.output()])?;
                    desc.Inputs.Anonymous.InstanceDescs =
                        unsafe { patched.output().GetGPUVirtualAddress() };
                    Instances::Gpu(patched)
                }
                Err(e) if e.code() != E_INVALIDARG => return Err(e),
                _ if unsafe { blas::has_relocations(owner) } => return Err(E_NOTIMPL.into()),
                _ => Instances::Direct,
            },
        };
        state.instance_uploads.push(instances);
    }
    Ok(desc)
}

// The scene builder passes one ordinary 48-byte description to List4, along
// with the standard count and optional array of postbuild requests.
pub(super) unsafe extern "system" fn single(
    list: &List,
    desc: *const D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC,
    postbuild_count: u32,
    _postbuild: *const D3D12_RAYTRACING_ACCELERATION_STRUCTURE_POSTBUILD_INFO_DESC,
) {
    if desc.is_null() || postbuild_count != 0 {
        let mut state = recording(list);
        if state.can_record(&list.native) {
            state.error = if desc.is_null() {
                E_INVALIDARG
            } else {
                E_NOTIMPL
            };
        }
        return;
    }
    let entry = Entry {
        build: unsafe { *desc },
        extension: [0; 4],
    };
    unsafe { batch(list, 1, &entry) };
}

pub(super) unsafe extern "system" fn batch(list: &List, count: u32, entries: *const Entry) {
    let mut state = recording(list);
    if !state.can_record(&list.native) || count == 0 {
        return;
    }
    let Some(host) = &list.state.raytracing else {
        state.error = E_NOTIMPL;
        return;
    };
    if entries.is_null() {
        state.error = E_INVALIDARG;
        return;
    }
    let entries = unsafe { core::slice::from_raw_parts(entries, count as usize) };
    let alignment = u64::from(D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BYTE_ALIGNMENT);
    for e in entries {
        if e.extension != [0; 4] {
            state.error = E_NOTIMPL;
            return;
        }
        let b = &e.build;
        let update = b
            .Inputs
            .Flags
            .contains(D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PERFORM_UPDATE);
        if !unsafe { ray_build::valid(&b.Inputs) }
            || b.DestAccelerationStructureData == 0
            || !(b.DestAccelerationStructureData
                | b.ScratchAccelerationStructureData
                | b.SourceAccelerationStructureData)
                .is_multiple_of(alignment)
            || (update && b.SourceAccelerationStructureData == 0)
        {
            state.error = E_INVALIDARG;
            return;
        }
        if b.Inputs.Type == D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_TOP_LEVEL
            && b.Inputs.NumDescs != 0
        {
            let va = unsafe { b.Inputs.Anonymous.InstanceDescs };
            let align = if b.Inputs.DescsLayout == D3D12_ELEMENTS_LAYOUT_ARRAY {
                u64::from(D3D12_RAYTRACING_INSTANCE_DESCS_BYTE_ALIGNMENT)
            } else {
                8
            };
            if va == 0 || !va.is_multiple_of(align) {
                state.error = E_INVALIDARG;
                return;
            }
        }
    }
    let Some(owner) = &list.parent else {
        state.error = E_INVALIDARG;
        return;
    };
    let initial_uploads = state.instance_uploads.len();
    let mut builds = Vec::new();
    if builds.try_reserve_exact(entries.len()).is_err() {
        state.error = E_OUTOFMEMORY;
        return;
    }
    for e in entries {
        match translate(&mut state, owner, host, e.build) {
            Ok(build) => builds.push(build),
            Err(err) => {
                state.error = err.code();
                return;
            }
        }
    }
    for build in &builds {
        unsafe { host.BuildRaytracingAccelerationStructure(build, None) };
    }
    for input in &state.instance_uploads[initial_uploads..] {
        if let Instances::Gpu(records) = input {
            unsafe {
                records.barrier(host, false);
            }
        }
    }
}
