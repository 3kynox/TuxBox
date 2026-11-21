//! Title-facing front panel functions for the 256 by 64 devkit display.
mod transport;
use transport::{Transport, transport};
use windows_sys::Win32::Foundation::{E_INVALIDARG, E_POINTER, S_OK};

// The packaged transport opens the Game OS front panel device path.
const NO_PANEL: i32 = 0x8007_0003u32 as i32;
const WIDTH: u32 = 256;
const HEIGHT: u32 = 64;
const R8_UNORM: u32 = 61;
const LED_BITS: [u16; 5] = [0x8000, 0x2000, 0x0002, 0x0800, 0x0200];

// The reviewed DLL returns the second connection attempt's status directly on failure.
fn with_panel(action: impl FnOnce(&Transport) -> i32) -> i32 {
    let Some(panel) = transport() else {
        return NO_PANEL;
    };
    // SAFETY: connect takes no parameters and the transport is pinned.
    unsafe {
        if (panel.connect)() < 0 {
            return (panel.connect)();
        }
    }
    action(panel)
}

#[unsafe(export_name = "XFrontPanelIsSupported")]
pub extern "system" fn supported() -> bool {
    transport().is_some_and(|panel| unsafe { (panel.connect)() >= 0 })
}

/// # Safety
/// Both outputs, if supplied, must point to writable 32-bit integers.
#[unsafe(export_name = "XFrontPanelGetScreenDimensions")]
pub unsafe extern "system" fn dimensions(height: *mut u32, width: *mut u32) -> i32 {
    if height.is_null() || width.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: The caller supplies two writable outputs.
    unsafe {
        height.write_unaligned(HEIGHT);
        width.write_unaligned(WIDTH);
    }
    S_OK
}

/// # Safety
/// A non-null output must point to a writable DXGI_FORMAT.
#[unsafe(export_name = "XFrontPanelGetScreenPixelFormat")]
pub unsafe extern "system" fn pixel_format(format: *mut u32) -> i32 {
    if format.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: A non-null output identifies a writable 32-bit value.
    unsafe {
        format.write_unaligned(R8_UNORM);
    }
    S_OK
}

#[unsafe(export_name = "XFrontPanelSetDisplayName")]
pub extern "system" fn set_name(name: *const core::ffi::c_void) -> i32 {
    if name.is_null() { E_INVALIDARG } else { S_OK }
}

/// # Safety
/// If a panel is connected, the caller must supply a writable button mask.
#[unsafe(export_name = "XFrontPanelGetButtonStates")]
pub unsafe extern "system" fn buttons(out: *mut u32) -> i32 {
    with_panel(|panel| unsafe { (panel.keys)(out) })
}

/// # Safety
/// A non-null output must identify a writable 32-bit light mask.
#[unsafe(export_name = "XFrontPanelGetLightStates")]
pub unsafe extern "system" fn lights(out: *mut u32) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: The caller supplies a writable output. The transport writes a u16 temporary.
    unsafe { out.write_unaligned(0) };
    with_panel(|panel| {
        let mut raw = 0;
        if unsafe { (panel.read_leds)(&mut raw) } < 0 {
            return unsafe { (panel.read_leds)(&mut raw) };
        }
        let mut mask = 0;
        for (i, bit) in LED_BITS.iter().enumerate() {
            if raw & bit != 0 {
                mask |= 1 << i;
            }
        }
        unsafe { out.write_unaligned(mask) };
        S_OK
    })
}

#[unsafe(export_name = "XFrontPanelSetLightStates")]
pub extern "system" fn set_lights(mask: u32) -> i32 {
    let mut raw = 0;
    for (i, bit) in LED_BITS.iter().enumerate() {
        if mask & (1 << i) != 0 {
            raw |= bit;
        }
    }
    with_panel(|panel| unsafe { (panel.write_leds)(raw) })
}

/// # Safety
/// A correctly sized, non-null buffer must contain 256 by 64 bytes.
#[unsafe(export_name = "XFrontPanelPresentBuffer")]
pub unsafe extern "system" fn present(size: u32, pixels: *const u8) -> i32 {
    if size != WIDTH * HEIGHT {
        return E_INVALIDARG;
    }
    if pixels.is_null() {
        return E_POINTER;
    }
    with_panel(|panel| unsafe { (panel.present)(pixels, 8) })
}
