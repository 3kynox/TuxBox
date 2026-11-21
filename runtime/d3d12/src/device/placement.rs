//! Keep CPU-backed tiled Xbox textures coherent with their desktop images.
mod descriptors;
mod publication;
mod upload;
mod writeback;
pub(crate) use descriptors::HeapViews;
pub(crate) use publication::QueueAccess;

use super::Device;
use core::{ffi::c_void, ops::Range};
use std::collections::{BTreeMap, btree_map::Entry};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use tuxbox_xg_layout::{Desc, Plane};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::{Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess};
use windows::core::{HRESULT, Result};

pub(crate) fn supported_state(state: D3D12_RESOURCE_STATES) -> bool {
    let allowed = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
        | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
        | D3D12_RESOURCE_STATE_COPY_SOURCE;
    state == D3D12_RESOURCE_STATE_COPY_DEST || state.0 & !allowed.0 == 0
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

struct Alias {
    desc: D3D12_RESOURCE_DESC,
    texture: Weak<Texture>,
}

#[derive(Default)]
struct Images(Mutex<BTreeMap<u64, Vec<Alias>>>);

pub(crate) struct Texture {
    pub(super) native: ID3D12Resource,
    pub(super) desc: D3D12_RESOURCE_DESC,
    pub(super) address: u64,
    plane: Plane,
    mips: Vec<Mip>,
    contents: Mutex<Contents>,
    images: Weak<Images>,
}

impl Drop for Texture {
    fn drop(&mut self) {
        let Some(images) = self.images.upgrade() else {
            return;
        };
        if let Entry::Occupied(mut entry) = lock(&images.0).entry(self.address) {
            entry
                .get_mut()
                .retain(|alias| alias.texture.strong_count() != 0);
            if entry.get().is_empty() {
                entry.remove();
            }
        }
    }
}

impl Texture {
    fn new(
        device: &ID3D12Device,
        images: &Arc<Images>,
        address: u64,
        desc: &D3D12_RESOURCE_DESC,
    ) -> Result<Arc<Self>> {
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
            images: Arc::downgrade(images),
        }))
    }

    fn snapshot_mip(&self, mip: &Mip, output: &mut Vec<u8>) -> Result<bool> {
        let size = (mip.source.end - mip.source.start) as usize;
        output.clear();
        output.try_reserve_exact(size).map_err(|_| E_OUTOFMEMORY)?;
        let mut copied = 0;
        // SAFETY: the OS checks the guest mapping. A partial transfer never
        // exposes uninitialized bytes, including when pages are decommitted.
        let result = unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                (self.address + mip.source.start) as *const _,
                output.as_mut_ptr().cast(),
                size,
                Some(&mut copied),
            )
        };
        if result.is_err() || copied != size {
            return Ok(false);
        }
        unsafe { output.set_len(size) };
        Ok(true)
    }

    fn changed(
        self: &Arc<Self>,
        changes: &mut Vec<upload::Change>,
        scratch: &mut Vec<u8>,
    ) -> Result<()> {
        let mut contents = lock(&self.contents);
        for (level, mip) in self.mips.iter().enumerate() {
            if !self.snapshot_mip(mip, scratch)? {
                contents.previous[level].clear();
                continue;
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
    // Equal descriptors at the same VA name the same image, including writes
    // made before the title creates a second resource proxy for it.
    images: Arc<Images>,
    submissions: Mutex<Option<Submissions>>,
    failed: Arc<AtomicI32>,
}

impl Placements {
    pub(super) fn texture(
        &self,
        device: &ID3D12Device,
        address: u64,
        desc: &D3D12_RESOURCE_DESC,
        initial: D3D12_RESOURCE_STATES,
        clear: *const D3D12_CLEAR_VALUE,
    ) -> Result<Arc<Texture>> {
        if !supported_state(initial) || !clear.is_null() {
            return Err(E_NOTIMPL.into());
        }
        let mut images = lock(&self.images.0);
        if let Some(texture) = images.get(&address).and_then(|aliases| {
            aliases
                .iter()
                .filter(|alias| alias.desc == *desc)
                .find_map(|alias| alias.texture.upgrade())
        }) {
            return Ok(texture);
        }
        let texture = Texture::new(device, &self.images, address, desc)?;
        images.entry(address).or_default().push(Alias {
            desc: *desc,
            texture: Arc::downgrade(&texture),
        });
        Ok(texture)
    }
}

struct PendingWrite {
    // The readback job owns the image only through native completion. An idle
    // queue need not keep its last frame's GPU allocations alive indefinitely.
    texture: Weak<Texture>,
    range: Range<u64>,
    published: ID3D12Fence,
    tick: u64,
}

struct Submissions {
    transfer: upload::Transfer,
    writes: Vec<PendingWrite>,
}

impl Texture {
    fn range(&self) -> Range<u64> {
        // The entire range was checked during construction.
        self.address..self.address + self.plane.size
    }
}

fn overlaps(a: &Range<u64>, b: &Range<u64>) -> bool {
    a.start < b.end && b.start < a.end
}

fn unique(textures: &mut Vec<Arc<Texture>>) {
    textures.sort_unstable_by_key(Arc::as_ptr);
    textures.dedup_by(|a, b| Arc::ptr_eq(a, b));
}

// Descriptor contents may change after command-list Close. Resolve SRVs in the
// shader-visible heaps that actually participated in a draw or dispatch.
pub(crate) unsafe fn submit(
    device: *mut c_void,
    queue: &ID3D12CommandQueue,
    access: &QueueAccess,
    textures: &mut Vec<Arc<Texture>>,
    writes: &mut Vec<Arc<Texture>>,
    heaps: &[Range<usize>],
    execute: impl FnOnce(),
) -> Result<()> {
    let device = unsafe { &*device.cast::<Device>() };
    let placements = &device.state.placements;
    placements.views.gather(heaps, textures);
    unique(writes);
    textures
        .try_reserve(writes.len())
        .map_err(|_| E_OUTOFMEMORY)?;
    textures.extend(writes.iter().cloned());
    if textures.is_empty() {
        execute();
        return Ok(());
    }
    unique(textures);
    for writer in writes.iter() {
        if textures
            .iter()
            .any(|t| !Arc::ptr_eq(writer, t) && overlaps(&writer.range(), &t.range()))
        {
            crate::diagnostics::recording_error(format_args!(
                "overlapping placed texture layouts in one GPU write batch"
            ));
            return Err(E_NOTIMPL.into());
        }
    }

    let mut slot = lock(&placements.submissions);
    HRESULT(placements.failed.load(Ordering::Acquire)).ok()?;
    let submission = match slot.as_mut() {
        Some(s) => s,
        None => slot.insert(Submissions {
            transfer: upload::Transfer::new(&device.native)?,
            writes: Vec::new(),
        }),
    };
    let reader = access.reader(&device.native)?;
    submission
        .writes
        .retain(|w| unsafe { w.published.GetCompletedValue() } < w.tick);
    let mut refresh = Vec::new();
    refresh
        .try_reserve(textures.len())
        .map_err(|_| E_OUTOFMEMORY)?;
    for texture in textures.iter() {
        let mut pending = false;
        for write in submission
            .writes
            .iter()
            .filter(|w| overlaps(&texture.range(), &w.range))
        {
            if Arc::as_ptr(texture) != write.texture.as_ptr() {
                crate::diagnostics::recording_error(format_args!(
                    "placed texture layout alias before GPU write publication"
                ));
                return Err(E_NOTIMPL.into());
            }
            // CPU bytes still contain the old version. The native image contains
            // the ordered GPU write and can be read directly by the next batch.
            unsafe { queue.Wait(&write.published, write.tick)? };
            pending = true;
        }
        if !pending {
            refresh.push(texture.clone());
        }
    }

    let transfer = &mut submission.transfer;
    transfer.refresh(&device.native, &refresh)?;
    let required = textures
        .iter()
        .map(|t| lock(&t.contents).uploaded)
        .max()
        .unwrap_or(0);
    if unsafe { transfer.fence.GetCompletedValue() } < required {
        unsafe { queue.Wait(&transfer.fence, required)? };
    }
    for texture in writes.iter() {
        for (fence, value) in &lock(&texture.contents).readers {
            if fence != reader && unsafe { fence.GetCompletedValue() } < *value {
                unsafe { queue.Wait(fence, *value)? };
            }
        }
    }
    let tick = transfer.tick()?;
    let publication = if writes.is_empty() {
        None
    } else {
        submission
            .writes
            .try_reserve(writes.len())
            .map_err(|_| E_OUTOFMEMORY)?;
        let worker = access.worker(&device.native, &placements.failed)?;
        let readback =
            writeback::Readback::record(&device.native, unsafe { queue.GetDesc() }.Type, writes)?;
        Some((worker, readback))
    };
    execute();
    if let Some((_, readback)) = &publication {
        // Separate ExecuteCommandLists calls allow simultaneous-access textures
        // to decay to COMMON between the guest batch and this copy.
        unsafe { queue.ExecuteCommandLists(&[Some(readback.list.clone())]) };
    }
    if let Err(error) = unsafe { queue.Signal(reader, tick) } {
        transfer.failed = Some(error.code());
        // A failed signal supplies no proof that the GPU released the recording.
        // Retry once to retire it, or retain it for the lifetime of a live driver.
        if let Some((worker, readback)) = publication {
            if unsafe { queue.Signal(reader, tick) }.is_ok() {
                worker.enqueue(readback, reader, tick)?;
            } else if unsafe { device.native.GetDeviceRemovedReason() }.is_ok() {
                std::mem::forget(readback);
            }
        }
        return Err(error);
    }
    if let Some((worker, readback)) = publication {
        if let Err(error) = worker.enqueue(readback, reader, tick) {
            transfer.failed = Some(error.code());
            return Err(error);
        }
        // A guest fence signaled after this operation must include the CPU
        // publication. The submitting CPU thread remains free to signal fences.
        if let Err(error) = unsafe { queue.Wait(&worker.published, tick) } {
            transfer.failed = Some(error.code());
            return Err(error);
        }
        for texture in writes.iter() {
            submission.writes.push(PendingWrite {
                texture: Arc::downgrade(texture),
                range: texture.range(),
                published: worker.published.clone(),
                tick,
            });
        }
    }
    for texture in textures {
        let mut state = lock(&texture.contents);
        state
            .readers
            .retain(|(f, v)| f != reader && unsafe { f.GetCompletedValue() } < *v);
        state.readers.push((reader.clone(), tick));
    }
    Ok(())
}
