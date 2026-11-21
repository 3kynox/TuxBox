//! Outgoing Game Chat packets generated when the title registers remote users.
use super::{INVALID_STATE, manager};
use core::{mem, ptr};
use windows_sys::Win32::Foundation::{E_INVALIDARG, E_OUTOFMEMORY, E_POINTER, S_OK};
use windows_sys::core::HRESULT;

#[repr(C)]
pub struct DataFrame {
    endpoint_count: u32,
    endpoints: *const u64,
    transport: u32,
    packet_size: u32,
    packet: *const u8,
}

struct Announcement {
    endpoint: u64,
    packet: u8,
}

#[derive(Default)]
pub(super) struct OutgoingFrames {
    pending: Vec<Announcement>,
    // The title borrows these three buffers together, until FinishProcessing.
    published: Vec<Announcement>,
    frames: Vec<DataFrame>,
    pointers: Vec<*const DataFrame>,
    borrowed: bool,
}

// SAFETY: the pointers refer only to this batch's stable Vec allocations.
// Moving the owner does not move those allocations.
// Session mutations run under MANAGER and keep borrowed buffers in place.
unsafe impl Send for OutgoingFrames {}

impl OutgoingFrames {
    pub(super) fn reserve_announcement(&mut self) -> Result<(), HRESULT> {
        self.pending.try_reserve(1).map_err(|_| E_OUTOFMEMORY)
    }

    pub(super) fn announce(&mut self, endpoint: u64) {
        self.pending.push(Announcement {
            endpoint,
            packet: 0x07,
        });
    }

    pub(super) fn remove_endpoint(&mut self, endpoint: u64) {
        self.pending.retain(|frame| frame.endpoint != endpoint);
    }

    fn pointer(&self) -> *const *const DataFrame {
        if self.pointers.capacity() == 0 {
            ptr::null()
        } else {
            self.pointers.as_ptr()
        }
    }

    fn start(&mut self) -> Result<u32, HRESULT> {
        if self.borrowed {
            return Err(INVALID_STATE);
        }
        let n = self.pending.len();
        let count = u32::try_from(n).map_err(|_| E_OUTOFMEMORY)?;
        self.frames.try_reserve(n).map_err(|_| E_OUTOFMEMORY)?;
        self.pointers.try_reserve(n).map_err(|_| E_OUTOFMEMORY)?;
        mem::swap(&mut self.pending, &mut self.published);
        self.frames
            .extend(self.published.iter_mut().map(|frame| DataFrame {
                endpoint_count: 1,
                endpoints: ptr::from_mut(&mut frame.endpoint),
                transport: 0, // guaranteed delivery
                packet_size: 1,
                packet: ptr::from_mut(&mut frame.packet),
            }));
        self.pointers.extend(self.frames.iter().map(ptr::from_ref));
        self.borrowed = true;
        Ok(count)
    }

    fn finish(&mut self, batch: *const *const DataFrame) -> HRESULT {
        if !self.borrowed {
            return INVALID_STATE;
        }
        if batch != self.pointer() {
            return E_INVALIDARG;
        }
        self.pointers.clear();
        self.frames.clear();
        self.published.clear();
        self.borrowed = false;
        S_OK
    }
}

/// # Safety
/// The two outputs must be writable and nonoverlapping. The title may read the
/// returned frames until FinishProcessing or Cleanup, synchronized with both.
#[unsafe(export_name = "ChatManagerStartProcessingDataFrames")]
pub unsafe extern "system" fn start(
    count: *mut u32,
    batch: *mut *const *const DataFrame,
) -> HRESULT {
    let mut manager = manager();
    let Some(session) = &mut manager.session else {
        return INVALID_STATE;
    };
    if count.is_null() || batch.is_null() {
        return E_POINTER;
    }
    let (hr, n, frames) = match session.outgoing.start() {
        Ok(n) => (S_OK, n, session.outgoing.pointer()),
        Err(hr) => (hr, 0, ptr::null()),
    };
    // SAFETY: the caller supplies storage for the count and array pointer.
    unsafe {
        count.write(n);
        batch.write(frames);
    }
    hr
}

#[unsafe(export_name = "ChatManagerFinishProcessingDataFrames")]
pub extern "system" fn finish(batch: *const *const DataFrame) -> HRESULT {
    let mut manager = manager();
    let Some(session) = &mut manager.session else {
        return INVALID_STATE;
    };
    session.outgoing.finish(batch)
}
