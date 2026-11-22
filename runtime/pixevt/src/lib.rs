//! Title-facing PIX capture status and optional counter logging.
//!
//! No Xbox PIX capture backend is currently available. Calls that would start
//! or stop a capture report the same unavailable result as the reference DLL.
use std::{io::{self, Write}, sync::OnceLock};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;

const NOT_IMPLEMENTED: i32 = 0x8000_4001u32 as i32;
const UNAVAILABLE: i32 = 0x8000_ffffu32 as i32;
static TRACE_COUNTERS: OnceLock<bool> = OnceLock::new();

/// The reviewed legacy entry point is unconditionally unavailable.
#[unsafe(export_name = "PIXBeginCapture1")]
pub extern "system" fn begin_capture_legacy(_flags: u32, _params: *const core::ffi::c_void) -> i32 {
    NOT_IMPLEMENTED
}

/// An Xbox PIX capture backend has not been attached.
#[unsafe(export_name = "PIXBeginCapture2")]
pub extern "system" fn begin_capture(_flags: u32, _params: *const core::ffi::c_void) -> i32 {
    UNAVAILABLE
}

/// There is no capture to end without a backend.
#[unsafe(export_name = "PIXEndCapture")]
pub extern "system" fn end_capture(_discard: i32) -> i32 {
    UNAVAILABLE
}

/// No PIX capture is active.
#[unsafe(export_name = "PIXGetCaptureState")]
pub extern "system" fn capture_state() -> u32 {
    0
}

/// Route a title counter to the diagnostic console when requested.
///
/// # Safety
/// When tracing is enabled, a non-null name must address a readable UTF-16
/// null-terminated string (up to 512 code units). The title owns the string.
#[unsafe(export_name = "PIXReportCounter")]
pub unsafe extern "system" fn report_counter(name: *const u16, value: f32) {
    if !*TRACE_COUNTERS.get_or_init(|| std::env::var_os("TUXBOX_TRACE_PIX").is_some_and(|v| v != "0")) {
        return;
    }
    let mut units = Vec::new();
    // SAFETY: The guest contract supplies a readable UTF-16 string; bounded
    // traversal avoids unbounded work if its terminator is unexpectedly absent.
    if !name.is_null() {
        while units.len() < 512 {
            let code = unsafe { name.add(units.len()).read_unaligned() };
            if code == 0 { break; }
            units.push(code);
        }
    }
    let label = if name.is_null() { "<null>".to_owned() } else { String::from_utf16_lossy(&units) };
    let label = label.replace(['\r', '\n'], " ");
    let message = format!("TuxBox PIX counter: {label} = {value}");
    if std::env::var_os("TUXBOX_LOG").as_deref() == Some(std::ffi::OsStr::new("1")) {
        let mut bytes = message.into_bytes();
        bytes.push(b'\n');
        bytes.push(0);
        // SAFETY: The UTF-8 message has a final NUL and no interior NUL.
        unsafe { OutputDebugStringA(bytes.as_ptr()) };
    } else {
        let _ = writeln!(io::stderr().lock(), "{message}");
    }
}
