//! Optional XFP front-panel transport supplied with the title or system image.
use core::{ffi::CStr, mem::transmute, ptr};
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    GetProcAddress, LOAD_LIBRARY_SEARCH_APPLICATION_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
    LoadLibraryExA,
};

type Proc = unsafe extern "system" fn() -> isize;
type Connect = unsafe extern "system" fn() -> i32;
type Keys = unsafe extern "system" fn(*mut u32) -> i32;
type ReadLeds = unsafe extern "system" fn(*mut u16) -> i32;
type WriteLeds = unsafe extern "system" fn(u16) -> i32;
type Present = unsafe extern "system" fn(*const u8, u8) -> i32;

pub(super) struct Transport {
    pub connect: Connect,
    pub keys: Keys,
    pub read_leds: ReadLeds,
    pub write_leds: WriteLeds,
    pub present: Present,
}

struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: load() owns exactly one loader reference.
        unsafe { FreeLibrary(self.0) };
    }
}

fn load() -> Option<Transport> {
    // SAFETY: NUL-terminated DLL name. Restrict lookup to the title and system directories.
    let module = unsafe {
        LoadLibraryExA(
            c"xfp_gc.dll".as_ptr().cast(),
            ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_APPLICATION_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return None;
    }
    let library = Library(module);
    let symbol = |name: &CStr| unsafe { GetProcAddress(library.0, name.as_ptr().cast()) };
    // SAFETY: Each named function has the corresponding XFP transport ABI.
    let transport = unsafe {
        Transport {
            connect: transmute::<Proc, Connect>(symbol(c"XfpTryConnect")?),
            keys: transmute::<Proc, Keys>(symbol(c"XfpGetKeyState")?),
            read_leds: transmute::<Proc, ReadLeds>(symbol(c"XfpGetLEDState")?),
            write_leds: transmute::<Proc, WriteLeds>(symbol(c"XfpSetLEDState")?),
            present: transmute::<Proc, Present>(symbol(c"XfpSetDisplayBuffer")?),
        }
    };
    // SAFETY: Pin the newly loaded module before caching its code pointers.
    let mut pinned = ptr::null_mut();
    let ok = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            module.cast(),
            &mut pinned,
        )
    };
    (ok != 0).then_some(transport)
}

pub(super) fn transport() -> Option<&'static Transport> {
    static TRANSPORT: OnceLock<Option<Transport>> = OnceLock::new();
    TRANSPORT.get_or_init(load).as_ref()
}
