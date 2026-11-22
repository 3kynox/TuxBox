//! Mapped upload buffers and host storage for Xbox acceleration structures.
use super::{Device, shader_cache::source_digest};
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Once, Weak};
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::{Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess};
use windows::core::{IUnknown, Interface, Result};

pub(crate) mod instance_fixup;
mod rebuild;

#[derive(Clone)]
pub(crate) struct NativeAs {
    pub(crate) resource: ID3D12Resource,
    pub(crate) bytes: u64,
    guest_bytes: u64,
}

type Index = Mutex<BTreeMap<(Kind, u64), Vec<Arc<Buffer>>>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(PartialOrd, Ord, PartialEq, Eq, Clone, Copy)]
enum Kind {
    Upload,
    Structure,
    Default,
}

struct Buffer {
    native: ID3D12Resource,
    start: u64,
    bytes: u64,
    kind: Kind,
    // None pins an in-pool address that a previously recorded command can still consume.
    targets: Mutex<BTreeMap<u64, Option<NativeAs>>>,
    mappings: Arc<Mutex<Mappings>>,
}

#[derive(Default)]
struct Mappings {
    addresses: BTreeMap<u64, u64>,
    snapshot: Option<Arc<[(u64, u64)]>>,
    table: Option<instance_fixup::Table>,
}

impl Mappings {
    fn dirty(&mut self) {
        self.snapshot = None;
        self.table = None;
    }

    fn snapshot(&mut self) -> Arc<[(u64, u64)]> {
        self.snapshot
            .get_or_insert_with(|| {
                self.addresses
                    .iter()
                    .map(|(&guest, &native)| (guest, native))
                    .collect()
            })
            .clone()
    }
}

pub(super) struct Registration {
    buffer: Arc<Buffer>,
    index: Weak<Index>,
}

pub(crate) fn is_structure(resource: &super::Resource) -> bool {
    resource
        .state
        .blas_buffer
        .as_ref()
        .is_some_and(|r| r.buffer.kind == Kind::Structure)
}

impl Drop for Registration {
    fn drop(&mut self) {
        if self.buffer.kind == Kind::Structure {
            let targets = lock(&self.buffer.targets);
            let mut mappings = lock(&self.buffer.mappings);
            for (&address, backing) in targets.iter() {
                if backing.is_some() && mappings.addresses.remove(&address).is_some() {
                    mappings.dirty();
                }
            }
        }
        if let Some(index) = self.index.upgrade() {
            let mut index = lock(&index);
            if let Some(entries) = index.get_mut(&(self.buffer.kind, self.buffer.start)) {
                entries.retain(|b| !Arc::ptr_eq(b, &self.buffer));
                if entries.is_empty() {
                    index.remove(&(self.buffer.kind, self.buffer.start));
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Buffers {
    index: Arc<Index>,
    builder: Mutex<Option<rebuild::Builder>>,
    fixup: Mutex<Option<instance_fixup::Kernel>>,
    mappings: Arc<Mutex<Mappings>>,
}

impl Buffers {
    pub(super) fn register(
        &self,
        resource: &ID3D12Resource,
        heap: D3D12_HEAP_TYPE,
        desc: &D3D12_RESOURCE_DESC,
        initial: D3D12_RESOURCE_STATES,
    ) -> Option<Registration> {
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER {
            return None;
        }
        let kind = if initial == D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE {
            Kind::Structure
        } else if heap == D3D12_HEAP_TYPE_UPLOAD {
            Kind::Upload
        } else if heap == D3D12_HEAP_TYPE_DEFAULT {
            Kind::Default
        } else {
            return None;
        };
        let start = unsafe { resource.GetGPUVirtualAddress() };
        let buffer = Arc::new(Buffer {
            native: resource.clone(),
            start,
            bytes: desc.Width,
            kind,
            targets: Mutex::default(),
            mappings: self.mappings.clone(),
        });
        lock(&self.index)
            .entry((kind, start))
            .or_default()
            .push(buffer.clone());
        Some(Registration {
            buffer,
            index: Arc::downgrade(&self.index),
        })
    }

    fn find(&self, address: u64, kind: Kind) -> Result<Arc<Buffer>> {
        let index = lock(&self.index);
        let mut candidates = index
            .range((kind, 0)..=(kind, address))
            .rev()
            .flat_map(|(_, group)| group)
            .filter(|buf| address - buf.start < buf.bytes);
        let result = candidates.next().cloned().ok_or(E_INVALIDARG)?;
        if candidates.next().is_some() {
            return Err(E_NOTIMPL.into()); // ambiguous overlapping placed resources
        }
        Ok(result)
    }
}

/// # Safety
/// The controlling unknown must belong to a live guest device.
unsafe fn guest(owner: &IUnknown) -> &Device {
    unsafe { &*owner.as_raw().cast::<Device>() }
}

/// # Safety
/// The owner must identify the controlling guest device.
pub(crate) unsafe fn instance_buffer(owner: &IUnknown, bytes: u64) -> Result<ID3D12Resource> {
    rebuild::upload_buffer(&unsafe { guest(owner) }.native, bytes)
}

/// # Safety
/// The owner must identify the controlling guest device.
pub(crate) unsafe fn has_relocations(owner: &IUnknown) -> bool {
    !lock(&unsafe { guest(owner) }.state.blas.mappings)
        .addresses
        .is_empty()
}

/// # Safety
/// The owner must be the list's controlling TuxBox device. Addresses map only
/// while the original guest AS resource is alive.
pub(crate) unsafe fn relocated(owner: &IUnknown) -> Arc<[(u64, u64)]> {
    lock(&unsafe { guest(owner) }.state.blas.mappings).snapshot()
}

/// # Safety
/// The owner must identify the list's controlling TuxBox device.
pub(crate) unsafe fn resolve(owner: &IUnknown, address: u64) -> Result<Option<NativeAs>> {
    let buffer = match unsafe { guest(owner) }
        .state
        .blas
        .find(address, Kind::Structure)
    {
        Ok(buffer) => buffer,
        // An unregistered address can be a native resource owned outside the
        // guest provider. Overlapping registered resources are different: the
        // backing cannot be chosen safely, so preserve the ambiguity error.
        Err(error) if error.code() == E_INVALIDARG => return Ok(None),
        Err(error) => return Err(error),
    };
    let result = lock(&buffer.targets).get(&address).and_then(Clone::clone);
    Ok(result)
}

struct Upload {
    buffer: Arc<Buffer>,
    offset: usize,
}

impl Upload {
    fn bytes(&self, len: usize) -> Result<Vec<u8>> {
        let end = self
            .offset
            .checked_add(len)
            .filter(|&v| (v as u64) <= self.buffer.bytes)
            .ok_or(E_INVALIDARG)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(len).map_err(|_| E_OUTOFMEMORY)?;
        bytes.resize(len, 0);
        let range = D3D12_RANGE {
            Begin: self.offset,
            End: end,
        };
        let mut ptr = core::ptr::null_mut();
        // SAFETY: restrict the mapping to the validated suballocation extent. Nested
        // Map/Unmap preserves existing guest uploads and their flush ranges.
        unsafe { self.buffer.native.Map(0, Some(&range), Some(&mut ptr)) }?;
        let mut copied = 0;
        let copied_ok = unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                ptr.cast::<u8>().wrapping_add(self.offset).cast(),
                bytes.as_mut_ptr().cast(),
                len,
                Some(&mut copied),
            )
        };
        unsafe { self.buffer.native.Unmap(0, Some(&D3D12_RANGE::default())) };
        copied_ok?;
        if copied != len {
            return Err(E_INVALIDARG.into());
        }
        Ok(bytes)
    }
}

impl Upload {
    fn snapshot(&self, guest_capacity: u64) -> Result<(Vec<u8>, u64)> {
        let header = self.bytes(64)?;
        let length = usize::try_from(u64::from_le_bytes(header[32..40].try_into().unwrap()))
            .map_err(|_| E_INVALIDARG)?;
        let guest_bytes = u64::from_le_bytes(header[40..48].try_into().unwrap());
        if length < 64
            || guest_bytes.checked_add(64) != Some(length as u64)
            || guest_bytes > guest_capacity
        {
            return Err(E_INVALIDARG.into());
        }
        let bytes = self.bytes(length)?;
        if bytes[..64] != header {
            return Err(E_INVALIDARG.into());
        }
        Ok((bytes, guest_bytes))
    }
}

pub(crate) struct Input {
    upload: Upload,
    snapshot: Mutex<Vec<u8>>,
    source: ID3D12Resource,
    guest_destination: u64,
    guest_capacity: u64,
    host_capacity: u64,
}

impl Input {
    /// Refresh the native source when the title rewrites a recorded upload
    /// after recording. The guest observes the contents at queue submission.
    /// # Safety
    /// The owner must be the controlling device that recorded the list.
    pub(crate) unsafe fn prepare(&self, owner: &IUnknown) -> Result<()> {
        let mut previous = lock(&self.snapshot);
        let (bytes, _) = self.upload.snapshot(self.guest_capacity)?;
        if *previous == bytes {
            return Ok(());
        }
        let src = self.upload.buffer.start + self.upload.offset as u64;
        let dst = self.guest_destination;
        let triangles =
            crate::commands::serialized_blas::decode_for_build(&bytes).map_err(|e| {
                failure(src, dst, "updated triangle decoder", e, Some(&bytes));
                E_NOTIMPL
            })?;
        let mut builder = lock(&unsafe { guest(owner) }.state.blas.builder);
        let builder = builder.as_mut().ok_or(E_INVALIDARG)?;
        let (updated, compacted) = builder
            .build(&triangles)
            .inspect_err(|e| failure(src, dst, "updated native build", e.code(), Some(&bytes)))?;
        if compacted > self.host_capacity {
            failure(
                src,
                dst,
                "updated compacted size exceeds backing",
                compacted,
                Some(&bytes),
            );
            return Err(E_NOTIMPL.into());
        }
        builder.replace(&self.source, updated).inspect_err(|e| {
            failure(
                src,
                dst,
                "updated source replacement",
                e.code(),
                Some(&bytes),
            )
        })?;
        *previous = bytes;
        Ok(())
    }
}

pub(crate) struct Rebuilt {
    pub input: Input,
    pub source: ID3D12Resource,
    pub destination: ID3D12Resource,
    pub destination_address: u64,
}

// Failed inputs are bounded, opt-in diagnostics for examining additional console
// geometry layouts. A trace alone never writes title data to disk.
fn failure(src: u64, dst: u64, stage: &str, why: impl std::fmt::Debug, bytes: Option<&[u8]>) {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    static CAPTURE_BYTES: Mutex<usize> = Mutex::new(0);
    static TRACE: LazyLock<bool> =
        LazyLock::new(|| std::env::var("TUXBOX_TRACE_RAYTRACING").as_deref() == Ok("1"));
    static CAPTURE: LazyLock<Option<std::path::PathBuf>> =
        LazyLock::new(|| std::env::var_os("TUXBOX_CAPTURE_BLAS").map(Into::into));
    if (!*TRACE && CAPTURE.is_none()) || COUNT.fetch_add(1, Ordering::Relaxed) >= 64 {
        return;
    }
    let digest = bytes.and_then(source_digest);
    crate::diagnostics::log_once(&Once::new(), || {
        format!(
            "Xbox BLAS {stage} failed {why:?}, source {src:#x}, destination {dst:#x}, bytes {}, SHA-256 {}",
            bytes.map_or(0, <[u8]>::len),
            digest.as_deref().unwrap_or("unavailable")
        )
    });
    if let (Some(folder), Some(bytes), Some(digest)) = (&*CAPTURE, bytes, digest) {
        let mut written = lock(&CAPTURE_BYTES);
        if bytes.len() <= 256 * 1024 * 1024 - *written
            && let Ok(mut file) = std::fs::File::create_new(folder.join(format!("{digest}.blas")))
        {
            *written += bytes.len();
            if let Err(error) = file.write_all(bytes) {
                crate::diagnostics::log_once(&Once::new(), || {
                    format!("Xbox BLAS capture failed: {error}")
                });
            }
        }
    }
}

/// # Safety
/// The owner must identify the controlling guest device.
pub(crate) unsafe fn deserialize(owner: &IUnknown, src: u64, dst: u64) -> Result<Rebuilt> {
    let device = unsafe { guest(owner) };
    let registry = &device.state.blas;
    let buffer = registry
        .find(src, Kind::Upload)
        .inspect_err(|e| failure(src, dst, "source lookup", e.code(), None))?;
    let upload = Upload {
        offset: (src - buffer.start) as usize,
        buffer,
    };
    let buffer = registry
        .find(dst, Kind::Structure)
        .inspect_err(|e| failure(src, dst, "destination lookup", e.code(), None))?;
    let (snapshot, guest_bytes) = upload
        .snapshot(buffer.bytes - (dst - buffer.start))
        .inspect_err(|e| failure(src, dst, "upload header", e.code(), None))?;
    let triangles = crate::commands::serialized_blas::decode_for_build(&snapshot).map_err(|e| {
        failure(src, dst, "triangle decoder", e, Some(&snapshot));
        E_NOTIMPL
    })?;
    let mut builder = lock(&registry.builder);
    if builder.is_none() {
        *builder = Some(rebuild::Builder::new(&device.native)?);
    }
    let (source, compacted) = builder
        .as_mut()
        .unwrap()
        .build(&triangles)
        .inspect_err(|e| failure(src, dst, "native build", e.code(), Some(&snapshot)))?;
    drop(builder);
    let (destination, destination_address) =
        target(&device.native, &buffer, dst, guest_bytes, compacted)
            .inspect_err(|e| failure(src, dst, "placement", e.code(), Some(&snapshot)))?;
    static TRACE: LazyLock<bool> =
        LazyLock::new(|| std::env::var("TUXBOX_TRACE_RAYTRACING").as_deref() == Ok("1"));
    static FIRST: Once = Once::new();
    if *TRACE {
        crate::diagnostics::log_once(&FIRST, || {
            format!(
                "rebuilt Xbox BLAS {} at {dst:#x}: {} triangles, {guest_bytes} guest bytes, {compacted} compacted host bytes, host VA {destination_address:#x}",
                source_digest(&snapshot).unwrap_or_default(),
                triangles.len()
            )
        });
    }
    Ok(Rebuilt {
        input: Input {
            upload,
            snapshot: Mutex::new(snapshot),
            source: source.clone(),
            guest_destination: dst,
            guest_capacity: guest_bytes,
            host_capacity: guest_bytes.max(compacted),
        },
        source,
        destination,
        destination_address,
    })
}

fn target(
    device: &ID3D12Device,
    buffer: &Buffer,
    address: u64,
    guest_bytes: u64,
    bytes: u64,
) -> Result<(ID3D12Resource, u64)> {
    let mut targets = lock(&buffer.targets);
    if let Some(previous) = targets.get(&address) {
        let Some(existing) = previous else {
            if bytes > guest_bytes {
                return Err(E_NOTIMPL.into());
            }
            return Ok((buffer.native.clone(), address));
        };
        if existing.bytes < bytes || existing.guest_bytes != guest_bytes {
            return Err(E_NOTIMPL.into()); // preserve addresses already taken by a TLAS
        }
        return Ok((existing.resource.clone(), unsafe {
            existing.resource.GetGPUVirtualAddress()
        }));
    }
    if bytes <= guest_bytes {
        targets.insert(address, None);
        return Ok((buffer.native.clone(), address));
    }
    let resource = rebuild::as_buffer(device, bytes)?;
    let host = unsafe { resource.GetGPUVirtualAddress() };
    targets.insert(
        address,
        Some(NativeAs {
            resource: resource.clone(),
            bytes,
            guest_bytes,
        }),
    );
    let mut mappings = lock(&buffer.mappings);
    mappings.addresses.insert(address, host);
    mappings.dirty();
    Ok((resource, host))
}

/// # Safety
/// The owner must identify the controlling guest device.
pub(crate) unsafe fn copy_dest(
    owner: &IUnknown,
    dst: u64,
    source: &NativeAs,
) -> Result<(ID3D12Resource, u64)> {
    let device = unsafe { guest(owner) };
    let buffer = device.state.blas.find(dst, Kind::Structure)?;
    if source.guest_bytes > buffer.bytes - (dst - buffer.start) {
        return Err(E_INVALIDARG.into());
    }
    target(
        &device.native,
        &buffer,
        dst,
        source.guest_bytes,
        source.bytes,
    )
}

/// # Safety
/// The owner must identify the controlling guest device.
pub(crate) unsafe fn upload_range(
    owner: &IUnknown,
    address: u64,
) -> Result<(ID3D12Resource, u64, u64)> {
    let buffer = unsafe { guest(owner) }
        .state
        .blas
        .find(address, Kind::Upload)?;
    let offset = address - buffer.start;
    Ok((buffer.native.clone(), offset, buffer.bytes - offset))
}

/// The guest device owns the returned buffer. Report ambiguity separately from a
/// native address registered outside the guest provider.
pub(crate) unsafe fn default_instances(
    owner: &IUnknown,
    address: u64,
) -> Result<(ID3D12Resource, u64)> {
    let buffer = unsafe { guest(owner) }
        .state
        .blas
        .find(address, Kind::Default)?;
    Ok((
        buffer.native.clone(),
        buffer.bytes - (address - buffer.start),
    ))
}
