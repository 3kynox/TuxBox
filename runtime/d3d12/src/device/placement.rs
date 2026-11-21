//! CPU-backed, read-only Xbox textures. Each guest submission observes the
//! current backing pages; desktop textures and staging allocations are private.
mod descriptors;
mod upload;
pub(crate) use descriptors::HeapViews;

use super::Device;
use core::{ffi::c_void, ops::Range};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use tuxbox_xg_layout::{Desc, Plane};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::{Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess};
use windows::core::{HRESULT, Result};

pub(crate) fn read_state(state: D3D12_RESOURCE_STATES) -> bool {
    let allowed = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
        | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
        | D3D12_RESOURCE_STATE_COPY_SOURCE;
    state.0 & !allowed.0 == 0
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|error| error.into_inner())
}

fn committed(
    device: &ID3D12Device,
    kind: D3D12_HEAP_TYPE,
    desc: &D3D12_RESOURCE_DESC,
    initial: D3D12_RESOURCE_STATES,
) -> Result<ID3D12Resource> {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: kind,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
        ..Default::default()
    };
    let mut native = None;
    // SAFETY: D3D12 only borrows the input structures during creation.
    unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            desc,
            initial,
            None,
            &mut native,
        )?;
    }
    native.ok_or_else(|| E_POINTER.into())
}

struct Mip {
    slice: u32,
    level: u32,
    source: Range<u64>,
    footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    elements: u32,
    rows: u32,
    bytes: u64,
}

#[derive(Default)]
struct Contents {
    // An exact copy makes writes to shared pages and all-zero rewrites observable.
    previous: Vec<Vec<u8>>,
    uploaded: u64,
    readers: Vec<(ID3D12Fence, u64)>,
}

pub(crate) struct Texture {
    pub(super) native: ID3D12Resource,
    pub(super) desc: D3D12_RESOURCE_DESC,
    pub(super) address: u64,
    plane: Plane,
    mips: Vec<Mip>,
    contents: Mutex<Contents>,
}

impl Texture {
    pub(super) fn new(
        device: &ID3D12Device,
        address: u64,
        desc: &D3D12_RESOURCE_DESC,
        initial: D3D12_RESOURCE_STATES,
        clear: *const D3D12_CLEAR_VALUE,
    ) -> Result<Arc<Self>> {
        if !read_state(initial) || !clear.is_null() {
            return Err(E_NOTIMPL.into());
        }
        let plane = Plane::compute(&Desc {
            dimension: desc.Dimension.0 as _,
            alignment: desc.Alignment,
            width: desc.Width,
            height: desc.Height,
            slices: desc.DepthOrArraySize,
            mips: desc.MipLevels,
            format: desc.Format.0 as _,
            samples: desc.SampleDesc.Count,
            quality: desc.SampleDesc.Quality,
            layout: desc.Layout.0 as _,
            flags: desc.Flags.0 as _,
        })
        .map_err(HRESULT)?;
        if address == 0
            || !address.is_multiple_of(plane.alignment)
            || address.checked_add(plane.size).is_none()
        {
            return Err(E_INVALIDARG.into());
        }
        // A simultaneous-access texture implicitly promotes on first use and
        // decays to COMMON after ExecuteCommandLists, including copy-queue uploads.
        let host = D3D12_RESOURCE_DESC {
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
            ..*desc
        };
        let native = committed(
            device,
            D3D12_HEAP_TYPE_DEFAULT,
            &host,
            D3D12_RESOURCE_STATE_COMMON,
        )?;
        let count = plane.mip_count() * plane.array_size();
        let mut mips = Vec::with_capacity(count as usize);
        for index in 0..count {
            let slice = index / plane.mip_count();
            let level = index % plane.mip_count();
            let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
            let (mut rows, mut row_bytes, mut bytes) = (0, 0, 0);
            unsafe {
                device.GetCopyableFootprints(
                    &host,
                    index,
                    1,
                    0,
                    Some(&mut footprint),
                    Some(&mut rows),
                    Some(&mut row_bytes),
                    Some(&mut bytes),
                );
            }
            let elements = u32::try_from(row_bytes / u64::from(plane.element_bytes()))
                .map_err(|_| E_INVALIDARG)?;
            if rows == 0
                || elements == 0
                || !row_bytes.is_multiple_of(u64::from(plane.element_bytes()))
                || bytes == u64::MAX
                || plane
                    .element_offset(level, u64::from(elements - 1), rows - 1)
                    .is_none()
            {
                return Err(E_NOTIMPL.into());
            }
            mips.push(Mip {
                slice,
                level,
                source: plane.mip_range_in_slice(slice, level).ok_or(E_INVALIDARG)?,
                footprint,
                elements,
                rows,
                bytes,
            });
        }
        let contents = Mutex::new(Contents {
            previous: vec![Vec::new(); mips.len()],
            ..Default::default()
        });
        Ok(Arc::new(Self {
            native,
            desc: *desc,
            address,
            plane,
            mips,
            contents,
        }))
    }

    fn changed(
        self: &Arc<Self>,
        changes: &mut Vec<upload::Change>,
        scratch: &mut Vec<u8>,
    ) -> Result<()> {
        let mut contents = lock(&self.contents);
        for (level, mip) in self.mips.iter().enumerate() {
            let size = (mip.source.end - mip.source.start) as usize;
            scratch.clear();
            scratch.try_reserve_exact(size).map_err(|_| E_OUTOFMEMORY)?;
            let mut copied = 0;
            // SAFETY: the OS checks guest mappings even when the title decommits a
            // page concurrently. Only fully initialized, owned snapshots are used.
            let result = unsafe {
                ReadProcessMemory(
                    GetCurrentProcess(),
                    (self.address + mip.source.start) as *const _,
                    scratch.as_mut_ptr().cast(),
                    size,
                    Some(&mut copied),
                )
            };
            if result.is_err() || copied != size {
                contents.previous[level].clear();
                continue;
            }
            unsafe {
                scratch.set_len(size);
            }
            if *scratch != contents.previous[level] {
                changes.push(upload::Change {
                    texture: self.clone(),
                    level,
                    bytes: std::mem::take(scratch),
                });
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Placements {
    pub(super) views: Arc<descriptors::Views>,
    transfer: Mutex<Option<upload::Transfer>>,
}

// Descriptor contents may change after command-list Close. Resolve SRVs in the
// shader-visible heaps that actually participated in a draw or dispatch.
pub(crate) unsafe fn submit(
    device: *mut c_void,
    queue: &ID3D12CommandQueue,
    reader: &OnceLock<ID3D12Fence>,
    textures: &mut Vec<Arc<Texture>>,
    heaps: &[Range<usize>],
    execute: impl FnOnce(),
) -> Result<()> {
    let device = unsafe { &*device.cast::<Device>() };
    device.state.placements.views.gather(heaps, textures);
    if textures.is_empty() {
        execute();
        return Ok(());
    }
    textures.sort_unstable_by_key(Arc::as_ptr);
    textures.dedup_by(|a, b| Arc::ptr_eq(a, b));
    let mut slot = lock(&device.state.placements.transfer);
    let transfer = match slot.as_mut() {
        Some(transfer) => transfer,
        None => slot.insert(upload::Transfer::new(&device.native)?),
    };
    let reader = match reader.get() {
        Some(reader) => reader,
        None => {
            let fence = unsafe { device.native.CreateFence(0, D3D12_FENCE_FLAG_NONE)? };
            reader.get_or_init(|| fence)
        }
    };
    transfer.refresh(&device.native, textures)?;
    let required = textures
        .iter()
        .map(|t| lock(&t.contents).uploaded)
        .max()
        .unwrap_or(0);
    if unsafe { transfer.fence.GetCompletedValue() } < required {
        unsafe {
            queue.Wait(&transfer.fence, required)?;
        }
    }
    let tick = transfer.tick()?;
    execute();
    if let Err(error) = unsafe { queue.Signal(reader, tick) } {
        // An unfenced reader prevents safe replacement of its texture on any queue.
        transfer.failed = Some(error.code());
        return Err(error);
    }
    for texture in textures {
        let mut state = lock(&texture.contents);
        state
            .readers
            .retain(|(f, value)| f != reader && unsafe { f.GetCompletedValue() } < *value);
        state.readers.push((reader.clone(), tick));
    }
    Ok(())
}
