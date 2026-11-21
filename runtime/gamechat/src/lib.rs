//! The local Game Chat manager's remote participant roster and bitrate setting.
use core::ptr;
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::Foundation::{E_FAIL, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER, S_OK};
use windows_sys::core::HRESULT;

const INVALID_STATE: HRESULT = 0x800710dd_u32 as i32;
const NOT_FOUND: HRESULT = 0x80070490_u32 as i32;
const ROSTER_FULL: HRESULT = 0x89234201_u32 as i32;
const INVALID_XUID: HRESULT = 0x89234501_u32 as i32;
const BITRATE_24_KBPS: u32 = 2;

unsafe extern "C" {
    fn _wcstoui64(input: *const u16, end: *mut *mut u16, radix: i32) -> u64;
}

struct User {
    xuid: u64,
    endpoint: u64,
}

struct Session {
    limit: usize,
    bitrate: u32,
    endpoints: Vec<u64>,
    users: Vec<User>,
    // Parallel indices give the guest the dense array of handles required by
    // GetChatUsers. A handle is an opaque value, never a Rust object pointer.
    handles: Vec<usize>,
}

struct Manager {
    session: Option<Session>,
    next_handle: usize,
}

static MANAGER: Mutex<Manager> = Mutex::new(Manager {
    session: None,
    next_handle: 1,
});

fn manager() -> MutexGuard<'static, Manager> {
    MANAGER.lock().unwrap_or_else(|p| p.into_inner())
}

#[unsafe(export_name = "ChatManagerInitialize")]
pub extern "system" fn initialize(
    max_users: u32,
    volume: f32,
    relationship: u32,
    resolution: u32,
    speech: u32,
    manipulation: u32,
) -> HRESULT {
    let mut manager = manager();
    if manager.session.is_some() {
        return INVALID_STATE;
    }
    // The verified profile uses default volume and zero mode flags.
    if volume != 1.0 || (relationship | resolution | speech | manipulation) != 0 {
        return E_NOTIMPL;
    }
    manager.session = Some(Session {
        limit: max_users as usize,
        bitrate: BITRATE_24_KBPS,
        endpoints: Vec::new(),
        users: Vec::new(),
        handles: Vec::new(),
    });
    S_OK
}

#[unsafe(export_name = "ChatManagerCleanup")]
pub extern "system" fn cleanup() -> HRESULT {
    if manager().session.take().is_some() {
        S_OK
    } else {
        INVALID_STATE
    }
}

#[unsafe(export_name = "ChatManagerSetAudioEncodingBitrate")]
pub extern "system" fn set_bitrate(bitrate: u32) -> HRESULT {
    let mut manager = manager();
    let Some(session) = &mut manager.session else {
        return INVALID_STATE;
    };
    session.bitrate = bitrate;
    S_OK
}

/// # Safety
/// A non-null output must be writable for one 32-bit bitrate enum.
#[unsafe(export_name = "ChatManagerGetAudioEncodingBitrate")]
pub unsafe extern "system" fn get_bitrate(out: *mut u32) -> HRESULT {
    let manager = manager();
    let Some(session) = &manager.session else {
        return INVALID_STATE;
    };
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { out.write(session.bitrate) };
    S_OK
}

/// # Safety
/// The XUID must be a NUL-terminated wide string. A non-null output must
/// have space for one pointer-sized opaque user handle.
#[unsafe(export_name = "ChatManagerAddRemoteUser")]
pub unsafe extern "system" fn add_remote(
    xuid: *const u16,
    endpoint: u64,
    out: *mut usize,
) -> HRESULT {
    let mut manager = manager();
    let Manager {
        session,
        next_handle,
    } = &mut *manager;
    let Some(session) = session else {
        return INVALID_STATE;
    };
    if xuid.is_null() || out.is_null() {
        return E_POINTER;
    }
    // The native desktop CRT accepts a decimal prefix and uses UINT64_MAX
    // for an overflow. Both zero and UINT64_MAX are invalid Xbox user IDs.
    let xuid = unsafe { _wcstoui64(xuid, ptr::null_mut(), 10) };
    if matches!(xuid, 0 | u64::MAX) {
        return INVALID_XUID;
    }
    let new_endpoint = !session.endpoints.contains(&endpoint);
    if new_endpoint {
        if session.endpoints.len() >= session.limit {
            return E_FAIL;
        }
        if session.endpoints.try_reserve(1).is_err() {
            return E_OUTOFMEMORY;
        }
    }
    let result = if session.users.len() >= session.limit {
        ROSTER_FULL
    } else if let Some(index) = session.users.iter().position(|user| user.xuid == xuid) {
        unsafe { out.write(session.handles[index]) };
        S_OK
    } else {
        if session.users.try_reserve(1).is_err() || session.handles.try_reserve(1).is_err() {
            return E_OUTOFMEMORY;
        }
        let handle = *next_handle;
        let Some(next) = handle.checked_add(1) else {
            return E_OUTOFMEMORY;
        };
        *next_handle = next;
        session.users.push(User { xuid, endpoint });
        session.handles.push(handle);
        unsafe { out.write(handle) };
        S_OK
    };
    // Duplicate and full-roster requests still register a new endpoint in the
    // supplied DLL. Allocation errors above leave both endpoint and roster intact.
    if new_endpoint {
        session.endpoints.push(endpoint);
    }
    result
}

#[unsafe(export_name = "ChatManagerRemoveUser")]
pub extern "system" fn remove_user(handle: usize) -> HRESULT {
    let mut manager = manager();
    let Some(session) = &mut manager.session else {
        return INVALID_STATE;
    };
    let Some(index) = session.handles.iter().position(|&h| h == handle) else {
        return NOT_FOUND;
    };
    let user = session.users.remove(index);
    session.handles.remove(index);
    if !session
        .users
        .iter()
        .any(|other| other.endpoint == user.endpoint)
    {
        session
            .endpoints
            .retain(|&endpoint| endpoint != user.endpoint);
    }
    S_OK
}

/// # Safety
/// Outputs must be writable and nonoverlapping. The returned array is borrowed
/// from the manager. Callers must finish reading it before the next add, remove,
/// or cleanup call, including calls from other threads.
#[unsafe(export_name = "ChatManagerGetChatUsers")]
pub unsafe extern "system" fn get_users(count: *mut u32, out: *mut *const usize) -> HRESULT {
    if count.is_null() || out.is_null() {
        return E_POINTER;
    }
    let manager = manager();
    let Some(session) = &manager.session else {
        unsafe {
            count.write(0);
            out.write(ptr::null())
        };
        return INVALID_STATE;
    };
    unsafe {
        count.write(session.handles.len() as u32);
        out.write(session.handles.as_ptr());
    }
    S_OK
}

/// # Safety
/// A non-null output must be writable for one byte (the Xbox C++ bool ABI).
#[unsafe(export_name = "ChatUserGetIsLocal")]
pub unsafe extern "system" fn is_local(handle: usize, out: *mut u8) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    let manager = manager();
    unsafe { out.write(0) };
    let Some(session) = &manager.session else {
        return INVALID_STATE;
    };
    if session.handles.contains(&handle) {
        S_OK
    } else {
        NOT_FOUND
    }
}
