//! Lookup of PDB records by suffix for the Xbox PIX live-merge interface.
use std::ffi::c_void;

// Reviewed Xbox/MSVC ABI: each record is 88 bytes. Its first member is the
// 32-byte std::wstring representation (small strings use seven inline WCHARs).
const RECORD_BYTES: usize = 88;
const WSTRING_CAPACITY_OFFSET: usize = 24;
const INLINE_CAPACITY: usize = 7;

#[repr(C)]
struct RecordVector {
    begin: *const u8,
    end: *const u8,
    capacity: *const u8,
}

unsafe fn wide_len(mut text: *const u16) -> usize {
    let mut length = 0;
    // SAFETY: The API requires a valid, null-terminated UTF-16 input.
    while unsafe { text.read_unaligned() } != 0 {
        length += 1;
        text = unsafe { text.add(1) };
    }
    length
}

fn fold_ascii(unit: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&unit) { unit + 32 } else { unit }
}

/// Find the first PDB record whose name matches the end of `path`.
///
/// The reviewed DLL compares ASCII letters without case, while other UTF-16
/// code units compare exactly. An empty record name matches every non-null path.
///
/// # Safety
/// `records` must point to a valid MSVC vector of 88-byte PDB records; each
/// stored string and any non-null `path` must be valid null-terminated UTF-16.
#[unsafe(export_name = "FindPdbInfo")]
pub unsafe extern "system" fn find_pdb_info(path: *const u16, records: *const c_void) -> i32 {
    if path.is_null() || records.is_null() { return -1; }
    // SAFETY: Caller supplies the vector header under the documented ABI.
    let vector = unsafe { records.cast::<RecordVector>().read_unaligned() };
    if vector.begin.is_null() && vector.end.is_null() { return -1; }
    let Some(bytes) = (vector.end as usize).checked_sub(vector.begin as usize) else { return -1; };
    if bytes == 0 || vector.begin.is_null() || bytes % RECORD_BYTES != 0 || bytes / RECORD_BYTES > i32::MAX as usize {
        return -1;
    }
    // SAFETY: The path contract guarantees termination and readability.
    let path_len = unsafe { wide_len(path) };
    for index in 0..bytes / RECORD_BYTES {
        // SAFETY: The caller supplies a valid vector and complete records.
        let entry = unsafe { vector.begin.add(index * RECORD_BYTES) };
        let capacity = unsafe { entry.add(WSTRING_CAPACITY_OFFSET).cast::<usize>().read_unaligned() };
        let candidate = if capacity <= INLINE_CAPACITY {
            entry.cast::<u16>()
        } else {
            unsafe { entry.cast::<*const u16>().read_unaligned() }
        };
        if candidate.is_null() { continue; }
        // SAFETY: Each vector entry supplies a valid null-terminated string.
        let candidate_len = unsafe { wide_len(candidate) };
        if candidate_len > path_len { continue; }
        let suffix = unsafe { path.add(path_len - candidate_len) };
        let mut equal = true;
        for i in 0..candidate_len {
            let a = unsafe { suffix.add(i).read_unaligned() };
            let b = unsafe { candidate.add(i).read_unaligned() };
            if fold_ascii(a) != fold_ascii(b) { equal = false; break; }
        }
        if equal { return index as i32; }
    }
    -1
}
