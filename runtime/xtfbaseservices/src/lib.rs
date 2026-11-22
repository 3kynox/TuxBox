//! Process-wide nested assertion enable state for XTF diagnostics.
use std::sync::Mutex;

struct AssertState {
    enabled: u8,
    previous: Vec<u8>,
}

static ASSERT_STATE: Mutex<AssertState> = Mutex::new(AssertState {
    enabled: 1,
    previous: Vec::new(),
});

#[unsafe(export_name = "XtfErrAreAssertsEnabled")]
pub extern "system" fn asserts_enabled() -> u8 {
    ASSERT_STATE.lock().unwrap_or_else(|e| e.into_inner()).enabled
}

#[unsafe(export_name = "XtfErrPushAssertsEnabled")]
pub extern "system" fn push_asserts_enabled(enabled: u8) {
    let mut state = ASSERT_STATE.lock().unwrap_or_else(|e| e.into_inner());
    let old = state.enabled;
    state.previous.push(old);
    state.enabled = enabled;
}

#[unsafe(export_name = "XtfErrPopAssertsEnabled")]
pub extern "system" fn pop_asserts_enabled() {
    let mut state = ASSERT_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(previous) = state.previous.pop() {
        state.enabled = previous;
    }
}

/// Return the label used by XTF diagnostic output for a severity level.
#[unsafe(export_name = "XtfLogGetDebugType")]
pub extern "system" fn get_debug_type(level: u32) -> *const std::ffi::c_char {
    match level {
        1 => c"Assertion".as_ptr(),
        2 => c"Error".as_ptr(),
        3 => c"Warning".as_ptr(),
        4 => c"Trace".as_ptr(),
        5 => c"Spam".as_ptr(),
        _ => std::ptr::null(),
    }
}

/// Return the Windows console color attributes for a severity level.
#[unsafe(export_name = "XtfLogGetTextColors")]
pub extern "system" fn get_text_colors(level: u32) -> u16 {
    match level {
        1 => 124,
        2 => 12,
        3 => 14,
        4 => 11,
        5 => 3,
        _ => 0,
    }
}
