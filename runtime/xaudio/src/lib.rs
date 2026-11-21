//! Xbox XAudio2.9 ordinals over the Windows system audio engine.
use core::ffi::{CStr, c_void};
use core::ptr;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{E_NOTIMPL, E_POINTER, FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleHandleExW, GetProcAddress,
    LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::core::{GUID, HRESULT};

type Object = *mut *mut c_void;
type Procedure = unsafe extern "system" fn() -> isize;
type Create = unsafe extern "system" fn(Object, u32, u32) -> HRESULT;
type Versioned = unsafe extern "system" fn(Object, u32, u32, u32) -> HRESULT;
type Apo = unsafe extern "system" fn(Object) -> HRESULT;
type Effect = unsafe extern "system" fn(*const GUID, Object, *const c_void, u32) -> HRESULT;
type Initialize = unsafe extern "system" fn(u32, f32, *mut u8) -> HRESULT;
type Calculate =
    unsafe extern "system" fn(*const u8, *const c_void, *const c_void, u32, *mut c_void);

struct Engine {
    create: Create,
    versioned: Option<Versioned>,
    reverb: Apo,
    meter: Apo,
    effect: Effect,
    initialize: Initialize,
    calculate: Calculate,
}

// GetSystemDirectoryW returns a length including the NUL only when the buffer
// is too small. A full path distinguishes the system DLL from our guest DLL.
fn system_path() -> Option<Vec<u16>> {
    let mut path = vec![0; 260];
    loop {
        let len = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) };
        if len == 0 {
            return None;
        }
        if (len as usize) < path.len() {
            path.truncate(len as usize);
            path.extend("\\xaudio2_9.dll\0".encode_utf16());
            return Some(path);
        }
        path.resize(len as usize, 0);
    }
}

struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

impl Engine {
    fn load() -> Option<Self> {
        let path = system_path()?;
        // SAFETY: the full system path is terminated, and borrowed code pointers
        // remain valid until the library is pinned below. Never dispatch to self
        // if the loader satisfied the request with the guest DLL.
        let module =
            unsafe { LoadLibraryExW(path.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            return None;
        }
        let library = Library(module);
        let mut own = ptr::null_mut();
        let found = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                Self::load as *const u16,
                &mut own,
            )
        };
        if found == 0 || own == library.0 {
            return None;
        }
        let symbol = |name: &CStr| unsafe { GetProcAddress(library.0, name.as_ptr().cast()) };
        // These are the named desktop signatures. The versioned factory is
        // ordinal 9 in the reviewed console DLL but ordinal 7 on Proton.
        let engine = unsafe {
            Self {
                create: core::mem::transmute::<Procedure, Create>(symbol(c"XAudio2Create")?),
                versioned: symbol(c"XAudio2CreateWithVersionInfo")
                    .map(|f| core::mem::transmute::<Procedure, Versioned>(f)),
                reverb: core::mem::transmute::<Procedure, Apo>(symbol(c"CreateAudioReverb")?),
                meter: core::mem::transmute::<Procedure, Apo>(symbol(c"CreateAudioVolumeMeter")?),
                effect: core::mem::transmute::<Procedure, Effect>(symbol(c"CreateFX")?),
                initialize: core::mem::transmute::<Procedure, Initialize>(symbol(
                    c"X3DAudioInitialize",
                )?),
                calculate: core::mem::transmute::<Procedure, Calculate>(symbol(
                    c"X3DAudioCalculate",
                )?),
            }
        };
        // Returned COM objects can outlive the caller's reference to this DLL.
        // Pin the system DLL once, then release the temporary LoadLibrary handle.
        let mut pinned = ptr::null_mut();
        let ok = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                library.0.cast(),
                &mut pinned,
            )
        };
        (ok != 0).then_some(engine)
    }
}

fn host() -> Option<&'static Engine> {
    static ENGINE: OnceLock<Option<Engine>> = OnceLock::new();
    ENGINE.get_or_init(Engine::load).as_ref()
}

unsafe fn create_object(out: Object, create: impl FnOnce(&Engine) -> HRESULT) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // The caller supplies writable storage for one returned COM interface.
    unsafe { out.write(ptr::null_mut()) };
    host().map_or(E_NOTIMPL, create)
}

#[unsafe(no_mangle)]
unsafe extern "system" fn tuxbox_audio_create(out: Object, flags: u32, processors: u32) -> HRESULT {
    unsafe { create_object(out, |engine| (engine.create)(out, flags, processors)) }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn tuxbox_audio_versioned(
    out: Object,
    flags: u32,
    processors: u32,
    version: u32,
) -> HRESULT {
    unsafe {
        create_object(out, |engine| {
            engine
                .versioned
                .map_or(E_NOTIMPL, |create| create(out, flags, processors, version))
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn tuxbox_audio_reverb(out: Object) -> HRESULT {
    unsafe { create_object(out, |engine| (engine.reverb)(out)) }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn tuxbox_audio_meter(out: Object) -> HRESULT {
    unsafe { create_object(out, |engine| (engine.meter)(out)) }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn tuxbox_audio_effect(
    class: *const GUID,
    out: Object,
    data: *const c_void,
    len: u32,
) -> HRESULT {
    unsafe { create_object(out, |engine| (engine.effect)(class, out, data, len)) }
}

#[unsafe(no_mangle)]
unsafe extern "system" fn tuxbox_audio_initialize(
    mask: u32,
    speed: f32,
    handle: *mut u8,
) -> HRESULT {
    host().map_or(E_NOTIMPL, |engine| unsafe {
        (engine.initialize)(mask, speed, handle)
    })
}

#[unsafe(no_mangle)]
unsafe extern "system" fn tuxbox_audio_calculate(
    handle: *const u8,
    listener: *const c_void,
    emitter: *const c_void,
    flags: u32,
    dsp: *mut c_void,
) {
    // A successful Initialize selected the system implementation for this
    // opaque handle. This void method cannot return a failure HRESULT.
    let Some(engine) = host() else {
        std::process::abort()
    };
    unsafe { (engine.calculate)(handle, listener, emitter, flags, dsp) }
}
