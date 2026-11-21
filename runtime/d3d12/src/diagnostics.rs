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

pub(super) fn device_created(device: &ID3D12Device) {
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
pub(crate) fn pipeline_stages(stages: &[Option<(String, bool)>; 6], status: i32, linked: bool) {
    use std::sync::Mutex;
    static REPORTED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let names = ["VS", "PS", "DS", "HS", "GS", "CS"];
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
