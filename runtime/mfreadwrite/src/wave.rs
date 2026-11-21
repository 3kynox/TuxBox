//! RIFF/WAVE integer PCM. Keep sample bytes in the caller's seekable stream.
use windows::Win32::Media::MediaFoundation::*;
use windows::core::Result;

pub(crate) struct Wave {
    pub channels: u16,
    pub rate: u32,
    pub bits: u16,
    pub block: u16,
    pub data: u64,
    pub frames: u64,
}

pub(crate) fn read_exact(stream: &IMFByteStream, mut dst: &mut [u8]) -> Result<()> {
    while !dst.is_empty() {
        let mut read = 0;
        // SAFETY: dst is writable for the entire read and read is the count output.
        unsafe { stream.Read(dst, &mut read)? };
        if read == 0 || read as usize > dst.len() {
            return Err(MF_E_INVALID_FILE_FORMAT.into());
        }
        dst = &mut dst[read as usize..];
    }
    Ok(())
}

impl Wave {
    pub fn open(stream: &IMFByteStream) -> Result<Self> {
        // SAFETY: the caller retains the stream throughout probing.
        let length = unsafe { stream.GetLength()? };
        unsafe { stream.SetCurrentPosition(0)? };
        let mut riff = [0; 12];
        read_exact(stream, &mut riff)?;
        if &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
            return Err(MF_E_UNSUPPORTED_BYTESTREAM_TYPE.into());
        }
        let end = u64::from(u32::from_le_bytes(riff[4..8].try_into().unwrap())) + 8;
        if end > length || end < 12 {
            return Err(MF_E_INVALID_FILE_FORMAT.into());
        }
        let mut offset = 12;
        let mut format = None;
        let mut data = None;
        while offset + 8 <= end {
            unsafe { stream.SetCurrentPosition(offset)? };
            let mut header = [0; 8];
            read_exact(stream, &mut header)?;
            let size = u64::from(u32::from_le_bytes(header[4..].try_into().unwrap()));
            let payload = offset + 8;
            offset = payload + size + size % 2;
            if offset > end {
                return Err(MF_E_INVALID_FILE_FORMAT.into());
            }
            match &header[..4] {
                b"fmt " if format.is_none() && size >= 16 => {
                    let mut fmt = [0; 16];
                    read_exact(stream, &mut fmt)?;
                    format = Some(fmt);
                }
                b"data" if data.is_none() => data = Some((payload, size)),
                _ => {}
            }
            if data.is_some() && format.is_some() {
                break;
            }
        }
        let fmt = format.ok_or(MF_E_INVALID_FILE_FORMAT)?;
        let (data, size) = data.ok_or(MF_E_INVALID_FILE_FORMAT)?;
        let word = |i| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
        if word(0) != 1 {
            return Err(MF_E_UNSUPPORTED_BYTESTREAM_TYPE.into());
        }
        let channels = word(2);
        let rate = u32::from_le_bytes(fmt[4..8].try_into().unwrap());
        let avg = u32::from_le_bytes(fmt[8..12].try_into().unwrap());
        let block = word(12);
        let bits = word(14);
        if channels == 0
            || rate == 0
            || block == 0
            || !matches!(bits, 8 | 16 | 24 | 32)
            || channels.checked_mul(bits / 8) != Some(block)
            || rate.checked_mul(u32::from(block)) != Some(avg)
            || size % u64::from(block) != 0
        {
            return Err(MF_E_INVALID_FILE_FORMAT.into());
        }
        Ok(Self {
            channels,
            rate,
            bits,
            block,
            data,
            frames: size / u64::from(block),
        })
    }

    pub fn timestamp(&self, frame: u64) -> i64 {
        // RIFF chunk sizes are 32-bit, so even one-byte frames fit this product.
        (frame * 10_000_000 / u64::from(self.rate)) as i64
    }

    pub fn media_type(&self) -> Result<IMFMediaType> {
        // SAFETY: each call operates on the newly allocated, owned attributes.
        unsafe {
            let ty = MFCreateMediaType()?;
            ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            ty.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            for (key, value) in [
                (MF_MT_AUDIO_NUM_CHANNELS, u32::from(self.channels)),
                (MF_MT_AUDIO_SAMPLES_PER_SECOND, self.rate),
                (MF_MT_AUDIO_BITS_PER_SAMPLE, u32::from(self.bits)),
                (MF_MT_AUDIO_BLOCK_ALIGNMENT, u32::from(self.block)),
                (
                    MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
                    self.rate * u32::from(self.block),
                ),
                (MF_MT_ALL_SAMPLES_INDEPENDENT, 1),
                (MF_MT_FIXED_SIZE_SAMPLES, 1),
            ] {
                ty.SetUINT32(&key, value)?;
            }
            Ok(ty)
        }
    }
}
