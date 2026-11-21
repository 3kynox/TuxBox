//! Synchronous source reader for seekable PCM WAVE byte streams.
mod wave;

use core::{ffi::c_void, ptr};
use std::sync::{Mutex, MutexGuard};
use wave::Wave;
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL, E_POINTER};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::{Com::StructuredStorage::PROPVARIANT, Variant::VT_I8};
use windows::core::{BOOL, GUID, HRESULT, Interface, OutRef, PCWSTR, Ref, Result, implement};

struct Cursor {
    stream: IMFByteStream,
    frame: u64,
    selected: bool,
}

#[implement(IMFSourceReader)]
struct Reader {
    wave: Wave,
    cursor: Mutex<Cursor>,
}

fn audio(index: u32) -> Result<()> {
    if index == 0 || index == MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32 {
        Ok(())
    } else {
        Err(MF_E_INVALIDSTREAMNUMBER.into())
    }
}

impl Reader {
    fn cursor(&self) -> Result<MutexGuard<'_, Cursor>> {
        self.cursor.lock().map_err(|_| E_FAIL.into())
    }

    fn read(&self, index: u32, control: u32) -> Result<(i64, Option<IMFSample>)> {
        if index != MF_SOURCE_READER_ANY_STREAM.0 as u32 {
            audio(index)?;
        }
        if control & !(MF_SOURCE_READER_CONTROLF_DRAIN.0 as u32) != 0 {
            return Err(E_INVALIDARG.into());
        }
        let mut cursor = self.cursor()?;
        if !cursor.selected {
            return Err(MF_E_INVALIDREQUEST.into());
        }
        // Whole PCM frames, bounded to one 64 KiB allocation and 1024 frames per call.
        let count = (self.wave.frames - cursor.frame)
            .min(1024)
            .min(65536 / u64::from(self.wave.block));
        let end = count == 0 || control != 0;
        let next = if end {
            None
        } else {
            Some(self.sample(&cursor, count)?)
        };
        let time = self.wave.timestamp(cursor.frame);
        if !end {
            cursor.frame += count;
        }
        Ok((time, next))
    }

    fn sample(&self, cursor: &Cursor, count: u64) -> Result<IMFSample> {
        let wave = &self.wave;
        let bytes = (count * u64::from(wave.block)) as u32;
        let start = wave.timestamp(cursor.frame);
        // SAFETY: the serialized cursor retains the stream; Lock provides the requested
        // writable storage. Always unlock, including on short or failed stream reads.
        unsafe {
            cursor
                .stream
                .SetCurrentPosition(wave.data + cursor.frame * u64::from(wave.block))?;
            let buffer = MFCreateMemoryBuffer(bytes)?;
            let mut address = ptr::null_mut();
            buffer.Lock(&mut address, None, None)?;
            let read = wave::read_exact(
                &cursor.stream,
                std::slice::from_raw_parts_mut(address, bytes as usize),
            );
            let unlock = buffer.Unlock();
            read?;
            unlock?;
            buffer.SetCurrentLength(bytes)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(start)?;
            sample.SetSampleDuration(wave.timestamp(cursor.frame + count) - start)?;
            Ok(sample)
        }
    }
}

#[allow(non_snake_case)]
impl IMFSourceReader_Impl for Reader_Impl {
    fn GetStreamSelection(&self, index: u32) -> Result<BOOL> {
        audio(index)?;
        Ok(self.cursor()?.selected.into())
    }

    fn SetStreamSelection(&self, index: u32, selected: BOOL) -> Result<()> {
        if index != MF_SOURCE_READER_ALL_STREAMS.0 as u32 {
            audio(index)?;
        }
        self.cursor()?.selected = selected.as_bool();
        Ok(())
    }

    fn GetNativeMediaType(&self, index: u32, format: u32) -> Result<IMFMediaType> {
        audio(index)?;
        if format != 0 {
            return Err(MF_E_NO_MORE_TYPES.into());
        }
        self.wave.media_type()
    }

    fn GetCurrentMediaType(&self, index: u32) -> Result<IMFMediaType> {
        audio(index)?;
        self.wave.media_type()
    }

    fn SetCurrentMediaType(
        &self,
        index: u32,
        reserved: *const u32,
        ty: Ref<'_, IMFMediaType>,
    ) -> Result<()> {
        audio(index)?;
        if !reserved.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let ty = ty.ok()?;
        let native = self.wave.media_type()?;
        // SAFETY: both interfaces remain valid for the comparison. Only native PCM is supported.
        unsafe {
            if ty.GetGUID(&MF_MT_MAJOR_TYPE).ok() != Some(MFMediaType_Audio)
                || ty.GetGUID(&MF_MT_SUBTYPE).ok() != Some(MFAudioFormat_PCM)
                || !native
                    .Compare(ty, MF_ATTRIBUTES_MATCH_THEIR_ITEMS)?
                    .as_bool()
            {
                return Err(MF_E_INVALIDMEDIATYPE.into());
            }
        }
        Ok(())
    }

    fn SetCurrentPosition(&self, format: *const GUID, position: *const PROPVARIANT) -> Result<()> {
        // SAFETY: COM supplies the GUID and PROPVARIANT by reference. Only read the
        // signed integer arm after checking its tag.
        unsafe {
            if *format.as_ref().ok_or(E_POINTER)? != GUID::zeroed() {
                return Err(MF_E_UNSUPPORTED_TIME_FORMAT.into());
            }
            let v = &position.as_ref().ok_or(E_POINTER)?.Anonymous.Anonymous;
            if v.vt != VT_I8 || v.Anonymous.hVal < 0 {
                return Err(E_INVALIDARG.into());
            }
            let frames = v.Anonymous.hVal as u128 * u128::from(self.wave.rate) / 10_000_000;
            self.cursor()?.frame = frames.min(u128::from(self.wave.frames)) as u64;
        }
        Ok(())
    }

    fn ReadSample(
        &self,
        index: u32,
        control: u32,
        actual: *mut u32,
        flags: *mut u32,
        timestamp: *mut i64,
        sample: OutRef<'_, IMFSample>,
    ) -> Result<()> {
        if sample.is_null() {
            return Err(E_POINTER.into());
        }
        // COM output storage may already contain a value. Do not take ownership
        // of that pointer; overwrite it with null on every failing path.
        let Some(flags) = (unsafe { flags.as_mut() }) else {
            sample.write(None)?;
            return Err(E_POINTER.into());
        };
        *flags = 0;
        let (time, next) = match self.read(index, control) {
            Ok(value) => value,
            Err(error) => {
                sample.write(None)?;
                return Err(error);
            }
        };
        // SAFETY: actual and timestamp are optional writable outputs.
        unsafe {
            if let Some(actual) = actual.as_mut() {
                *actual = 0;
            }
            if let Some(timestamp) = timestamp.as_mut() {
                *timestamp = time;
            }
        }
        if next.is_none() {
            *flags = MF_SOURCE_READERF_ENDOFSTREAM.0 as u32;
        }
        sample.write(next)
    }

    fn Flush(&self, index: u32) -> Result<()> {
        if index != MF_SOURCE_READER_ALL_STREAMS.0 as u32 {
            audio(index)?;
        }
        // All reads are synchronous. Wait for an in-progress read to finish.
        drop(self.cursor()?);
        Ok(())
    }

    fn GetServiceForStream(
        &self,
        _: u32,
        _: *const GUID,
        _: *const GUID,
        out: *mut *mut c_void,
    ) -> Result<()> {
        if out.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            out.write(ptr::null_mut());
        }
        Err(E_NOINTERFACE.into())
    }

    fn GetPresentationAttribute(&self, index: u32, key: *const GUID) -> Result<PROPVARIANT> {
        if index != MF_SOURCE_READER_MEDIASOURCE.0 as u32 {
            return Err(MF_E_INVALIDSTREAMNUMBER.into());
        }
        if unsafe { key.as_ref() }.ok_or(E_POINTER)? != &MF_PD_DURATION {
            return Err(MF_E_ATTRIBUTENOTFOUND.into());
        }
        Ok(PROPVARIANT::from(
            self.wave.timestamp(self.wave.frames) as u64
        ))
    }
}

fn open_reader(stream: IMFByteStream, attrs: Option<&IMFAttributes>) -> Result<IMFSourceReader> {
    if let Some(attrs) = attrs
        && unsafe { attrs.GetCount()? } != 0
    {
        return Err(E_NOTIMPL.into());
    }
    let needed = MFBYTESTREAM_IS_SEEKABLE | MFBYTESTREAM_IS_READABLE;
    if unsafe { stream.GetCapabilities()? } & needed != needed {
        return Err(MF_E_BYTESTREAM_NOT_SEEKABLE.into());
    }
    let position = unsafe { stream.GetCurrentPosition()? };
    let wave = Wave::open(&stream);
    // Leave input position intact if probing fails or a reader is never used.
    unsafe { stream.SetCurrentPosition(position)? };
    Ok(Reader {
        wave: wave?,
        cursor: Mutex::new(Cursor {
            stream,
            frame: 0,
            selected: true,
        }),
    }
    .into())
}

unsafe fn return_reader(
    out: *mut *mut c_void,
    create: impl FnOnce() -> Result<IMFSourceReader>,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { out.write(ptr::null_mut()) };
    create()
        .map(|reader| unsafe { out.write(reader.into_raw()) })
        .into()
}

/// Opens one synchronous PCM audio stream. Empty or absent reader attributes select defaults.
///
/// # Safety
/// Non-null input pointers must be live COM interfaces; out must be writable.
#[unsafe(export_name = "MFCreateSourceReaderFromByteStream")]
pub unsafe extern "system" fn from_stream(
    input: *mut c_void,
    attrs: *mut c_void,
    out: *mut *mut c_void,
) -> HRESULT {
    unsafe {
        return_reader(out, || {
            let stream = IMFByteStream::from_raw_borrowed(&input).ok_or(E_POINTER)?;
            open_reader(stream.clone(), IMFAttributes::from_raw_borrowed(&attrs))
        })
    }
}

/// Opens a URL as a byte stream, using the same format and seek rules as the stream factory.
///
/// # Safety
/// The URL must be a NUL-terminated UTF-16 string, non-null attributes must be a
/// live IMFAttributes, and a non-null output must be writable.
#[unsafe(export_name = "MFCreateSourceReaderFromURL")]
pub unsafe extern "system" fn from_url(
    url: PCWSTR,
    attrs: *mut c_void,
    out: *mut *mut c_void,
) -> HRESULT {
    unsafe {
        return_reader(out, || {
            if url.is_null() {
                return Err(E_POINTER.into());
            }
            let resolver = MFCreateSourceResolver()?;
            let mut kind = MF_OBJECT_INVALID;
            let mut object = None;
            resolver.CreateObjectFromURL(
                url,
                (MF_RESOLUTION_BYTESTREAM.0 | MF_RESOLUTION_READ.0) as u32,
                None,
                &mut kind,
                &mut object,
            )?;
            if kind != MF_OBJECT_BYTESTREAM {
                return Err(MF_E_UNSUPPORTED_BYTESTREAM_TYPE.into());
            }
            let stream = object.ok_or(E_NOINTERFACE)?.cast()?;
            open_reader(stream, IMFAttributes::from_raw_borrowed(&attrs))
        })
    }
}
