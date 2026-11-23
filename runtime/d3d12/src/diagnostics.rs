//! Game launch milestones for the Wine debug channel or a native console.

use std::ffi::OsStr;
use std::io::{self, Write};
use std::sync::Once;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE, D3D12_COMMAND_LIST_TYPE_COMPUTE, D3D12_COMMAND_LIST_TYPE_COPY,
    D3D12_COMMAND_LIST_TYPE_DIRECT, ID3D12Device,
};
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringA;
use windows::core::PCSTR;

pub(crate) fn log_once(site: &Once, detail: impl FnOnce() -> String) {
    site.call_once(|| {
        let target = std::env::var_os("TUXBOX_LOG");
        if target.as_deref() == Some(OsStr::new("stderr")) {
            // A closed console must not interrupt the game's rendering thread.
            let _ = writeln!(io::stderr().lock(), "TuxBox d3d12_xs: {}", detail());
        } else if target.as_deref() == Some(OsStr::new("1")) {
            let line = format!("TuxBox d3d12_xs: {}\n\0", detail());
            // SAFETY: All milestone text is ASCII and the buffer is NUL-terminated.
            unsafe { OutputDebugStringA(PCSTR(line.as_ptr())) };
        }
    });
}

pub(crate) struct RasterDraw {
    first: Once,
    vertex: String,
    pixel: String,
}

impl RasterDraw {
    pub(crate) fn trace(vertex: Option<&[u8]>, pixel: Option<&[u8]>, digest: impl Fn(&[u8]) -> Option<String>) -> Option<std::sync::Arc<Self>> {
        static ENABLED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(||
            std::env::var_os("TUXBOX_TRACE_RASTER_DRAWS").as_deref() == Some(OsStr::new("1")));
        if !*ENABLED { return None; }
        Some(std::sync::Arc::new(Self {
            first: Once::new(),
            vertex: digest(vertex?)?,
            pixel: digest(pixel?)?,
        }))
    }
    pub(crate) fn recorded(&self) {
        log_once(&self.first, || format!("recorded raster draw VS={} PS={}", self.vertex, self.pixel));
    }
}

pub(super) fn device_created(device: &ID3D12Device) {
    crate::survey::install();
    static FIRST_DEVICE: Once = Once::new();
    log_once(&FIRST_DEVICE, || {
        // SAFETY: The factory holds this native device reference through the call.
        let luid = unsafe { device.GetAdapterLuid() };
        format!(
            "created host D3D12 device (adapter LUID {:08x}:{:08x})",
            luid.HighPart, luid.LowPart
        )
    });
}

pub(super) fn submitted(kind: D3D12_COMMAND_LIST_TYPE, count: u32) {
    static FIRST_SUBMISSION: Once = Once::new();
    log_once(&FIRST_SUBMISSION, || {
        let queue = match kind {
            D3D12_COMMAND_LIST_TYPE_DIRECT => "direct",
            D3D12_COMMAND_LIST_TYPE_COMPUTE => "compute",
            D3D12_COMMAND_LIST_TYPE_COPY => "copy",
            _ => "unknown",
        };
        format!("submitted first guest batch (queue {queue}, command lists {count})")
    });
}

pub(super) fn presented(width: u64, height: u32) {
    static FIRST_PRESENT: Once = Once::new();
    log_once(&FIRST_PRESENT, || {
        format!("PresentX presented first frame ({width}x{height})")
    });
}

/// A shader's container ID and chunk types let the package tools locate the
/// original program without copying shader bytecode into the game log.
pub(super) fn compute_failure(shader: &[u8], status: i32) {
    static FIRST_FAILURE: Once = Once::new();
    log_once(&FIRST_FAILURE, || {
        let description = if shader.starts_with(b"DXBC") && shader.len() >= 32 {
            let count =
                u32::from_le_bytes([shader[28], shader[29], shader[30], shader[31]]) as usize;
            let mut chunks = Vec::new();
            for entry in shader[32..].as_chunks::<4>().0.iter().take(count.min(16)) {
                let offset = u32::from_le_bytes(*entry) as usize;
                if let Some(tag) = shader.get(offset..).and_then(|rest| rest.get(..4))
                    && tag.iter().all(u8::is_ascii_alphanumeric)
                    && let Ok(name) = std::str::from_utf8(tag)
                {
                    chunks.push(name);
                }
            }
            let id: String = shader[4..20].iter().map(|b| format!("{b:02x}")).collect();
            format!("DXBC {id}, chunks {}", chunks.join("/"))
        } else {
            "unrecognized shader container".to_owned()
        };
        format!(
            "compute pipeline creation failed ({status:#010x}, {} bytes, {description})",
            shader.len()
        )
    });
}

pub(super) fn pipeline_stream(bytes: usize, error: Option<i32>) {
    static SUCCESS: Once = Once::new();
    static FAILURE: Once = Once::new();
    match error {
        Some(code) => log_once(&FAILURE, || {
            format!("slot 86 pipeline stream ({bytes} bytes) failed: {code:#010x}")
        }),
        None => log_once(&SUCCESS, || {
            format!("created pipeline from Xbox slot 86 stream ({bytes} bytes)")
        }),
    }
}

pub(super) fn recording_error(reason: std::fmt::Arguments<'_>) {
    static FIRST: Once = Once::new();
    log_once(&FIRST, || format!("command list cannot record {reason}"));
}

// Multiple pipelines can fail on worker threads during the same frame. Keep a
// bounded list of distinct failures so the exact shader pair can be prepared.
pub(crate) fn pipeline_stages(stages: &[Option<(String, bool)>; 8], status: i32, linked: bool) {
    use std::sync::Mutex;
    static REPORTED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let names = ["VS", "PS", "DS", "HS", "GS", "CS", "AS", "MS"];
    let shaders: Vec<_> = names
        .iter()
        .zip(stages)
        .filter_map(|(stage, entry)| {
            entry.as_ref().map(|(hash, cached)| {
                format!(
                    "{stage}={hash}({})",
                    if *cached { "prepared" } else { "original" }
                )
            })
        })
        .collect();
    let key = format!("{status:#010x}/{linked}/{}", shaders.join(" "));
    let id = {
        let mut reported = REPORTED.lock().unwrap_or_else(|e| e.into_inner());
        if reported.len() == 32 || reported.contains(&key) {
            return;
        }
        reported.push(key);
        reported.len()
    };
    // Wine clips long debug strings, and worker threads can interleave their
    // messages. Carry a shared number and keep at most two stages per line.
    for pair in shaders.chunks(2) {
        log_once(&Once::new(), || {
            format!(
                "slot 86 pipeline stage failure #{id} {status:#010x} linked={linked} {}",
                pair.join(" ")
            )
        });
    }
}

// A title checks the returned resource, not the HRESULT. Report the first
// distinct failed creations so the unsupported description is visible.
pub(crate) fn resource_failure(
    slot: u32, status: windows::core::HRESULT, initial: i32,
    desc: Option<&windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_DESC>,
) {
    use std::sync::Mutex;
    static SEEN: Mutex<Vec<(u32, i32, i32, u32)>> = Mutex::new(Vec::new());
    if status.is_ok() { return; }
    let key = desc.map_or((slot, status.0, 0, 0), |d| (slot, status.0, d.Format.0, d.Flags.0 as u32));
    {
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if seen.len() >= 32 || seen.contains(&key) { return; }
        seen.push(key);
    }
    let detail = desc.map_or_else(|| "no description".to_owned(), |d| format!(
        "dim={} {}x{}x{} mips={} format={} samples={} layout={:#x} flags={:#x} align={:#x}",
        d.Dimension.0, d.Width, d.Height, d.DepthOrArraySize, d.MipLevels, d.Format.0,
        d.SampleDesc.Count, d.Layout.0, d.Flags.0, d.Alignment));
    log_once(&Once::new(), || format!("resource slot {slot} failed {:#010x}: {detail} initial={initial:#x}", status.0));
}

/// `TUXBOX_WATCH_ADDRESS=<hex>[,<hex radius>]`: report each resource whose
/// guest memory range comes within the radius of that address. Used to find
/// the writer of corrupted title memory.
pub(crate) fn watch(kind: &str, start: u64, len: u64, detail: impl FnOnce() -> String) {
    static ADDRESS: std::sync::LazyLock<Option<(u64, u64)>> = std::sync::LazyLock::new(|| {
        let text = std::env::var("TUXBOX_WATCH_ADDRESS").ok()?;
        let (at, radius) = text.split_once(',').unwrap_or((&text, "0"));
        let hex = |t: &str| u64::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok();
        Some((hex(at)?, hex(radius)?))
    });
    let Some((address, radius)) = *ADDRESS else { return };
    let end = start.saturating_add(len);
    if !(start <= address.saturating_add(radius) && address.saturating_sub(radius) < end) {
        return;
    }
    let rva = crate::survey::title_rva().unwrap_or(0);
    let line = format!("watch {address:#x}: {kind} {start:#x}+{len:#x} {} from title RVA {rva:#x}", detail());
    log_once(&Once::new(), || line);
}
