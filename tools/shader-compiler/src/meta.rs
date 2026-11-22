//! Input registers and binding metadata consumed by the gameplay HLSL emitter.

/// The PS configuration in FSSM 1.1 carries SPI_PS_INPUT_ENA at 0x248.
/// RDNA2 packs the front-face and ancillary words after the barycentrics,
/// line stipple and position components, skipping disabled inputs.
#[derive(Clone, Copy)]
pub(crate) enum PixelSystemInput {
    FrontFace = 12,
    Ancillary = 13,
}

pub(crate) fn pixel_system_vgpr(fssm: &[u8], input: PixelSystemInput) -> Option<usize> {
    let enabled = ps_input_enable(fssm)?;
    let bit = input as usize;
    (enabled & (1 << bit) != 0).then(|| packed_pixel_vgpr(enabled, bit))
}

fn ps_input_enable(fssm: &[u8]) -> Option<u32> {
    if fssm.get(..8)? != b"FSSM\x01\x01\0\0" {
        return None;
    }
    let enabled = u32::from_le_bytes(*fssm.get(0x248..)?.first_chunk()?);
    (enabled >> 16 == 0).then_some(enabled)
}

fn packed_pixel_vgpr(enabled: u32, bit: usize) -> usize {
    const WIDTHS: [usize; 13] = [2, 2, 2, 3, 2, 2, 2, 1, 1, 1, 1, 1, 1];
    WIDTHS[..bit].iter().enumerate()
        .filter(|(i, _)| enabled & (1 << i) != 0)
        .map(|(_, width)| width)
        .sum()
}

/// Position components follow the enabled barycentric/stipple registers.
/// A disabled component occupies no VGPR, including when W follows X/Y or Z.
pub(crate) fn position_vgprs(fssm: &[u8], read: u8) -> Option<[Option<usize>; 4]> {
    let enabled = ps_input_enable(fssm)?;
    if read == 0 || (enabled >> 8) & 15 != u32::from(read) {
        return None;
    }
    Some(std::array::from_fn(|i| {
        (read & (1 << i) != 0).then(|| packed_pixel_vgpr(enabled, 8 + i))
    }))
}

#[cfg(test)]
mod pixel_inputs {
    use super::{pixel_system_vgpr, position_vgprs, PixelSystemInput};

    #[test]
    fn sparse_fragment_position_follows_enabled_barycentrics() {
        let mut fssm = [0; 0x24c];
        fssm[..8].copy_from_slice(b"FSSM\x01\x01\0\0");
        for (mask, read, slots) in [
            (0x402u32, 4, Some([None, None, Some(2), None])),
            (0x702, 7, Some([Some(2), Some(3), Some(4), None])),
            (0x528, 5, Some([Some(5), None, Some(6), None])),
            (0x402, 3, None),
            (0xb02, 11, Some([Some(2), Some(3), None, Some(4)])),
            (0xc02, 12, Some([None, None, Some(2), Some(3)])),
            (0x1f02, 15, Some([Some(2), Some(3), Some(4), Some(5)])),
            (0x20, 0, None),
        ] {
            fssm[0x248..].copy_from_slice(&mask.to_le_bytes());
            assert_eq!(position_vgprs(&fssm, read), slots);
        }
    }

    #[test]
    fn ancillary_location_follows_enabled_inputs() {
        let mut fssm = [0; 0x24c];
        fssm[..8].copy_from_slice(b"FSSM\x01\x01\0\0");
        for (mask, slot) in [(0x2020u32, Some(2)), (0x2302, Some(4)),
                             (0x3fff, Some(21)), (0x0302, None)] {
            fssm[0x248..].copy_from_slice(&mask.to_le_bytes());
            assert_eq!(pixel_system_vgpr(&fssm, PixelSystemInput::Ancillary), slot);
        }
        assert_eq!(pixel_system_vgpr(&fssm[..24], PixelSystemInput::Ancillary), None);
    }

    #[test]
    fn front_face_follows_only_enabled_position_components() {
        let mut fssm = [0; 0x24c];
        fssm[..8].copy_from_slice(b"FSSM\x01\x01\0\0");
        for (mask, slot) in [(0x1302u32, Some(4)), (0x1702, Some(5)),
                             (0x1f02, Some(6)), (0x3000, Some(0)), (0x0f02, None)] {
            fssm[0x248..].copy_from_slice(&mask.to_le_bytes());
            assert_eq!(pixel_system_vgpr(&fssm, PixelSystemInput::FrontFace), slot);
        }
    }
}

pub use tuxbox_dxbc::{has_packing_mismatch, parse_signature, pixel_input_order, SigElement};

pub use crate::psv::{parse_compute_threads, parse_mesh_info, parse_psv0, parse_psv0_gs, GsInfo, MeshInfo, PsvResource};

pub use crate::rootsig::{parse_rts0, RootParamKind, RootParam, RootRange, RootSig};
