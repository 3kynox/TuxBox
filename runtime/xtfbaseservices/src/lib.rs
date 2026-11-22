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
