//! The x64 texture address computer used by XGCreateTextureComputer.

use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};
use std::alloc::{Layout as Allocation, alloc};
use tuxbox_xg_layout::{Desc, Layout, Plane};
use windows_sys::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER, S_OK};

#[repr(C)]
pub struct Computer {
    methods: &'static Methods,
    refs: AtomicU32,
    plane: Plane,
    dimension: u32,
}

/// Buffer layout descriptor consumed by the Xbox XG library.
#[repr(C)]
pub struct BufferDesc {
    size: u32,
    reserved_04: u32,
    flags: u32,
    reserved_0c: u32,
    options: u32,
}

/// Compute a buffer's linear allocation layout.
///
/// # Safety
/// Valid non-null pointers must refer to a readable descriptor and a writable,
/// disjoint `Layout`, respectively.
#[unsafe(export_name = "XGComputeBufferLayout")]
unsafe extern "system" fn compute_buffer_layout(desc: *const BufferDesc, out: *mut Layout) -> i32 {
    if desc.is_null() || out.is_null() {
        return E_INVALIDARG;
    }
    let desc = unsafe { &*desc };
    if desc.size == 0
        || desc.flags & 0xffff_f900 != 0
        || desc.options & 0xf385_6d0a != 0
        || (desc.options & 0x0400_0000 != 0 && desc.size & 0xffff != 0)
    {
        return E_INVALIDARG;
    }
    unsafe {
        out.write_bytes(0, 1);
        (*out).bytes = u64::from(desc.size);
        (*out).alignment = if desc.options & 0x0c00_0000 != 0 {
            65536
        } else {
            4
        };
        (*out).mips = 1;
        (*out).dimension = 1;
    }
    S_OK
}

type Unsupported = unsafe extern "system" fn() -> !;

#[repr(C)]
struct Methods {
    add_ref: unsafe extern "system" fn(*mut Computer) -> u32,
    release: unsafe extern "system" fn(*mut Computer) -> u32,
    layout: unsafe extern "system" fn(&Computer, *mut Layout) -> i32,
    unused: [Unsupported; 3],
    offset: unsafe extern "system" fn(&Computer, u32, u32, u64, u32, u32, u32, *mut u32) -> u64,
    other: [Unsupported; 9],
}

unsafe extern "system" fn unsupported() -> ! {
    std::process::abort()
}

static METHODS: Methods = Methods {
    add_ref,
    release,
    layout: get_layout,
    unused: [unsupported; 3],
    offset: element_offset,
    other: [unsupported; 9],
};

unsafe extern "system" fn add_ref(this: *mut Computer) -> u32 {
    unsafe { (*this).refs.fetch_add(1, Ordering::Relaxed) + 1 }
}

unsafe extern "system" fn release(this: *mut Computer) -> u32 {
    let left = unsafe { (*this).refs.fetch_sub(1, Ordering::AcqRel) } - 1;
    if left == 0 {
        // The final owner reclaims the global allocation made by the factory.
        drop(unsafe { Box::from_raw(this) });
    }
    left
}

unsafe extern "system" fn get_layout(this: &Computer, out: *mut Layout) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    // The guest supplies one complete layout. Inactive planes and reserved
    // fields have a defined zero representation, irrespective of prior contents.
    unsafe { write_texture_layout(out, this.plane, this.dimension) };
    S_OK
}

/// # Safety
/// `out` points to a writable `Layout` disjoint from `plane`.
unsafe fn write_texture_layout(out: *mut Layout, plane: Plane, dimension: u32) {
    unsafe {
        out.write_bytes(0, 1);
        (*out).bytes = plane.size;
        (*out).alignment = plane.alignment;
        (*out).mips = plane.mip_count();
        (*out).plane_count = 1;
        (*out).planes[0] = plane;
        (*out).dimension = dimension;
    }
}

/// Xbox-specific 2D texture descriptor, distinct from a D3D12 resource desc.
#[repr(C)]
pub struct Texture2DDesc {
    width: u32,
    height: u32,
    mips: u32,
    slices: u32,
    format: u32,
    samples: u32,
    quality: u32,
    usage: u32,
    bind_flags: u32,
    cpu_access: u32,
    misc_flags: u32,
    swizzle: u32,
    pitch: u32,
}

fn texture_2d_plane(desc: &Texture2DDesc) -> Result<Plane, i32> {
    // Restrict support to the two usage/swizzle combinations compared against
    // the supplied Xbox library. Other combinations are not equivalent to
    // blindly substituting a D3D12 resource descriptor.
    if !matches!((desc.usage, desc.swizzle), (3, 0) | (0 | 1, 5))
        || desc.bind_flags != 8
        || desc.cpu_access != 0
        || desc.misc_flags != 0
        || desc.pitch != 0
        || desc.samples != 1
        || desc.quality != 0
        || desc.mips == 0
    {
        return Err(E_NOTIMPL);
    }
    let mips = u16::try_from(desc.mips).map_err(|_| E_INVALIDARG)?;
    let slices = u16::try_from(desc.slices).map_err(|_| E_INVALIDARG)?;
    Plane::compute(&Desc {
        dimension: 3,
        alignment: 0,
        width: u64::from(desc.width),
        height: desc.height,
        slices,
        mips,
        format: desc.format,
        samples: 1,
        quality: 0,
        layout: 256 + desc.swizzle,
        flags: 0,
    })
}

/// # Safety
/// Non-null pointers must reference a readable descriptor and a disjoint,
/// writable layout. Unsupported descriptor combinations return an error.
#[unsafe(export_name = "XGComputeTexture2DLayout")]
unsafe extern "system" fn compute_texture_2d(desc: *const Texture2DDesc, out: *mut Layout) -> i32 {
    if desc.is_null() || out.is_null() {
        return E_INVALIDARG;
    }
    let plane = match texture_2d_plane(unsafe { &*desc }) {
        Ok(plane) => plane,
        Err(hr) => return hr,
    };
    unsafe { write_texture_layout(out, plane, 3) };
    S_OK
}

/// # Safety
/// Non-null pointers must reference a readable descriptor and a disjoint,
/// writable output pointer. Release a successful object through slot 1.
#[unsafe(export_name = "XGCreateTexture2DComputer")]
unsafe extern "system" fn create_texture_2d(
    desc: *const Texture2DDesc,
    out: *mut *mut Computer,
) -> i32 {
    if desc.is_null() || out.is_null() {
        return E_INVALIDARG;
    }
    unsafe { out.write(ptr::null_mut()) };
    let plane = match texture_2d_plane(unsafe { &*desc }) {
        Ok(plane) => plane,
        Err(hr) => return hr,
    };
    unsafe { allocate_computer(plane, 3, out) }
}

/// Xbox-specific 1D texture descriptor. It differs from the D3D12 descriptor.
#[repr(C)]
pub struct Texture1DDesc {
    width: u32,
    mips: u32,
    slices: u32,
    format: u32,
    usage: u32,
    bind_flags: u32,
    cpu_access: u32,
    misc_flags: u32,
    swizzle: u32,
    pitch: u32,
}

fn texture_1d_plane(desc: &Texture1DDesc) -> Result<Plane, i32> {
    // Native controls establish this linear, shader-resource subset only.
    // Xbox 1D textures reject block-compressed formats; do not reuse the 2D
    // implementation for those or for unmeasured usage and swizzle modes.
    if !matches!(desc.usage, 0 | 1 | 3)
        || desc.bind_flags != 8
        || desc.cpu_access != 0
        || desc.misc_flags != 0
        || desc.swizzle != 0
        || desc.pitch != 0
        || desc.mips == 0
        || (70..=84).contains(&desc.format)
        || (94..=99).contains(&desc.format)
    {
        return Err(E_NOTIMPL);
    }
    let mips = u16::try_from(desc.mips).map_err(|_| E_INVALIDARG)?;
    let slices = u16::try_from(desc.slices).map_err(|_| E_INVALIDARG)?;
    Plane::compute(&Desc {
        dimension: 3,
        alignment: 0,
        width: u64::from(desc.width),
        height: 1,
        slices,
        mips,
        format: desc.format,
        samples: 1,
        quality: 0,
        layout: 256,
        flags: 0,
    })
}

/// # Safety
/// Non-null pointers must reference a readable descriptor and a disjoint,
/// writable layout. Unsupported descriptor combinations return an error.
#[unsafe(export_name = "XGComputeTexture1DLayout")]
unsafe extern "system" fn compute_texture_1d(desc: *const Texture1DDesc, out: *mut Layout) -> i32 {
    if desc.is_null() || out.is_null() {
        return E_INVALIDARG;
    }
    let plane = match texture_1d_plane(unsafe { &*desc }) {
        Ok(plane) => plane,
        Err(hr) => return hr,
    };
    unsafe { write_texture_layout(out, plane, 2) };
    S_OK
}

/// # Safety
/// Non-null pointers must reference a readable descriptor and a disjoint,
/// writable output pointer. Release a successful object through slot 1.
#[unsafe(export_name = "XGCreateTexture1DComputer")]
unsafe extern "system" fn create_texture_1d(
    desc: *const Texture1DDesc,
    out: *mut *mut Computer,
) -> i32 {
    if desc.is_null() || out.is_null() {
        return E_INVALIDARG;
    }
    unsafe { out.write(ptr::null_mut()) };
    let plane = match texture_1d_plane(unsafe { &*desc }) {
        Ok(plane) => plane,
        Err(hr) => return hr,
    };
    unsafe { allocate_computer(plane, 2, out) }
}

/// Xbox-specific 3D texture descriptor.
#[repr(C)]
pub struct Texture3DDesc {
    width: u32,
    height: u32,
    depth: u32,
    mips: u32,
    format: u32,
    usage: u32,
    bind_flags: u32,
    cpu_access: u32,
    misc_flags: u32,
    swizzle: u32,
    pitch: u32,
}

fn texture_3d_plane(desc: &Texture3DDesc) -> Result<Plane, i32> {
    // Only the native-verified linear, single-plane color subset is supported.
    if !matches!(desc.usage, 0 | 1 | 3)
        || desc.bind_flags != 8
        || desc.cpu_access != 0
        || desc.misc_flags != 0
        || desc.swizzle != 0
        || desc.pitch != 0
        || desc.mips == 0
        || (70..=84).contains(&desc.format)
        || (94..=99).contains(&desc.format)
    {
        return Err(E_NOTIMPL);
    }
    if desc.width > 2048 || desc.height > 2048 || desc.depth > 2048 {
        return Err(E_NOTIMPL);
    }
    // The existing 2D plane calculator determines levels from width/height;
    // depth-only mip chains need their own validated implementation.
    if desc.width != 0 && desc.height != 0 && desc.mips > 1 + desc.width.max(desc.height).ilog2() {
        return Err(E_NOTIMPL);
    }
    let mips = u16::try_from(desc.mips).map_err(|_| E_INVALIDARG)?;
    let depth = u16::try_from(desc.depth).map_err(|_| E_INVALIDARG)?;
    Plane::compute_linear_volume(&Desc {
        dimension: 3,
        alignment: 0,
        width: u64::from(desc.width),
        height: desc.height,
        slices: depth,
        mips,
        format: desc.format,
        samples: 1,
        quality: 0,
        layout: 256,
        flags: 0,
    })
}

/// # Safety
/// Non-null pointers must reference a readable descriptor and a disjoint,
/// writable layout. Unsupported descriptor combinations return an error.
#[unsafe(export_name = "XGComputeTexture3DLayout")]
unsafe extern "system" fn compute_texture_3d(desc: *const Texture3DDesc, out: *mut Layout) -> i32 {
    if desc.is_null() || out.is_null() {
        return E_INVALIDARG;
    }
    let plane = match texture_3d_plane(unsafe { &*desc }) {
        Ok(plane) => plane,
        Err(hr) => return hr,
    };
    unsafe { write_texture_layout(out, plane, 4) };
    S_OK
}

/// # Safety
/// Non-null pointers must reference a readable descriptor and a disjoint,
/// writable output pointer. Release a successful object through slot 1.
#[unsafe(export_name = "XGCreateTexture3DComputer")]
unsafe extern "system" fn create_texture_3d(
    desc: *const Texture3DDesc,
    out: *mut *mut Computer,
) -> i32 {
    if desc.is_null() || out.is_null() {
        return E_INVALIDARG;
    }
    unsafe { out.write(ptr::null_mut()) };
    let plane = match texture_3d_plane(unsafe { &*desc }) {
        Ok(plane) => plane,
        Err(hr) => return hr,
    };
    unsafe { allocate_computer(plane, 4, out) }
}

/// # Safety
/// `out` must be a valid writable pointer, disjoint from the new object.
unsafe fn allocate_computer(plane: Plane, dimension: u32, out: *mut *mut Computer) -> i32 {
    let object = unsafe { alloc(Allocation::new::<Computer>()).cast::<Computer>() };
    if object.is_null() {
        return E_OUTOFMEMORY;
    }
    unsafe {
        object.write(Computer {
            methods: &METHODS,
            refs: AtomicU32::new(1),
            plane,
            dimension,
        });
        out.write(object);
    }
    S_OK
}

unsafe extern "system" fn element_offset(
    this: &Computer,
    plane: u32,
    mip: u32,
    x: u64,
    y: u32,
    slice: u32,
    sample: u32,
    bit: *mut u32,
) -> u64 {
    if !bit.is_null() {
        // All supported formats address whole bytes.
        unsafe { bit.write(0) };
    }
    if plane != 0 || sample != 0 {
        return u64::MAX;
    }
    let offset = if this.dimension == 4 {
        this.plane.element_offset_in_volume(slice, mip, x, y)
    } else {
        this.plane.element_offset_in_slice(slice, mip, x, y)
    };
    offset.unwrap_or(u64::MAX)
}

/// Returns an owned texture computer for a supported single-plane descriptor.
///
/// # Safety
/// Non-null inputs must describe a readable x64 resource descriptor and a
/// disjoint writable output pointer. Release the returned object through slot 1.
#[unsafe(export_name = "XGCreateTextureComputer")]
unsafe extern "system" fn create(desc: *const Desc, out: *mut *mut Computer) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe { out.write(ptr::null_mut()) };
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return E_INVALIDARG;
    };
    let plane = match Plane::compute(desc) {
        Ok(p) => p,
        Err(hr) => return hr,
    };
    unsafe { allocate_computer(plane, 3, out) }
}
