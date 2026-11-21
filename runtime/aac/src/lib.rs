//! AAC transform adapter for Proton's installed Wine/GStreamer decoder.
use core::{ffi::c_void, ptr};
use std::sync::{Mutex, MutexGuard};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, E_UNEXPECTED, S_FALSE,
    S_OK,
};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IClassFactory, IClassFactory_Impl,
};
use windows::core::{BOOL, GUID, HRESULT, IUnknown, Interface, Ref, Result, implement};

// This class is hosted in Wine's media backend. It differs from the guest AAC
// class so COM activation cannot resolve recursively to our own class factory.
const WINE_AAC_DECODER: GUID = GUID::from_u128(0xe7889a8a_2083_4844_8370_5ee349b14503);
struct ServerState {
    objects: usize,
    locks: usize,
}

static SERVER: Mutex<ServerState> = Mutex::new(ServerState {
    objects: 0,
    locks: 0,
});

fn server() -> MutexGuard<'static, ServerState> {
    SERVER.lock().unwrap_or_else(|poison| poison.into_inner())
}

struct ModuleRef;

impl ModuleRef {
    fn new() -> Self {
        server().objects += 1;
        Self
    }
}

impl Drop for ModuleRef {
    fn drop(&mut self) {
        server().objects -= 1;
    }
}

#[implement(IClassFactory)]
struct Factory(ModuleRef);

#[allow(non_snake_case)]
impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<'_, IUnknown>,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> Result<()> {
        if out.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            out.write(ptr::null_mut());
        }
        if iid.is_null() {
            return Err(E_POINTER.into());
        }
        if outer.as_ref().is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        // SAFETY: COM is initialized by the guest. The separate decoder class
        // returns an owned IMFTransform and never receives our guest IID.
        let host = unsafe { CoCreateInstance(&WINE_AAC_DECODER, None, CLSCTX_INPROC_SERVER)? };
        let decoder: IMFTransform = Decoder {
            host,
            negotiation: Mutex::default(),
            _module: ModuleRef::new(),
        }
        .into();
        unsafe { decoder.query(iid, out).ok() }
    }
    fn LockServer(&self, lock: BOOL) -> Result<()> {
        let mut state = server();
        state.locks = if lock.as_bool() {
            state.locks.checked_add(1)
        } else {
            state.locks.checked_sub(1)
        }
        .ok_or(E_UNEXPECTED)?;
        Ok(())
    }
}

#[derive(Default)]
struct Negotiation {
    output: Option<IMFMediaType>,
    draining: bool,
}

#[implement(IMFTransform)]
struct Decoder {
    host: IMFTransform,
    negotiation: Mutex<Negotiation>,
    _module: ModuleRef,
}

// Methods whose host and Xbox contracts match pass the guest's original COM arguments.
macro_rules! forward {
    ($name:ident($($arg:ident: $ty:ty),* $(,)?) -> $result:ty) => {
        fn $name(&self, $($arg: $ty),*) -> Result<$result> {
            unsafe { self.host.$name($($arg),*) }
        }
    };
}

#[allow(non_snake_case)]
impl IMFTransform_Impl for Decoder_Impl {
    forward!(
        GetStreamLimits(
            in_min: *mut u32,
            in_max: *mut u32,
            out_min: *mut u32,
            out_max: *mut u32,
        ) -> ()
    );
    forward!(GetStreamCount(inputs: *mut u32, outputs: *mut u32) -> ());
    fn GetStreamIDs(&self, n: u32, inputs: *mut u32, m: u32, outputs: *mut u32) -> Result<()> {
        unsafe { (self.host.vtable().GetStreamIDs)(self.host.as_raw(), n, inputs, m, outputs).ok() }
    }
    forward!(GetInputStreamInfo(id: u32, info: *mut MFT_INPUT_STREAM_INFO) -> ());
    forward!(GetOutputStreamInfo(id: u32) -> MFT_OUTPUT_STREAM_INFO);
    forward!(GetAttributes() -> IMFAttributes);
    forward!(GetInputStreamAttributes(id: u32) -> IMFAttributes);
    forward!(GetOutputStreamAttributes(id: u32) -> IMFAttributes);
    forward!(DeleteInputStream(id: u32) -> ());
    forward!(AddInputStreams(count: u32, ids: *const u32) -> ());
    forward!(GetInputAvailableType(id: u32, index: u32) -> IMFMediaType);
    forward!(GetOutputAvailableType(id: u32, index: u32) -> IMFMediaType);
    forward!(GetInputCurrentType(id: u32) -> IMFMediaType);
    forward!(GetOutputCurrentType(id: u32) -> IMFMediaType);
    forward!(GetInputStatus(id: u32) -> u32);
    forward!(GetOutputStatus() -> u32);
    forward!(SetOutputBounds(start: i64, end: i64) -> ());

    fn SetInputType(&self, id: u32, ty: Ref<'_, IMFMediaType>, flags: u32) -> Result<()> {
        let mut state = self.negotiation.lock().map_err(|_| E_FAIL)?;
        unsafe {
            self.host.SetInputType(id, ty.as_ref(), flags)?;
        }
        if flags & MFT_SET_TYPE_TEST_ONLY.0 as u32 == 0 {
            state.output = None;
            state.draining = false;
        }
        Ok(())
    }
    fn SetOutputType(&self, id: u32, ty: Ref<'_, IMFMediaType>, flags: u32) -> Result<()> {
        let mut state = self.negotiation.lock().map_err(|_| E_FAIL)?;
        if flags & MFT_SET_TYPE_TEST_ONLY.0 as u32 != 0 {
            return unsafe { self.host.SetOutputType(id, ty.as_ref(), flags) };
        }
        // Remember the accepted media type independently of guest mutations.
        let snapshot = match ty.as_ref() {
            Some(ty) => unsafe {
                let copy = MFCreateMediaType()?;
                ty.CopyAllItems(&copy)?;
                Some(copy)
            },
            None => None,
        };
        unsafe {
            self.host.SetOutputType(id, ty.as_ref(), flags)?;
        }
        state.output = snapshot;
        Ok(())
    }
    fn ProcessEvent(&self, id: u32, event: Ref<'_, IMFMediaEvent>) -> Result<()> {
        unsafe { self.host.ProcessEvent(id, event.as_ref()) }
    }
    fn ProcessMessage(&self, msg: MFT_MESSAGE_TYPE, param: usize) -> Result<()> {
        let mut state = self.negotiation.lock().map_err(|_| E_FAIL)?;
        unsafe {
            self.host.ProcessMessage(msg, param)?;
        }
        match msg {
            MFT_MESSAGE_COMMAND_DRAIN => state.draining = true,
            MFT_MESSAGE_COMMAND_FLUSH
            | MFT_MESSAGE_NOTIFY_START_OF_STREAM
            | MFT_MESSAGE_NOTIFY_BEGIN_STREAMING => state.draining = false,
            _ => {}
        }
        Ok(())
    }
    fn ProcessInput(&self, id: u32, sample: Ref<'_, IMFSample>, flags: u32) -> Result<()> {
        let _guard = self.negotiation.lock().map_err(|_| E_FAIL)?;
        unsafe { self.host.ProcessInput(id, sample.as_ref(), flags) }
    }
    fn ProcessOutput(
        &self,
        flags: u32,
        count: u32,
        buffers: *mut MFT_OUTPUT_DATA_BUFFER,
        status: *mut u32,
    ) -> Result<()> {
        if buffers.is_null() || status.is_null() {
            return Err(E_POINTER.into());
        }
        let state = self.negotiation.lock().map_err(|_| E_FAIL)?;
        let host = &self.host;
        // SAFETY: COM supplies count writable buffers. Retain caller ownership of
        // samples and leave any returned events or genuine format changes visible.
        unsafe {
            let call =
                move || (host.vtable().ProcessOutput)(host.as_raw(), flags, count, buffers, status);
            let single = count == 1;
            let original = if single {
                (*buffers).pSample.as_ref().map(Interface::as_raw)
            } else {
                None
            };
            let mut hr = call();
            if hr == MF_E_TRANSFORM_STREAM_CHANGE
                && single
                && (*buffers).pEvents.is_none()
                && (*buffers).pSample.as_ref().map(Interface::as_raw) == original
            {
                // GE-Proton requests a format change after its first AAC packet
                // even when it can produce exactly the PCM format already selected.
                if let Some(ty) = &state.output
                    && host.SetOutputType((*buffers).dwStreamID, ty, 0).is_ok()
                {
                    (*buffers).dwStatus = 0;
                    *status = 0;
                    hr = call();
                }
            }
            if state.draining && hr == MF_E_END_OF_STREAM {
                return Err(MF_E_TRANSFORM_NEED_MORE_INPUT.into());
            }
            hr.ok()
        }
    }
}

/// # Safety
/// The class and interface IDs must be readable GUIDs, and out must be writable.
#[unsafe(export_name = "DllGetClassObject")]
pub unsafe extern "system" fn get_class(
    class: *const GUID,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe {
        out.write(ptr::null_mut());
    }
    if class.is_null() || iid.is_null() {
        return E_POINTER;
    }
    if unsafe { *class } != CLSID_MSAACDecMFT {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory(ModuleRef::new()).into();
    unsafe { factory.query(iid, out) }
}

#[unsafe(export_name = "DllCanUnloadNow")]
pub extern "system" fn can_unload() -> HRESULT {
    let state = server();
    if state.objects == 0 && state.locks == 0 {
        S_OK
    } else {
        S_FALSE
    }
}
