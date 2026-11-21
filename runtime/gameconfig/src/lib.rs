//! GameConfigHelper registry queries for desktop Windows and Proton.
use core::ffi::c_void;
use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows_sys::Win32::System::Registry::{HKEY, HKEY_LOCAL_MACHINE, HKEY_USERS, RegOpenKeyExW};
use windows_sys::core::{HRESULT, PCWSTR};

const REGISTRY_PREFIX: &[u8] = b"Software\\Microsoft\\GamingServices\\GameConfig\\";
const PATH_CAPACITY: usize = 512;

#[link(name = "runtimeobject")]
unsafe extern "system" {
    fn WindowsCreateString(value: PCWSTR, length: u32, out: *mut *mut c_void) -> HRESULT;
}

fn hresult_from_win32(status: u32) -> HRESULT {
    if status as i32 <= 0 {
        status as i32
    } else {
        (0x8007_0000 | (status & 0xffff)) as i32
    }
}

// Write the native, NUL-terminated UTF-16 path and return its length excluding NUL.
// Keep the caller's UTF-16 code units, including any unpaired surrogates.
unsafe fn registry_path(package: PCWSTR, path: &mut [u16; PATH_CAPACITY]) -> Result<u32, HRESULT> {
    let package = if package.is_null() {
        windows_sys::w!("(null)")
    } else {
        package
    };
    for (slot, &ascii) in path.iter_mut().zip(REGISTRY_PREFIX) {
        *slot = u16::from(ascii);
    }
    for (offset, slot) in path[REGISTRY_PREFIX.len()..].iter_mut().enumerate() {
        // SAFETY: The input is either the static fallback or a caller-provided
        // NUL-terminated string. Stop reading at the first NUL or size limit.
        *slot = unsafe { *package.add(offset) };
        if *slot == 0 {
            return Ok((REGISTRY_PREFIX.len() + offset) as u32);
        }
    }
    Err(hresult_from_win32(ERROR_INSUFFICIENT_BUFFER))
}

/// Returns an owned HSTRING naming the desktop package registry key.
///
/// # Safety
/// A non-null package must address a NUL-terminated UTF-16 string. A non-null
/// output must be writable and must receive one release with WindowsDeleteString
/// after a successful call. The desktop path is independent of the user argument.
#[unsafe(export_name = "GetRegistryPathForPackage")]
pub unsafe extern "system" fn get_registry_path_for_package(
    package: PCWSTR,
    _user: PCWSTR,
    out: *mut *mut c_void,
) -> HRESULT {
    let mut path = [0; PATH_CAPACITY];
    // SAFETY: The export forwards its documented caller contract.
    match unsafe { registry_path(package, &mut path) } {
        Ok(length) => unsafe { WindowsCreateString(path.as_ptr(), length, out) },
        Err(error) => error,
    }
}

/// Opens the package's configuration key with the requested registry access.
///
/// # Safety
/// A non-null package must address a NUL-terminated UTF-16 string. A non-null
/// output must be writable. Close a successfully returned key with RegCloseKey.
#[unsafe(export_name = "OpenGameConfigKeyForPackage")]
pub unsafe extern "system" fn open_game_config_key_for_package(
    package: PCWSTR,
    access: u32,
    user: PCWSTR,
    out: *mut HKEY,
) -> HRESULT {
    let mut path = [0; PATH_CAPACITY];
    // SAFETY: The export forwards its documented caller contract.
    if let Err(error) = unsafe { registry_path(package, &mut path) } {
        return error;
    }
    let hive = if user.is_null() {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_USERS
    };
    // SAFETY: The path is terminated and valid through RegOpenKeyExW. The host
    // checks the access mask and transfers the resulting key handle to the caller.
    hresult_from_win32(unsafe { RegOpenKeyExW(hive, path.as_ptr(), 0, access, out) })
}
