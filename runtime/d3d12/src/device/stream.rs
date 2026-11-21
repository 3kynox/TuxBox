//! Xbox device slot 86. Standard x64 state subobjects use desktop D3D12 layout;
//! the console also appends a stencil subobject and embeds guest object handles.
use super::{Device, Out, compute, identity, shader_cache, wrap_pageable};
use crate::proxy::check_output;
use core::{ffi::c_void, mem::MaybeUninit, ptr, slice};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL, E_POINTER};
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::DXGI_SAMPLE_DESC};
use windows::core::{GUID, HRESULT, Interface};

// In the packaged mesh builder at RVA 0x57577d0, Xbox AS and MS are one
// lower than the corresponding desktop subobject tags.
const AMPLIFICATION_X: i32 = 23;
const MESH_X: i32 = 24;
const STENCIL_X: i32 = 26;

fn subobject<T>() -> (usize, usize) {
    let payload = size_of::<u32>().next_multiple_of(align_of::<T>());
    (
        payload,
        (payload + size_of::<T>()).next_multiple_of(align_of::<usize>()),
    )
}

fn layout(tag: i32) -> Option<(usize, usize)> {
    Some(match tag {
        0 => subobject::<*mut c_void>(), // root signature
        1..=6 | AMPLIFICATION_X | MESH_X => subobject::<D3D12_SHADER_BYTECODE>(),
        7 => subobject::<D3D12_STREAM_OUTPUT_DESC>(),
        8 => subobject::<D3D12_BLEND_DESC>(),
        9 | 13 | 14 | 16 | 18 | 20 => subobject::<u32>(), // mask, strip cut, topology, DSV, node, flags
        10 => subobject::<D3D12_RASTERIZER_DESC>(),
        11 => subobject::<D3D12_DEPTH_STENCIL_DESC>(),
        12 => subobject::<D3D12_INPUT_LAYOUT_DESC>(),
        15 => subobject::<D3D12_RT_FORMAT_ARRAY>(),
        17 => subobject::<DXGI_SAMPLE_DESC>(),
        19 => subobject::<D3D12_CACHED_PIPELINE_STATE>(),
        21 => subobject::<D3D12_DEPTH_STENCIL_DESC1>(),
        22 => subobject::<D3D12_VIEW_INSTANCING_DESC>(),
        STENCIL_X => (8, 32),
        _ => return None,
    })
}

// The retail builder at RVA 0x57572f0 repeats depth/stencil masks and per-face
// enables at the end of the stream. Dropping that record preserves state only
// when it agrees with DEPTH_STENCIL1 and all extra state fields are inactive.
unsafe fn equivalent_stencil(payload: *const u8, depth: &D3D12_DEPTH_STENCIL_DESC1) -> bool {
    let enabled = u8::from(depth.StencilEnable.as_bool());
    let mut expected = [0; 18];
    expected[12..18].copy_from_slice(&[
        depth.StencilReadMask,
        depth.StencilWriteMask,
        0,
        0,
        enabled,
        enabled,
    ]);
    unsafe { payload.cast::<[u8; 18]>().read_unaligned() == expected }
}

unsafe fn create(
    device: &Device,
    desc: &D3D12_PIPELINE_STATE_STREAM_DESC,
) -> windows::core::Result<ID3D12PipelineState> {
    let host = device.state.pipeline_device.as_ref().ok_or(E_NOINTERFACE)?;
    let mut size = desc.SizeInBytes;
    let input = desc.pPipelineStateSubobjectStream.cast::<u8>();
    if input.is_null() {
        return Err(E_POINTER.into());
    }
    // Even a stream containing every supported subobject once fits in 4 KiB.
    if size == 0
        || size > 4096
        || !size.is_multiple_of(size_of::<usize>())
        || !input.cast::<usize>().is_aligned()
    {
        return Err(E_INVALIDARG.into());
    }
    // Guest structures can have uninitialized alignment padding. Copy their
    // representation verbatim and only interpret the fields selected by tags.
    let mut storage = vec![MaybeUninit::<usize>::uninit(); size / size_of::<usize>()];
    let output = storage.as_mut_ptr().cast::<u8>();
    unsafe { ptr::copy_nonoverlapping(input, output, size) };
    let mut depth = None;
    let mut stages = Vec::new();
    let mut original = [None; 8];
    let mut fields = [None; 8];
    let mut seen = 0u32;
    let mut at = 0;
    while at < size {
        // Each validated subobject begins on an x64 pointer boundary.
        let record = unsafe { output.add(at) };
        let tag = unsafe { record.cast::<i32>().read() };
        let (offset, bytes) = layout(tag).ok_or(E_NOTIMPL)?;
        if bytes > size - at || seen & (1 << tag) != 0 {
            return Err(E_INVALIDARG.into());
        }
        seen |= 1 << tag;
        let payload = unsafe { record.add(offset) };
        match tag {
            0 => {
                let handle = payload.cast::<*mut c_void>();
                let root =
                    unsafe { compute::root_signature(handle.read(), identity(device).as_raw()) }?;
                // The guest keeps its root (and therefore this native interface)
                // alive until CreatePipelineState returns.
                unsafe { handle.write(root.map_or(ptr::null_mut(), Interface::as_raw)) };
            }
            1..=6 | AMPLIFICATION_X | MESH_X => {
                let index = match tag {
                    AMPLIFICATION_X | MESH_X => {
                        let native = if tag == AMPLIFICATION_X {
                            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS
                        } else {
                            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS
                        };
                        unsafe { record.cast::<i32>().write(native.0) };
                        (6 + tag - AMPLIFICATION_X) as usize
                    }
                    _ => (tag - 1) as usize,
                };
                let shader = unsafe { &mut *payload.cast::<D3D12_SHADER_BYTECODE>() };
                if shader.BytecodeLength != 0 {
                    if shader.pShaderBytecode.is_null()
                        || shader.BytecodeLength > isize::MAX as usize
                    {
                        return Err(E_INVALIDARG.into());
                    }
                    let code = unsafe {
                        slice::from_raw_parts(shader.pShaderBytecode.cast(), shader.BytecodeLength)
                    };
                    fields[index] = Some(ptr::from_mut(shader));
                    let compiled = match tag {
                        AMPLIFICATION_X => {
                            shader_cache::mesh_stage(code, shader_cache::Stage::Amplification)?
                        }
                        MESH_X => shader_cache::mesh_stage(code, shader_cache::Stage::Mesh)?,
                        _ => shader_cache::prepared(code),
                    };
                    original[index] = Some((code, compiled.as_ref().map(|_| stages.len())));
                    if let Some(compiled) = compiled {
                        shader.pShaderBytecode = compiled.as_ptr().cast();
                        shader.BytecodeLength = compiled.len();
                        stages.push(compiled);
                    }
                }
            }
            // Cached native Xbox PSOs are hints, specific to the console driver.
            19 => unsafe {
                payload
                    .cast::<D3D12_CACHED_PIPELINE_STATE>()
                    .write(Default::default())
            },
            21 => depth = Some(unsafe { &*payload.cast::<D3D12_DEPTH_STENCIL_DESC1>() }),
            STENCIL_X => {
                let depth = depth.ok_or(E_NOTIMPL)?;
                if at + bytes != size || !unsafe { equivalent_stencil(payload, depth) } {
                    return Err(E_NOTIMPL.into());
                }
                size = at;
                break;
            }
            _ => {}
        }
        at += bytes;
    }

    let native_desc = D3D12_PIPELINE_STATE_STREAM_DESC {
        SizeInBytes: size,
        pPipelineStateSubobjectStream: output.cast(),
    };
    // SAFETY: the stream storage, translated shaders, and guest-owned inputs
    // all stay alive throughout this synchronous call on the desktop Device2.
    let initial = unsafe { host.CreatePipelineState(&native_desc) };
    let Err(error) = &initial else {
        if fields[7].is_some() {
            static FIRST_MESH: std::sync::Once = std::sync::Once::new();
            crate::diagnostics::log_once(&FIRST_MESH, || {
                format!(
                    "created Xbox mesh pipeline ({}-byte stream, amplification={})",
                    desc.SizeInBytes,
                    fields[6].is_some()
                )
            });
        }
        return initial;
    };
    let status = error.code();
    // Do not hash normal successful submissions twice. The consumer layout
    // only matters when the desktop driver rejects the ordinary stage pair.
    let hashes = original.map(|stage| {
        stage.and_then(|(code, prepared)| {
            shader_cache::source_digest(code).map(|hash| (hash, prepared.is_some()))
        })
    });
    // Pixel inputs come from the last active raster stage. Geometry outputs
    // take precedence over vertex outputs, even in a stream that has both.
    let producer = if fields[4].is_some() { 4 } else { 0 };
    let direct = fields[5..].iter().all(Option::is_none)
        && (producer == 4 || fields[2..4].iter().all(Option::is_none));
    if direct
        && let (Some(field), Some((source, _)), Some((pixel, _))) =
            (fields[producer], &hashes[producer], &hashes[1])
        && let Some(linked) = (if producer == 4 {
            shader_cache::linked_geometry(source, pixel)
        } else {
            shader_cache::linked_vertex(source, pixel)
        })
        .or_else(|| {
            if status != E_INVALIDARG {
                return None;
            }
            let (producer_code, Some(i)) = original[producer]? else {
                return None;
            };
            let (pixel_code, Some(j)) = original[1]? else {
                return None;
            };
            shader_cache::link_on_demand(
                (source, producer_code),
                (pixel, pixel_code),
                &stages[i],
                &stages[j],
            )
        })
    {
        // Use this producer only for this exact consumer. It retains the
        // semantics of the original raster producer with a compatible output signature.
        unsafe {
            field.write(D3D12_SHADER_BYTECODE {
                pShaderBytecode: linked.as_ptr().cast(),
                BytecodeLength: linked.len(),
            })
        };
        let retry = unsafe { host.CreatePipelineState(&native_desc) };
        match &retry {
            Ok(_) => {
                static VERTEX: std::sync::Once = std::sync::Once::new();
                static GEOMETRY: std::sync::Once = std::sync::Once::new();
                let (name, site) = if producer == 4 {
                    ("GS", &GEOMETRY)
                } else {
                    ("VS", &VERTEX)
                };
                crate::diagnostics::log_once(site, || {
                    format!("linked slot 86 {name}={source} PS={pixel}")
                });
            }
            Err(error) => crate::diagnostics::pipeline_stages(&hashes, error.code().0, true),
        }
        return retry;
    }
    crate::diagnostics::pipeline_stages(&hashes, status.0, false);
    initial
}

pub(super) unsafe extern "system" fn create_pipeline(
    device: &Device,
    desc: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    iid: *const GUID,
    out: Out,
) -> HRESULT {
    let status = unsafe { check_output::<ID3D12PipelineState>(iid, out) };
    if status.is_err() {
        return status;
    }
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_POINTER;
    };
    let native = unsafe { create(device, desc) };
    crate::diagnostics::pipeline_stream(
        desc.SizeInBytes,
        native.as_ref().err().map(|e| e.code().0),
    );
    unsafe {
        wrap_pageable(
            native,
            &compute::PIPELINE,
            device,
            compute::PipelineData::default(),
            out,
        )
    }
}
