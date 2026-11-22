//! Scarlett Texture2D layout and array addressing for single-sample color textures.
use windows_sys::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL};

const MAX_MIPS: usize = 15;
const LINEAR: u32 = 256;
const TILE_4K: u32 = 261;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Desc {
    pub dimension: u32,
    pub alignment: u64,
    pub width: u64,
    pub height: u32,
    pub slices: u16,
    pub mips: u16,
    pub format: u32,
    pub samples: u32,
    pub quality: u32,
    pub layout: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Mip {
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
pub struct Plane {
    color_plane: u32,
    padding: u32,
    pub size: u64,
    slice_bytes: u64,
    offset: u64,
    pub alignment: u64,
    element_bytes: u32,
    unpacked_mips: u32,
    tail_mips: u32,
    reserved: u32,
    mips: [Mip; MAX_MIPS],
}

#[repr(C)]
pub struct Layout {
    pub bytes: u64,
    pub alignment: u64,
    pub mips: u32,
    pub plane_count: u32,
    pub planes: [Plane; 4],
    pub dimension: u32,
    reserved: u32,
}

const _: () = {
    assert!(size_of::<Desc>() == 56 && core::mem::offset_of!(Desc, layout) == 44);
    assert!(size_of::<Mip>() == 0x70 && size_of::<Plane>() == 0x6c8);
    assert!(core::mem::offset_of!(Plane, mips) == 0x38);
    assert!(size_of::<Layout>() == 0x1b40 && core::mem::offset_of!(Layout, planes) == 0x18);
};

impl Layout {
    /// Append the bounded color metadata plane used by the 64 KiB profile.
    pub fn append_color_metadata(&mut self, kind: u8) {
        let size = if kind == 2 {
            let (tw, th) = self.planes[0].metadata_tile_shape();
            u64::from(self.planes[0].mips[0].width_texels.div_ceil(tw))
                * u64::from(self.planes[0].mips[0].height_texels.div_ceil(th))
                * 8192
        } else {
            8192
        } * u64::from(self.planes[0].array_size());
        let alignment = if kind == 2 { 65536 } else { 8192 };
        let mut metadata = Plane {
            color_plane: if kind == 2 { 9 } else { 2 },
            size,
            offset: self.bytes,
            alignment,
            element_bytes: if kind == 2 { 1 } else { 0 },
            ..Plane::default()
        };
        metadata.mips[0].size = size;
        metadata.mips[0].alignment = alignment as u32;
        self.planes[1] = metadata;
        self.plane_count = 2;
        self.bytes += size;
    }
}

impl Plane {
    /// Bounded square 64 KiB mip chains, including their shared mip tail.
    pub fn compute_64k_mips(desc: &Desc) -> Result<Self, i32> {
        if desc.dimension != 3
            || desc.layout != 0
            || desc.alignment != 0
            || desc.samples != 1
            || desc.quality != 0
            || desc.slices != 1
            || !(2..=12).contains(&desc.mips)
            || !(32..=2048).contains(&desc.width)
            || !(32..=2048).contains(&desc.height)
            || !matches!(
                desc.flags,
                1 | 5 | 0x100001 | 0x100005 | 0x200001 | 0x200005 | 0x300001 | 0x300005
            )
            || !matches!(desc.format, 1..=4 | 9..=18 | 23..=39 | 41..=43 | 48..=54 | 56..=65 | 87 | 88 | 90..=93)
        {
            return Err(E_NOTIMPL);
        }
        let mut base = *desc;
        base.layout = LINEAR;
        base.flags = 0;
        let mut plane = Self::compute(&base)?;
        let (tw, th) = match plane.element_bytes {
            1 => (256u32, 256u32),
            2 => (256, 128),
            4 => (128, 128),
            8 => (128, 64),
            16 => (64, 64),
            _ => return Err(E_NOTIMPL),
        };
        let levels = usize::from(desc.mips);
        let first_tail = (0..levels)
            .find(|i| {
                (desc.width as u32).div_ceil(1 << i) <= tw / 2 && desc.height.div_ceil(1 << i) <= th
            })
            .unwrap_or(levels);
        let max_tail = match plane.element_bytes {
            16 => 7,
            8 | 4 => 7,
            _ => 8,
        };
        if levels - first_tail > max_tail {
            return Err(E_NOTIMPL);
        }
        plane.unpacked_mips = first_tail as u32;
        plane.tail_mips = (levels - first_tail) as u32;
        plane.alignment = 65536;
        plane.size = if first_tail < levels { 65536 } else { 0 };
        for (i, mip) in plane.mips[..levels].iter_mut().enumerate() {
            let tail = i >= first_tail;
            let idx = i.saturating_sub(first_tail);
            let pitch = if tail {
                (tw >> (idx + 1)).max(tw / 16)
            } else {
                (desc.width as u32).div_ceil(1 << i).next_multiple_of(tw)
            };
            let rows = if tail {
                (th >> idx).max(th / 16)
            } else {
                desc.height.div_ceil(1 << i).next_multiple_of(th)
            };
            mip.size = u64::from(pitch) * u64::from(rows) * u64::from(plane.element_bytes);
            mip.slice_bytes = mip.size;
            mip.pitch_texels = pitch;
            mip.pitch_bytes = pitch * plane.element_bytes;
            mip.pitch_elements = pitch;
            mip.padded_rows = rows;
            mip.alignment = if tail { 256 } else { 65536 };
            mip.swizzle = if tail { 3 } else { 27 };
            mip.packed_tail = u32::from(tail);
            mip.reserved[1] = if tail { 0 } else { 0x8000 };
            mip.reserved[3] = if tail { 0 } else { 0x8000 };
            if !tail {
                plane.size += mip.size;
            }
        }
        plane.slice_bytes = plane.size;
        let mut end = plane.size;
        for (i, mip) in plane.mips[..levels].iter_mut().enumerate() {
            if i < first_tail {
                end -= mip.size;
                mip.offset = end;
            } else {
                let idx = i - first_tail;
                mip.offset = if idx < 5 {
                    32768 >> idx
                } else {
                    1536 - ((idx - 5) as u64) * 256
                };
            }
        }
        Ok(plane)
    }

    /// Bounded single-mip render-target layout with 64 KiB swizzle 27.
    pub fn compute_64k_color(desc: &Desc) -> Result<Self, i32> {
        let four_byte = matches!(desc.format, 1..=4 | 9..=18 | 23..=39 | 41..=43 | 48..=54 | 56..=65 | 87 | 88 | 90..=93)
            && matches!(
                desc.flags,
                1 | 5 | 0x100001 | 0x100005 | 0x200001 | 0x200005 | 0x300001 | 0x300005
            );
        let color_family = matches!(desc.format, 1..=4 | 9..=18 | 23..=39 | 41..=43 | 48..=54 | 56..=65 | 87 | 88 | 90..=93)
            && matches!(desc.flags, 0xc00001 | 0xc00005);
        // Type-2 metadata addressing is currently validated only through 512.
        let limit = if color_family || (four_byte && !matches!(desc.flags, 1 | 5)) {
            1024
        } else {
            512
        };
        if desc.dimension != 3
            || !(four_byte || color_family)
            || desc.layout != 0
            || desc.alignment != 0
            || desc.samples != 1
            || desc.quality != 0
            || desc.mips != 1
            || !(1..=8).contains(&desc.slices)
            || desc.width == 0
            || desc.width > limit
            || desc.height == 0
            || desc.height > limit as u32
        {
            return Err(E_NOTIMPL);
        }
        let mut base = *desc;
        base.layout = TILE_4K;
        base.flags = 0;
        let mut plane = Self::compute(&base)?;
        let (tw, th) = match plane.element_bytes {
            1 => (256, 256),
            2 => (256, 128),
            4 => (128, 128),
            8 => (128, 64),
            16 => (64, 64),
            _ => return Err(E_NOTIMPL),
        };
        let pitch = (desc.width as u32).next_multiple_of(tw);
        let rows = desc.height.next_multiple_of(th);
        let bytes = u64::from(pitch) * u64::from(rows) * u64::from(plane.element_bytes);
        plane.size = bytes * u64::from(desc.slices);
        plane.slice_bytes = bytes;
        plane.alignment = 65536;
        let mip = &mut plane.mips[0];
        mip.size = bytes;
        mip.slice_bytes = bytes;
        mip.pitch_texels = pitch;
        mip.pitch_bytes = pitch * plane.element_bytes;
        mip.pitch_elements = pitch;
        mip.padded_rows = rows;
        mip.alignment = 65536;
        mip.swizzle = 27;
        mip.reserved[1] = 0x8000;
        mip.reserved[3] = 0x8000;
        Ok(plane)
    }

    /// Compute a single-plane, linear 3D texture layout. `desc.slices` is the
    /// original volume depth. Mip storage is padded to that original depth,
    /// while each mip's visible depth shrinks with the level.
    pub fn compute_linear_volume(desc: &Desc) -> Result<Self, i32> {
        if desc.layout != LINEAR || desc.slices > 2048 {
            return Err(E_NOTIMPL);
        }
        let depth = u32::from(desc.slices);
        let mut plane = Self::compute(desc)?;
        let mip_count = plane.mip_count() as usize;
        for (level, mip) in plane.mips[..mip_count].iter_mut().enumerate() {
            mip.size = mip.size.checked_mul(u64::from(depth)).ok_or(E_INVALIDARG)?;
            mip.depth = (depth >> level).max(1);
        }
        Ok(plane)
    }

    /// Compute a bounded uncompressed 4 KiB-swizzled 3D plane.
    pub fn compute_4k_volume(desc: &Desc) -> Result<Self, i32> {
        if desc.layout != TILE_4K
            || (70..=84).contains(&desc.format)
            || (94..=99).contains(&desc.format)
        {
            return Err(E_NOTIMPL);
        }
        let mut linear = *desc;
        linear.layout = LINEAR;
        let mut plane = Self::compute(&linear)?;
        let (tw, th, td, _, _, _) = volume_tile(plane.element_bytes).ok_or(E_NOTIMPL)?;
        let levels = usize::from(desc.mips);
        // Packed tails have only been verified for these bounded shapes.
        let tail = if levels > 1
            && (8..=128).contains(&desc.width)
            && (desc.width as u32).is_power_of_two()
            && desc.height <= 4
        {
            (0..levels).find(|level| ((desc.width as u32) >> level).max(1) <= tw)
        } else {
            None
        };
        if tail.is_some_and(|first| levels - first > 2) {
            return Err(E_NOTIMPL);
        }
        if let Some(first) = tail {
            plane.unpacked_mips = first as u32;
            plane.tail_mips = (levels - first) as u32;
        }
        let mut chain_bytes = 0u64;
        for (level, mip) in plane.mips[..levels].iter_mut().enumerate() {
            let scale = 1u32 << level;
            let w = (desc.width as u32).div_ceil(scale);
            let h = desc.height.div_ceil(scale);
            let d = u32::from(desc.slices).div_ceil(scale);
            // Reject unverified packed-tail shapes.
            let in_tail = tail.is_some_and(|first| level >= first);
            if levels > 1 && w <= tw && !in_tail {
                return Err(E_NOTIMPL);
            }
            let tail_index = level.saturating_sub(tail.unwrap_or(level));
            let pitch = if in_tail {
                tw >> tail_index
            } else {
                w.next_multiple_of(tw)
            };
            let rows = if in_tail {
                (th / 2) >> tail_index
            } else {
                h.next_multiple_of(th)
            };
            let depth = d.next_multiple_of(if in_tail { td / 2 } else { td });
            let slice_bytes = if in_tail {
                4096 / u64::from(td)
            } else {
                u64::from(pitch) * u64::from(rows) * u64::from(plane.element_bytes)
            };
            mip.size = slice_bytes * u64::from(depth);
            mip.slice_bytes = slice_bytes;
            mip.pitch_texels = pitch;
            mip.pitch_bytes = pitch * plane.element_bytes;
            mip.alignment = if in_tail { 256 } else { 4096 };
            mip.pitch_elements = pitch;
            mip.padded_rows = rows;
            mip.padded_depth = depth;
            mip.depth = (u32::from(desc.slices) >> level).max(1);
            mip.swizzle = if in_tail { 1 } else { 5 };
            mip.samples = td;
            mip.packed_tail = u32::from(in_tail);
            if in_tail {
                mip.padding[0] = if tail_index == 0 {
                    (th / 2) << 11
                } else {
                    ((th / 4) << 11) | (tw / 2)
                };
            }
            if !in_tail {
                chain_bytes += slice_bytes * u64::from(td);
            }
        }
        if tail.is_some() {
            chain_bytes += 4096;
        }
        let mut end = chain_bytes;
        for mip in &mut plane.mips[..levels] {
            if mip.packed_tail == 0 {
                end -= mip.slice_bytes * u64::from(td);
                mip.offset = end;
            } else {
                mip.offset = 0;
            }
        }
        plane.slice_bytes = chain_bytes;
        plane.size = chain_bytes * u64::from(u32::from(desc.slices).div_ceil(td));
        plane.alignment = 4096;
        Ok(plane)
    }

    pub fn compute(desc: &Desc) -> Result<Self, i32> {
        let (block, element_bytes, tile_width, tile_height): (u32, u32, u32, u32) =
            match desc.format {
                1..=4 => (1, 16, 16, 16),            // RGBA32.
                9..=18 => (1, 8, 32, 16),            // RGBA16 and RG32.
                48..=54 | 56..=59 => (1, 2, 64, 32), // RG8 and color R16.
                60..=65 => (1, 1, 64, 64),           // R8 and A8.
                // Packed RGB, RGBA8, RG16, R32 (color), and BGR(A/X)8.
                23..=39 | 41..=43 | 87 | 88 | 90..=93 => (1, 4, 32, 32),
                // BC1 and BC4 each store a 4x4 block in eight bytes.
                70..=72 | 79..=81 => (4, 8, 32, 16),
                // BC2/3/5/6H/7 each store a 4x4 block in 16 bytes.
                73..=78 | 82..=84 | 94..=99 => (4, 16, 16, 16),
                _ => return Err(E_NOTIMPL),
            };
        if desc.dimension != 3
            || desc.samples != 1
            || desc.quality != 0
            || desc.flags != 0
            || desc.alignment != 0
            || !matches!(desc.layout, LINEAR | TILE_4K)
        {
            return Err(E_NOTIMPL);
        }
        if !(1..=16384).contains(&desc.width)
            || !(1..=16384).contains(&desc.height)
            || !(1..=2048).contains(&desc.slices)
        {
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
            if !linear && levels > 1 && w <= tile_width / 2 && h <= tile_height {
                plane.unpacked_mips = plane.unpacked_mips.min(level);
            }
            let in_tail = level >= plane.unpacked_mips;
            let (pitch, rows, alignment, swizzle) = if linear {
                (w.next_multiple_of(256 / element_bytes), h, 256, 0)
            } else if in_tail {
                // Small mips share one 4 KiB tile, down to a 256-byte microtile.
                let index = level - plane.unpacked_mips;
                (
                    (tile_width >> (index + 1)).max(tile_width / 4),
                    (tile_height >> index).max(tile_height / 4),
                    256,
                    1,
                )
            } else {
                (
                    w.next_multiple_of(tile_width),
                    h.next_multiple_of(tile_height),
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
                padded_depth: u32::from(desc.slices),
                width_elements: width_texels.div_ceil(block),
                height_elements: height_texels.div_ceil(block),
                width_texels,
                height_texels,
                depth: u32::from(desc.slices),
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
        plane.size *= u64::from(desc.slices);
        Ok(plane)
    }

    pub fn mip_count(&self) -> u32 {
        self.unpacked_mips + self.tail_mips
    }

    pub fn array_size(&self) -> u32 {
        self.mips[0].depth
    }

    pub fn element_bytes(&self) -> u32 {
        self.element_bytes
    }

    pub fn metadata_tile_shape(&self) -> (u32, u32) {
        match self.element_bytes {
            16 => (512, 256),
            8 => (512, 512),
            4 => (1024, 512),
            _ => (2048, 1024),
        }
    }

    pub fn metadata_tile_stride(&self) -> u32 {
        self.mips[0]
            .width_texels
            .div_ceil(self.metadata_tile_shape().0)
    }

    pub fn metadata_tile_rows(&self) -> u32 {
        self.mips[0]
            .height_texels
            .div_ceil(self.metadata_tile_shape().1)
    }

    /// Range within the Xbox allocation occupied by this mip, including padding.
    pub fn mip_range_in_slice(&self, slice: u32, level: u32) -> Option<core::ops::Range<u64>> {
        if slice >= self.array_size() || level >= self.mip_count() {
            return None;
        }
        let mip = &self.mips[level as usize];
        let base = u64::from(slice) * self.slice_bytes;
        Some(base + mip.offset..base + mip.offset + mip.size)
    }

    pub fn element_offset(&self, level: u32, x: u64, y: u32) -> Option<u64> {
        self.element_offset_in_slice(0, level, x, y)
    }

    /// Element address in a supported linear or 4 KiB-swizzled 3D plane.
    pub fn element_offset_in_volume(&self, z: u32, level: u32, x: u64, y: u32) -> Option<u64> {
        let mip = self.mips.get(level as usize)?;
        if z >= mip.depth || x >= u64::from(mip.width_elements) || y >= mip.height_elements {
            return None;
        }
        if mip.swizzle == 0 {
            return self.element_offset_in_slice(z, level, x, y);
        }
        let (tw, th, td, xb, yb, zb) = volume_tile(self.element_bytes)?;
        let x = u32::try_from(x).ok()?;
        let tile = (y / th) * (mip.pitch_elements / tw) + x / tw;
        Some(
            mip.offset
                + u64::from(z / td) * self.slice_bytes
                + u64::from(tile) * 4096
                + if mip.packed_tail != 0 {
                    if level == self.unpacked_mips {
                        2048
                    } else {
                        768
                    }
                } else {
                    0
                }
                + u64::from(scatter(x, xb) + scatter(y, yb) + scatter(z, zb)),
        )
    }

    pub fn element_offset_in_slice(&self, slice: u32, level: u32, x: u64, y: u32) -> Option<u64> {
        if slice >= self.array_size() {
            return None;
        }
        let mip = self.mips.get(level as usize)?;
        let origin = u64::from(slice) * self.slice_bytes + mip.offset;
        if x >= u64::from(mip.width_elements) || y >= mip.height_elements {
            return None;
        }
        let x = x as u32;
        if mip.swizzle == 0 {
            return Some(origin + u64::from(y * mip.pitch_bytes + x * self.element_bytes));
        }
        if mip.swizzle == 27 || mip.swizzle == 3 {
            let (x_basis, y_basis, tw, th): (&[u64], &[u64], u32, u32) = match self.element_bytes {
                16 => (
                    &[
                        0x10, 0x40, 0x2000, 0x1000, 0x200, 0x800, 0x8400, 0x100, 0x1000, 0,
                    ],
                    &[
                        0x20, 0x80, 0x4000, 0x8000, 0x300, 0x400, 0x800, 0x100, 0x1000, 0,
                    ],
                    64,
                    64,
                ),
                8 => (
                    &[
                        8, 0x20, 0x40, 0x4000, 0x200, 0x800, 0x1400, 0x8100, 0x1000, 0,
                    ],
                    &[
                        0x10, 0x80, 0x2000, 0x8000, 0x300, 0x400, 0x800, 0x100, 0x1000, 0,
                    ],
                    128,
                    64,
                ),
                4 => (
                    &[4, 8, 0x40, 0x2000, 0x200, 0x800, 0x1400, 0x8100, 0x1000, 0],
                    &[
                        0x10, 0x20, 0x80, 0x4000, 0x300, 0x400, 0x8800, 0x100, 0x1000, 0,
                    ],
                    128,
                    128,
                ),
                2 => (
                    &[2, 4, 8, 0x80, 0x200, 0x800, 0x1400, 0x8100, 0x1000, 0],
                    &[
                        0x10, 0x20, 0x40, 0x2000, 0x300, 0x400, 0x4800, 0x8100, 0x1000, 0,
                    ],
                    256,
                    128,
                ),
                1 => (
                    &[1, 2, 4, 8, 0x200, 0x800, 0x1400, 0x4100, 0x9000, 0],
                    &[
                        0x10, 0x20, 0x40, 0x80, 0x300, 0x400, 0x2800, 0x8100, 0x1000, 0,
                    ],
                    256,
                    256,
                ),
                _ => return None,
            };
            let mut inner = 0u64;
            for i in 0..x_basis.len() {
                if x & (1 << i) != 0 {
                    inner ^= x_basis[i];
                }
                if y & (1 << i) != 0 {
                    inner ^= y_basis[i];
                }
            }
            if slice & 1 != 0 {
                inner ^= 0x800;
            }
            if slice & 2 != 0 {
                inner ^= 0x400;
            }
            if slice & 4 != 0 {
                inner ^= 0x200;
            }
            if mip.swizzle == 3 {
                let origins: &[u64] = match self.element_bytes {
                    16 => &[0x800, 0x400, 0x200, 0x300, 0x1000, 0xa000, 0xc000],
                    8 => &[0x1400, 0x400, 0x800, 0x300, 0x200, 0xc000, 0xa000],
                    4 => &[0x1400, 0x8800, 0x800, 0x400, 0x200, 0x2300, 0x4300],
                    2 => &[0x8100, 0x4800, 0x1400, 0x400, 0x800, 0x100, 0x2300, 0x300],
                    1 => &[0x4100, 0x8100, 0x1400, 0x2800, 0x800, 0x600, 0x700, 0x400],
                    _ => return None,
                };
                return Some(origins.get((level - self.unpacked_mips) as usize)? ^ inner);
            }
            let tile = (y / th) * (mip.pitch_elements / tw) + x / tw;
            return Some(origin + u64::from(tile) * 65536 + inner);
        }
        // These are byte-address bit positions for the X and Y element coordinates
        // within one 4 KiB tile. Compressed formats address 4x4 blocks.
        let (tile_width, tile_height, x_bits, y_bits): (u32, u32, &[u8], &[u8]) =
            match self.element_bytes {
                1 => (64, 64, &[0, 1, 2, 3, 9, 11], &[4, 5, 6, 7, 8, 10]),
                2 => (64, 32, &[1, 2, 3, 7, 9, 11], &[4, 5, 6, 8, 10]),
                4 => (32, 32, &[2, 3, 7, 9, 11], &[4, 5, 6, 8, 10]),
                8 => (32, 16, &[3, 6, 7, 9, 11], &[4, 5, 8, 10]),
                16 => (16, 16, &[6, 7, 9, 11], &[4, 5, 8, 10]),
                _ => return None,
            };
        let tile = (y / tile_height) * (mip.pitch_elements / tile_width) + x / tile_width;
        Some(origin + u64::from(tile * 4096 + scatter(x, x_bits) + scatter(y, y_bits)))
    }
}

type VolumeTile = (u32, u32, u32, &'static [u8], &'static [u8], &'static [u8]);

fn volume_tile(bytes: u32) -> Option<VolumeTile> {
    Some(match bytes {
        1 => (16, 16, 16, &[0, 1, 6, 9], &[3, 5, 8, 11], &[2, 4, 7, 10]),
        2 => (8, 16, 16, &[1, 6, 9], &[3, 5, 8, 11], &[2, 4, 7, 10]),
        4 => (8, 16, 8, &[2, 6, 9], &[3, 5, 8, 11], &[4, 7, 10]),
        8 => (8, 8, 8, &[3, 6, 9], &[5, 8, 11], &[4, 7, 10]),
        16 => (4, 8, 8, &[6, 9], &[5, 8, 11], &[4, 7, 10]),
        _ => return None,
    })
}

fn scatter(value: u32, positions: &[u8]) -> u32 {
    positions
        .iter()
        .enumerate()
        .fold(0, |bits, (i, bit)| bits | (((value >> i) & 1) << bit))
}
