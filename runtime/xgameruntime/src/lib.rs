//! Xbox Game Runtime entry points used by the statically linked GDK wrappers.
//! The queue vtable is mapped in evidence/matrix-awakens/runtime-wrappers.csv.
mod queue;

use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;
use windows_sys::core::GUID;

const OK: i32 = 0;
const INVALID: i32 = 0x80070057u32 as i32;
const NO_INTERFACE: i32 = 0x80004002u32 as i32;
const POINTER: i32 = 0x80004003u32 as i32;
const NOT_IMPLEMENTED: i32 = 0x80004001u32 as i32;
const ABORT: i32 = 0x80004004u32 as i32;
const NOT_INITIALIZED: i32 = 0x89240100u32 as i32;
const THREADING: GUID = GUID::from_u128(0x073b7dcb_1fcf_4030_94be_e3c9eb623428);
const UNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
static INITIALIZATIONS: AtomicU32 = AtomicU32::new(0);

fn same(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

#[unsafe(no_mangle)]
extern "system" fn InitializeApiImpl(build: u32, revision: u32) -> i32 {
    InitializeApiImplEx(build, revision, 0)
}

#[unsafe(no_mangle)]
extern "system" fn InitializeApiImplEx(_build: u32, _revision: u32, _flags: u64) -> i32 {
    if INITIALIZATIONS.fetch_add(1, Ordering::AcqRel) == 0 {
        unsafe { OutputDebugStringA(c"TuxBox xgameruntime: initialized\n".as_ptr().cast()) };
    }
    OK
}

#[unsafe(no_mangle)]
extern "system" fn UninitializeApiImpl() {
    let _ = INITIALIZATIONS.try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1));
}

#[unsafe(no_mangle)]
unsafe extern "system" fn XErrorReport(_error: i32, message: *const c_char) {
    if !message.is_null() {
        // The GDK error message is a borrowed, NUL-terminated ANSI string.
        unsafe { OutputDebugStringA(message.cast()) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn QueryApiImpl(
    class: *const GUID,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() {
        return POINTER;
    }
    unsafe {
        out.write(ptr::null_mut());
    }
    let (Some(class), Some(iid)) = (unsafe { class.as_ref() }, unsafe { iid.as_ref() }) else {
        return INVALID;
    };
    if INITIALIZATIONS.load(Ordering::Acquire) == 0 {
        return NOT_INITIALIZED;
    }
    if !same(class, &THREADING) {
        return NO_INTERFACE;
    }
    unsafe { query(&THREAD_OBJECT, iid, out) }
}

// Function addresses only, all tables live for the lifetime of the DLL.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Slot(*const ());
unsafe impl Sync for Slot {}
macro_rules! slot {
    ($func:path) => {
        Slot($func as *const ())
    };
}

#[repr(C)]
struct Object {
    vtable: &'static [Slot; 32],
    references: AtomicU32,
}

unsafe extern "system" fn query(this: &Object, iid: *const GUID, out: *mut *mut c_void) -> i32 {
    if out.is_null() {
        return POINTER;
    }
    unsafe {
        out.write(ptr::null_mut());
    }
    let Some(iid) = (unsafe { iid.as_ref() }) else {
        return INVALID;
    };
    if !same(iid, &THREADING) && !same(iid, &UNKNOWN) {
        return NO_INTERFACE;
    }
    add_ref(this);
    unsafe {
        out.write(ptr::from_ref(this).cast_mut().cast());
    }
    OK
}

extern "system" fn add_ref(this: &Object) -> u32 {
    this.references.fetch_add(1, Ordering::Relaxed) + 1
}
extern "system" fn release(this: &Object) -> u32 {
    // The DLL owns the permanent interface. The external count can reach zero.
    this.references.fetch_sub(1, Ordering::Relaxed) - 1
}

extern "system" fn unsupported(_: &Object) -> i32 {
    NOT_IMPLEMENTED
}
extern "system" fn idle_thread(_: &Object) {} // No guest thread is designated time-sensitive.
static THREAD_TABLE: [Slot; 32] = {
    let mut t = [slot!(unsupported); 32];
    t[0] = slot!(query);
    t[1] = slot!(add_ref);
    t[2] = slot!(release);
    t[12] = slot!(queue::create);
    t[15] = slot!(queue::duplicate);
    t[16] = slot!(queue::dispatch);
    t[17] = slot!(queue::close);
    t[18] = slot!(queue::submit);
    t[19] = slot!(queue::submit_delayed);
    t[22] = slot!(queue::terminate);
    t[25] = slot!(queue::process_queue);
    t[29] = slot!(idle_thread);
    t
};
static THREAD_OBJECT: Object = Object {
    vtable: &THREAD_TABLE,
    references: AtomicU32::new(0),
};
