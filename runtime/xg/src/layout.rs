//! Scarlett Texture2D layout for one slice and one sample.
use windows_sys::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL};

pub(crate) const MAX_MIPS: usize = 15;
const LINEAR: u32 = 256;
const TILE_4K: u32 = 261;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Desc {
    dimension: u32,
    alignment: u64,
    width: u64,
    height: u32,
    slices: u16,
    mips: u16,
    format: u32,
    samples: u32,
    quality: u32,
    layout: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Mip {
    size: u64,
    offset: u64,
    slice_bytes: u64,
    pitch_texels: u32,
    pitch_bytes: u32,
    alignment: u32,
    pitch_elements: u32,
    padded_rows: u32,
    padded_depth: u32,
    width_elements: u32,
    height_elements: u32,
    width_texels: u32,
    height_texels: u32,
    depth: u32,
    slices: u32,
    swizzle: u32,
    reserved: [u32; 5],
    samples: u32,
    packed_tail: u32,
    padding: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Plane {
    color_plane: u32,
    padding: u32,
    pub(crate) size: u64,
    slice_bytes: u64,
    offset: u64,
    pub(crate) alignment: u64,
    element_bytes: u32,
    unpacked_mips: u32,
    tail_mips: u32,
    reserved: u32,
    mips: [Mip; MAX_MIPS],
}

#[repr(C)]
pub(crate) struct Layout {
    pub(crate) bytes: u64,
    pub(crate) alignment: u64,
    pub(crate) mips: u32,
    pub(crate) plane_count: u32,
    pub(crate) planes: [Plane; 4],
    pub(crate) dimension: u32,
    reserved: u32,
}

const _: () = {
    assert!(size_of::<Desc>() == 56 && core::mem::offset_of!(Desc, layout) == 44);
    assert!(size_of::<Mip>() == 0x70 && size_of::<Plane>() == 0x6c8);
    assert!(core::mem::offset_of!(Plane, mips) == 0x38);
    assert!(size_of::<Layout>() == 0x1b40 && core::mem::offset_of!(Layout, planes) == 0x18);
};

impl Plane {
    pub(crate) fn compute(desc: &Desc) -> Result<Self, i32> {
        let (block, element_bytes, tile_side): (u32, u32, u32) = match desc.format {
            28 | 87 | 90 => (1, 4, 32),  // RGBA8 UNORM, BGRA8 UNORM/TYPELESS
            76 | 83 | 98 => (4, 16, 16), // BC3 TYPELESS, BC5 UNORM, BC7 UNORM
            _ => return Err(E_NOTIMPL),
        };
        if desc.dimension != 3
            || desc.slices != 1
            || desc.samples != 1
            || desc.quality != 0
            || desc.flags != 0
            || desc.alignment != 0
            || !matches!(desc.layout, LINEAR | TILE_4K)
        {
            return Err(E_NOTIMPL);
        }
        if !(1..=16384).contains(&desc.width) || !(1..=16384).contains(&desc.height) {
            return Err(E_INVALIDARG);
        }
        let width = desc.width as u32;
        let levels = u32::from(desc.mips);
        if levels == 0 || levels > 1 + width.max(desc.height).ilog2() {
            return Err(E_INVALIDARG);
        }
        let linear = desc.layout == LINEAR;
        let mut plane = Self {
            color_plane: 1,
            element_bytes,
            unpacked_mips: levels,
            alignment: if linear { 256 } else { 4096 },
            ..Self::default()
        };
        // Padded levels round the original extent up. Visible mip dimensions round down.
        for level in 0..levels {
            let scale = 1 << level;
            let w = width.div_ceil(block * scale);
            let h = desc.height.div_ceil(block * scale);
            if !linear && levels > 1 && w <= tile_side / 2 && h <= tile_side {
                plane.unpacked_mips = plane.unpacked_mips.min(level);
            }
            let in_tail = level >= plane.unpacked_mips;
            let (pitch, rows, alignment, swizzle) = if linear {
                (w.next_multiple_of(256 / element_bytes), h, 256, 0)
            } else if in_tail {
                // Small mips share one 4 KiB tile, down to a 256-byte microtile.
                let index = level - plane.unpacked_mips;
                let micro = tile_side / 4;
                (
                    (tile_side >> (index + 1)).max(micro),
                    (tile_side >> index).max(micro),
                    256,
                    1,
                )
            } else {
                (
                    w.next_multiple_of(tile_side),
                    h.next_multiple_of(tile_side),
                    4096,
                    5,
                )
            };
            let size = u64::from(pitch * element_bytes) * u64::from(rows);
            let width_texels = (width >> level).max(1);
            let height_texels = (desc.height >> level).max(1);
            plane.mips[level as usize] = Mip {
                size,
                slice_bytes: size,
                pitch_texels: pitch * block,
                pitch_bytes: pitch * element_bytes,
                alignment,
                pitch_elements: pitch,
                padded_rows: rows,
                padded_depth: 1,
                width_elements: width_texels.div_ceil(block),
                height_elements: height_texels.div_ceil(block),
                width_texels,
                height_texels,
                depth: 1,
                slices: 1,
                swizzle,
                samples: 1,
                packed_tail: u32::from(in_tail),
                ..Mip::default()
            };
            if !in_tail {
                plane.size += size;
            }
        }
        plane.tail_mips = levels - plane.unpacked_mips;
        if plane.tail_mips > 0 {
            plane.size += 4096;
        }
        plane.slice_bytes = plane.size;
        // Smaller mips occupy the lower addresses in both layouts.
        let mut end = plane.size;
        for mip in &mut plane.mips[..levels as usize] {
            end -= mip.size;
            mip.offset = end;
        }
        Ok(plane)
    }

    pub(crate) fn mip_count(&self) -> u32 {
        self.unpacked_mips + self.tail_mips
    }

    pub(crate) fn element_offset(&self, level: u32, x: u64, y: u32) -> Option<u64> {
        let mip = self.mips.get(level as usize)?;
        if x >= u64::from(mip.width_elements) || y >= mip.height_elements {
            return None;
        }
        let x = x as u32;
        if mip.swizzle == 0 {
            return Some(mip.offset + u64::from(y * mip.pitch_bytes + x * self.element_bytes));
        }
        // These are byte-address bit positions for the X and Y element coordinates
        // within one 4 KiB tile. Compressed formats address 4x4 blocks.
        let (side, x_bits, y_bits): (u32, &[u8], &[u8]) = if self.element_bytes == 4 {
            (32, &[2, 3, 7, 9, 11], &[4, 5, 6, 8, 10])
        } else {
            (16, &[6, 7, 9, 11], &[4, 5, 8, 10])
        };
        let tile = (y / side) * (mip.pitch_elements / side) + x / side;
        Some(mip.offset + u64::from(tile * 4096 + scatter(x, x_bits) + scatter(y, y_bits)))
    }
}

fn scatter(value: u32, positions: &[u8]) -> u32 {
    positions
        .iter()
        .enumerate()
        .fold(0, |bits, (i, bit)| bits | (((value >> i) & 1) << bit))
}
