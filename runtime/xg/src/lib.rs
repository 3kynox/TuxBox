//! The x64 texture address computer used by XGCreateTextureComputer.

use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};
use std::alloc::{Layout as Allocation, alloc};
use tuxbox_xg_layout::{Desc, Layout, Plane};
use windows_sys::Win32::Foundation::{
    E_FAIL, E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER, S_OK,
};

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

#[repr(C)]
pub struct TileShape {
    width: u32,
    height: u32,
    depth: u32,
}

/// Select the Xbox swizzle mode for a resource description.
///
/// The supplied XG build reads an eighth flags argument and does not inspect
/// the dimensions or sample count for this selection.
#[unsafe(export_name = "XGComputeOptimalSwizzleMode")]
extern "system" fn optimal_swizzle_mode(
    dimension: u32,
    format: u32,
    _width: u32,
    _height: u32,
    _depth_or_array: u32,
    _samples: u32,
    bind_flags: u32,
    resource_flags: u32,
) -> u32 {
    if dimension == 1 {
        return 0;
    }
    if bind_flags & 0x40 != 0 {
        let incompatible = matches!(format, 19 | 20 | 22 | 44 | 45 | 47 | 62 | 118 | 119 | 120);
        return if resource_flags & 0x0800_8000 != 0 && incompatible {
            u32::MAX
        } else {
            24
        };
    }
    match dimension {
        3 | 4 if resource_flags & 0x0800_0000 != 0 => 10,
        3 | 4 if bind_flags & 0x20 != 0 => 27,
        3 | 4 => 5,
        _ => 0,
    }
}

/// Validate a depth/stencil swizzle mode. This Xbox DLL's supported modes are
/// independent of the supplied format and sample-count parameters.
#[unsafe(export_name = "XGValidateDepthStencilSwizzleMode")]
extern "system" fn validate_depth_stencil_swizzle(
    _format: u32,
    _samples: u32,
    swizzle: u32,
) -> i32 {
    if matches!(swizzle, 4 | 8 | 12 | 16 | 20 | 24 | 28) {
        S_OK
    } else {
        E_FAIL
    }
}

/// Format storage in bytes per element and pixels per element, for the
/// color formats already supported by TuxBox's texture-layout provider.
fn tile_format(format: u32) -> Option<(u32, u32)> {
    Some(match format {
        1..=4 => (16, 1),
        9..=18 => (8, 1),
        23..=39 | 41..=43 | 87 | 88 | 90..=93 => (4, 1),
        48..=54 | 56..=59 => (2, 1),
        60..=65 => (1, 1),
        70..=72 | 79..=81 => (8, 4),
        73..=78 | 82..=84 | 94..=99 => (16, 4),
        _ => return None,
    })
}

/// Compute the 64 KiB tile dimensions for supported buffers and 2D formats.
///
/// # Safety
/// Each non-null output must point to a writable `TileShape`.
#[unsafe(export_name = "XGComputeTileShape")]
unsafe extern "system" fn compute_tile_shape(
    dimension: u32,
    format: u32,
    bytes_per_element: u32,
    samples: u32,
    pixels: *mut TileShape,
    elements: *mut TileShape,
) -> i32 {
    if dimension != 3 && samples != 1 {
        return E_INVALIDARG;
    }
    let (element_bytes, block) = match (dimension, format) {
        (1, 0) if bytes_per_element <= 1 => (1, 0),
        (1, _) => return E_INVALIDARG,
        (2 | 4, _) => return E_NOTIMPL,
        (3, 0) => (bytes_per_element, 0),
        (3, _) => match tile_format(format) {
            Some(format) => format,
            None => return E_NOTIMPL,
        },
        _ => return E_NOTIMPL,
    };
    let (mut width, mut height) = if dimension == 1 {
        (65536, 1)
    } else {
        match element_bytes {
            1 => (256, 256),
            2 => (256, 128),
            4 => (128, 128),
            8 => (128, 64),
            16 => (64, 64),
            _ => return E_INVALIDARG,
        }
    };
    if dimension == 3 {
        match samples {
            1 => (),
            2 => height /= 2,
            4 => {
                width /= 2;
                height /= 2;
            }
            8 => {
                width /= 2;
                height /= 4;
            }
            _ => return E_INVALIDARG,
        }
    }
    // Native writes the element shape first; this also preserves the result
    // when a caller deliberately aliases the two output pointers.
    unsafe {
        if !elements.is_null() {
            elements.write(TileShape {
                width,
                height,
                depth: 1,
            });
        }
        if !pixels.is_null() {
            pixels.write(if block == 0 {
                TileShape {
                    width: 0,
                    height: 0,
                    depth: 0,
                }
            } else {
                TileShape {
                    width: width * block,
                    height: height * block,
                    depth: 1,
                }
            });
        }
    }
    S_OK
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
    // Native-verified linear color volumes and single-mip 4 KiB volumes.
    if !matches!((desc.usage, desc.swizzle), (0 | 1 | 3, 0) | (0 | 1, 5))
        || desc.bind_flags != 8
        || desc.cpu_access != 0
        || desc.misc_flags != 0
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
    let base = Desc {
        dimension: 3,
        alignment: 0,
        width: u64::from(desc.width),
        height: desc.height,
        slices: depth,
        mips,
        format: desc.format,
        samples: 1,
        quality: 0,
        layout: 256 + desc.swizzle,
        flags: 0,
    };
    if desc.swizzle == 0 {
        Plane::compute_linear_volume(&base)
    } else {
        Plane::compute_4k_volume(&base)
    }
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
    let (plane, dimension) = match desc.dimension {
        4 => {
            if desc.alignment != 0 || desc.flags != 0 || desc.samples != 1 || desc.quality != 0 {
                return E_NOTIMPL;
            }
            let width = match u32::try_from(desc.width) {
                Ok(width) => width,
                Err(_) => return E_NOTIMPL,
            };
            let swizzle = match desc.layout {
                256 => 0,
                261 => 5,
                _ => return E_NOTIMPL,
            };
            let typed = Texture3DDesc {
                width,
                height: desc.height,
                depth: u32::from(desc.slices),
                mips: u32::from(desc.mips),
                format: desc.format,
                usage: if swizzle == 0 { 3 } else { 0 },
                bind_flags: 8,
                cpu_access: 0,
                misc_flags: 0,
                swizzle,
                pitch: 0,
            };
            match texture_3d_plane(&typed) {
                Ok(plane) => (plane, 4),
                Err(hr) => return hr,
            }
        }
        _ => match Plane::compute(desc) {
            Ok(plane) => (plane, 3),
            Err(hr) => return hr,
        },
    };
    unsafe { allocate_computer(plane, dimension, out) }
}
