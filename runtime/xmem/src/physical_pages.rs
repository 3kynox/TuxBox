//! CPU page tokens for Frosty's 64 KiB allocation mode.
use core::{ffi::c_void, mem::size_of, ptr, slice};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use windows_sys::Win32::Foundation::{
    ERROR_BUSY, ERROR_INVALID_ADDRESS, ERROR_INVALID_PARAMETER, ERROR_NOT_ENOUGH_MEMORY,
    ERROR_NOT_SUPPORTED, GetLastError, SetLastError,
};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_DECOMMIT, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_READWRITE, VirtualAlloc,
    VirtualFree, VirtualQuery,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const PAGE: usize = 65536;
const FLAGS: u64 = 0x2040_0000;

#[derive(Default)]
struct Page {
    mapped: bool,
    saved: Vec<u8>,
}

#[derive(Default)]
struct Pool {
    next: u64,
    pages: HashMap<u64, Page>,
    mappings: HashMap<usize, u64>,
}

static POOL: LazyLock<Mutex<Pool>> = LazyLock::new(|| Mutex::new(Pool::default()));

fn failure(code: u32) -> i32 {
    // SAFETY: this sets the calling thread's last-error code.
    unsafe { SetLastError(code) };
    0
}

impl Pool {
    fn allocate(&mut self, count: usize) -> Result<u64, u32> {
        let last = self
            .next
            .checked_add(count as u64)
            .ok_or(ERROR_NOT_ENOUGH_MEMORY)?;
        self.pages
            .try_reserve(count)
            .map_err(|_| ERROR_NOT_ENOUGH_MEMORY)?;
        let first = self.next + 1;
        for token in first..=last {
            self.pages.insert(token, Page::default());
        }
        self.next = last;
        Ok(first)
    }

    fn validate_unmapped(&self, tokens: &[u64]) -> Result<(), u32> {
        let mut seen = HashSet::new();
        seen.try_reserve(tokens.len())
            .map_err(|_| ERROR_NOT_ENOUGH_MEMORY)?;
        for token in tokens {
            let page = self.pages.get(token).ok_or(ERROR_INVALID_PARAMETER)?;
            if page.mapped {
                return Err(ERROR_BUSY);
            }
            if !seen.insert(*token) {
                return Err(ERROR_INVALID_PARAMETER);
            }
        }
        Ok(())
    }

    fn map(&mut self, address: usize, bytes: usize, tokens: &[u64]) -> Result<(), u32> {
        self.validate_unmapped(tokens)?;
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let mut cursor = address;
        let mut reservation = ptr::null_mut::<c_void>();
        while cursor < address + bytes {
            // SAFETY: VirtualQuery writes only the specified local structure.
            if unsafe {
                VirtualQuery(
                    cursor as _,
                    &mut info,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            } == 0
                || info.State != MEM_RESERVE
                || (!reservation.is_null() && info.AllocationBase != reservation)
            {
                return Err(ERROR_INVALID_ADDRESS);
            }
            // Newly reserved and previously decommitted pages can form separate
            // VirtualQuery regions within the same reservation.
            reservation = info.AllocationBase;
            cursor = (info.BaseAddress as usize)
                .checked_add(info.RegionSize)
                .filter(|&end| end > cursor)
                .ok_or(ERROR_INVALID_ADDRESS)?;
        }
        if (0..tokens.len()).any(|i| self.mappings.contains_key(&(address + i * PAGE))) {
            return Err(ERROR_BUSY);
        }
        self.mappings
            .try_reserve(tokens.len())
            .map_err(|_| ERROR_NOT_ENOUGH_MEMORY)?;
        // SAFETY: caller owns the reserved range exclusively during this operation.
        if unsafe { VirtualAlloc(address as _, bytes, MEM_COMMIT, PAGE_READWRITE) }.is_null() {
            return Err(unsafe { GetLastError() });
        }
        for (i, token) in tokens.iter().enumerate() {
            let at = address + i * PAGE;
            let page = self
                .pages
                .get_mut(token)
                .expect("validated under pool lock");
            if !page.saved.is_empty() {
                // SAFETY: saved holds one full page and VirtualAlloc committed its destination.
                unsafe { ptr::copy_nonoverlapping(page.saved.as_ptr(), at as *mut u8, PAGE) };
                page.saved = Vec::new();
            }
            page.mapped = true;
            self.mappings.insert(at, *token);
        }
        Ok(())
    }

    fn unmap(&mut self, address: usize, bytes: usize) -> Result<(), u32> {
        // Take a snapshot of every mapped page before changing the reservation.
        // Allocation or read failure leaves every mapping in place for a retry.
        for at in (address..address + bytes).step_by(PAGE) {
            let token = self.mappings.get(&at).ok_or(ERROR_INVALID_ADDRESS)?;
            let page = self.pages.get_mut(token).expect("mapping owns a live page");
            if page.saved.is_empty() {
                page.saved
                    .try_reserve_exact(PAGE)
                    .map_err(|_| ERROR_NOT_ENOUGH_MEMORY)?;
                page.saved.resize(PAGE, 0);
            }
            // SAFETY: Windows checks the guest source range. The owned buffer holds PAGE bytes.
            if unsafe {
                ReadProcessMemory(
                    GetCurrentProcess(),
                    at as _,
                    page.saved.as_mut_ptr().cast(),
                    PAGE,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(unsafe { GetLastError() });
            }
        }
        // SAFETY: all pages in this aligned range are registered to this provider.
        if unsafe { VirtualFree(address as _, bytes, MEM_DECOMMIT) } == 0 {
            return Err(unsafe { GetLastError() });
        }
        for at in (address..address + bytes).step_by(PAGE) {
            let token = self
                .mappings
                .remove(&at)
                .expect("validated under pool lock");
            self.pages
                .get_mut(&token)
                .expect("mapping owns a live page")
                .mapped = false;
        }
        Ok(())
    }
}

/// # Safety
/// The count points to a writable SIZE_T. The separate token array has space
/// for the requested number of UINT64 entries.
#[unsafe(export_name = "XMemAllocatePhysicalPages")]
pub unsafe extern "system" fn allocate(flags: u64, count: *mut usize, tokens: *mut u64) -> i32 {
    if count.is_null() {
        return failure(ERROR_INVALID_PARAMETER);
    }
    // SAFETY: the caller owns the count for the duration of the call.
    let requested = unsafe { count.read_unaligned() };
    unsafe { count.write_unaligned(0) };
    if flags != FLAGS {
        return failure(ERROR_NOT_SUPPORTED);
    }
    if requested == 0 {
        return 1;
    }
    if tokens.is_null() || requested > isize::MAX as usize / size_of::<u64>() {
        return failure(ERROR_INVALID_PARAMETER);
    }
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    let first = match pool.allocate(requested) {
        Ok(first) => first,
        Err(e) => return failure(e),
    };
    for i in 0..requested {
        // SAFETY: caller provided requested entries.
        unsafe { tokens.add(i).write_unaligned(first + i as u64) };
    }
    unsafe { count.write_unaligned(requested) };
    1
}

/// # Safety
/// A nonempty token array must contain count readable, aligned entries.
#[unsafe(export_name = "XMemFreePhysicalPages")]
pub unsafe extern "system" fn free(count: usize, tokens: *const u64) -> i32 {
    if count == 0 {
        return 1;
    }
    if tokens.is_null() || !tokens.is_aligned() || count > isize::MAX as usize / size_of::<u64>() {
        return failure(ERROR_INVALID_PARAMETER);
    }
    // SAFETY: the caller supplies the initialized array.
    let tokens = unsafe { slice::from_raw_parts(tokens, count) };
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = pool.validate_unmapped(tokens) {
        return failure(e);
    }
    for token in tokens {
        pool.pages.remove(token);
    }
    1
}

/// # Safety
/// The caller owns the reservation, serializes access to the range while this
/// call runs, and unmaps before releasing or independently decommitting it.
/// A supplied token array contains count aligned entries outside that range.
#[unsafe(export_name = "XMemMapPhysicalPages")]
pub unsafe extern "system" fn map(
    address: *mut c_void,
    count: usize,
    tokens: *const u64,
) -> *mut c_void {
    let at = address as usize;
    if at == 0 {
        failure(ERROR_INVALID_PARAMETER);
        return ptr::null_mut();
    }
    if count == 0 {
        return address;
    }
    let Some(bytes) = count
        .checked_mul(PAGE)
        .filter(|&n| at.checked_add(n).is_some())
    else {
        failure(ERROR_INVALID_PARAMETER);
        return ptr::null_mut();
    };
    if !at.is_multiple_of(PAGE) || !tokens.is_aligned() {
        failure(ERROR_INVALID_PARAMETER);
        return ptr::null_mut();
    }
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    let result = if tokens.is_null() {
        pool.unmap(at, bytes)
    } else {
        // SAFETY: PAGE is larger than sizeof(u64), and the checked range fits usize.
        // The caller supplies a separate initialized token array.
        pool.map(at, bytes, unsafe { slice::from_raw_parts(tokens, count) })
    };
    match result {
        Ok(()) => address,
        Err(e) => {
            failure(e);
            ptr::null_mut()
        }
    }
}

/// Map or unmap individually addressed 64 KiB pages owned by this provider.
///
/// # Safety
/// The caller supplies a nonnull array of `count` aligned page addresses and,
/// if nonnull, `count` aligned page tokens. Arrays must remain readable outside
/// the mapped ranges. The caller must own and serialize access to each range.
#[unsafe(export_name = "XMemMapPhysicalPagesScatter")]
pub unsafe extern "system" fn map_scatter(
    addresses: *const *mut c_void,
    count: usize,
    tokens: *const u64,
) -> i32 {
    if addresses.is_null() || !addresses.is_aligned() {
        return failure(ERROR_INVALID_PARAMETER);
    }
    if count == 0 {
        return 1;
    }
    if count > isize::MAX as usize / size_of::<usize>() || !tokens.is_aligned() {
        return failure(ERROR_INVALID_PARAMETER);
    }
    // SAFETY: The caller supplies separate, initialized arrays of count items.
    let input = unsafe { slice::from_raw_parts(addresses, count) };
    let mut pages = Vec::new();
    if pages.try_reserve_exact(count).is_err() {
        return failure(ERROR_NOT_ENOUGH_MEMORY);
    }
    let mut unique = HashSet::new();
    if unique.try_reserve(count).is_err() {
        return failure(ERROR_NOT_ENOUGH_MEMORY);
    }
    for &address in input {
        let at = address as usize;
        if at == 0 || !at.is_multiple_of(PAGE) || at.checked_add(PAGE).is_none() || !unique.insert(at) {
            return failure(ERROR_INVALID_PARAMETER);
        }
        pages.push(at);
    }
    let mut pool = POOL.lock().unwrap_or_else(|e| e.into_inner());
    let mut associated = Vec::new();
    if associated.try_reserve_exact(count).is_err() {
        return failure(ERROR_NOT_ENOUGH_MEMORY);
    }
    if tokens.is_null() {
        for &at in &pages {
            let Some(&token) = pool.mappings.get(&at) else { return failure(ERROR_INVALID_ADDRESS); };
            associated.push(token);
        }
        for (index, &at) in pages.iter().enumerate() {
            if let Err(error) = pool.unmap(at, PAGE) {
                for prior in (0..index).rev() {
                    if pool.map(pages[prior], PAGE, &associated[prior..prior + 1]).is_err() {
                        return failure(ERROR_INVALID_ADDRESS);
                    }
                }
                return failure(error);
            }
        }
    } else {
        // SAFETY: The caller supplies count readable tokens, checked above.
        let values = unsafe { slice::from_raw_parts(tokens, count) };
        if let Err(error) = pool.validate_unmapped(values) { return failure(error); }
        for (index, (&at, &token)) in pages.iter().zip(values).enumerate() {
            if let Err(error) = pool.map(at, PAGE, core::slice::from_ref(&token)) {
                for prior in (0..index).rev() {
                    if pool.unmap(pages[prior], PAGE).is_err() {
                        return failure(ERROR_INVALID_ADDRESS);
                    }
                }
                return failure(error);
            }
        }
    }
    1
}
