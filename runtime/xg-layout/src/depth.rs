//! The single-sample 64 KiB Z-order surface used by Scarlett depth/stencil textures.
use super::{Desc, Layout, Mip, Plane};
use windows_sys::Win32::Foundation::E_NOTIMPL;

const TILE_BYTES: u64 = 65536;

#[derive(Clone, Copy)]
pub struct DepthStencil {
    width: u32,
    height: u32,
    htile: bool,
    depth_bytes: u32,
    stencil: bool,
}

impl DepthStencil {
    pub fn compute(desc: &Desc) -> Result<Self, i32> {
        if desc.dimension != 3
            || desc.layout != 0
            || desc.alignment != 0
            || desc.samples != 1
            || desc.quality != 0
            || desc.slices != 1
            || desc.mips != 1
            || !(1..=16384).contains(&desc.width)
            || !(1..=16384).contains(&desc.height)
            || !matches!(desc.flags, 0x0100_0006 | 0x0100_000a | 0x8130_0002)
        {
            return Err(E_NOTIMPL);
        }
        let (depth_bytes, stencil) = match desc.format {
            19 | 20 | 44 | 45 => (4, true),
            39 | 40 => (4, false),
            53 | 55 => (2, false),
            _ => return Err(E_NOTIMPL),
        };
        Ok(Self {
            width: desc.width as u32,
            height: desc.height,
            htile: desc.flags != 0x8130_0002,
            depth_bytes,
            stencil,
        })
    }

    fn geometry(self, stencil: bool) -> (u32, u32, u32, u64) {
        let bytes = if stencil { 1 } else { self.depth_bytes };
        let tile_width = if bytes == 4 { 128 } else { 256 };
        let tile_height = if bytes == 1 { 256 } else { 128 };
        let pitch = self.width.next_multiple_of(tile_width);
        let rows = self.height.next_multiple_of(tile_height);
        (pitch, rows, bytes, u64::from(pitch * rows * bytes))
    }

    pub fn htile_bytes(self) -> u64 {
        if self.htile {
            u64::from(self.width.div_ceil(1024) * self.height.div_ceil(1024)) * TILE_BYTES
        } else {
            0
        }
    }

    pub fn depth_index(self) -> u32 {
        u32::from(self.htile)
    }

    pub fn base(self, index: u32) -> Option<u64> {
        let depth = self.depth_index();
        if index == 0 {
            Some(0)
        } else if index == depth {
            Some(self.htile_bytes())
        } else if self.stencil && index == depth + 1 {
            Some(self.htile_bytes() + self.geometry(false).3)
        } else {
            None
        }
    }

    pub fn size(self) -> u64 {
        self.htile_bytes()
            + self.geometry(false).3
            + if self.stencil {
                self.geometry(true).3
            } else {
                0
            }
    }

    pub fn data_plane(self, stencil: bool) -> Plane {
        let (pitch, rows, bytes, size) = self.geometry(stencil);
        let mut result = Plane {
            color_plane: if stencil { 8 } else { 7 },
            size,
            slice_bytes: size,
            offset: self
                .base(self.depth_index() + u32::from(stencil))
                .unwrap_or(0),
            alignment: TILE_BYTES,
            element_bytes: bytes,
            unpacked_mips: 1,
            ..Plane::default()
        };
        result.mips[0] = Mip {
            size,
            slice_bytes: size,
            pitch_texels: pitch,
            pitch_bytes: pitch * bytes,
            alignment: TILE_BYTES as u32,
            pitch_elements: pitch,
            padded_rows: rows,
            padded_depth: 1,
            width_elements: self.width,
            height_elements: self.height,
            width_texels: self.width,
            height_texels: self.height,
            depth: 1,
            slices: 1,
            swizzle: 24,
            reserved: [0, 0x8000, 0, 0x8000, 0],
            samples: 1,
            ..Mip::default()
        };
        result
    }

    pub fn write_planes(self, layout: &mut Layout) {
        let depth = self.depth_index() as usize;
        if self.htile {
            let mut htile = Plane {
                color_plane: 4,
                size: self.htile_bytes(),
                alignment: TILE_BYTES,
                ..Plane::default()
            };
            htile.mips[0].size = htile.size;
            htile.mips[0].alignment = TILE_BYTES as u32;
            layout.planes[0] = htile;
        }
        layout.planes[depth] = self.data_plane(false);
        if self.stencil {
            layout.planes[depth + 1] = self.data_plane(true);
        }
        layout.plane_count = depth as u32 + 1 + u32::from(self.stencil);
        layout.bytes = self.size();
    }

    /// XG returns a byte offset relative to the selected plane. The tiled
    /// data planes XOR their coordinate bits within a 64 KiB macrotile.
    pub fn offset(self, index: u32, x: u64, y: u32) -> Option<u64> {
        if x >= u64::from(self.width) || y >= self.height {
            return None;
        }
        let x = x as u32;
        let depth = self.depth_index();
        let (xb, yb, tw, th): (&[u32], &[u32], u32, u32) = if self.htile && index == 0 {
            (
                &[0, 0, 0, 4, 0x200, 0x800, 0x1400, 0x120, 0x80, 0x4000],
                &[0, 0, 0, 8, 0x300, 0x400, 0x1810, 0x140, 0x2000, 0x8000],
                1024,
                1024,
            )
        } else if index == depth && self.depth_bytes == 2 {
            (
                &[2, 8, 0x20, 0x80, 0x200, 0x800, 0x1400, 0x8100],
                &[4, 0x10, 0x40, 0x2000, 0x300, 0x400, 0x5800, 0x8100],
                256,
                128,
            )
        } else if index == depth {
            (
                &[4, 0x10, 0x40, 0x2000, 0x200, 0x800, 0x1400, 0x8100],
                &[8, 0x20, 0x80, 0x4000, 0x300, 0x400, 0x9800, 0x100],
                128,
                128,
            )
        } else if self.stencil && index == depth + 1 {
            (
                &[1, 4, 0x10, 0x40, 0x200, 0x800, 0x1400, 0x4100, 0x8000],
                &[2, 8, 0x20, 0x80, 0x300, 0x400, 0x3800, 0x8100, 0],
                256,
                256,
            )
        } else {
            return None;
        };
        let mut inner = 0;
        for (bit, (&a, &b)) in xb.iter().zip(yb).enumerate() {
            if x & (1 << bit) != 0 {
                inner ^= a;
            }
            if y & (1 << bit) != 0 {
                inner ^= b;
            }
        }
        Some(u64::from((y / th) * self.width.div_ceil(tw) + x / tw) * TILE_BYTES + u64::from(inner))
    }
}
