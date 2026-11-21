//! Subsurface indirect setup, Frosty global entry 856. One 8x8 group produces
//! one flag word. Both append elections are explicitly intersected with VCC,
//! which this kernel sets once to (local_y * 8 + local_x == 0).
use super::{compute_group, packed_root::matches_program, Enc, Insn, Opnd, ShaderExtract, Stage};

pub(super) fn prepare(ex: &ShaderExtract, insns: &mut [Insn]) -> bool {
    if ex.stage != Stage::Compute || compute_group(ex) != [8, 8, 1]
        || ex.entry.as_deref() != Some("SetupIndirectCS")
        || !matches_program(&ex.isa, 1104, 0x3bc35cb76f547697)
    {
        return false;
    }
    for ins in insns {
        match ins.offset {
            // The native workgroup occupies one wave64. A wave32 host needs
            // workgroup-wide ordering between clear, atomic OR and load.
            0x080 | 0x2b8 => ins.mnemonic = "tb_subsurface_group_sync".into(),
            // The only participant is local lane 0. Its physical lane index
            // and its prefix among the active lanes are both zero. The frame
            // of code that reads/increments the UAV counter remains intact.
            0x2e8 | 0x2f8 | 0x334 | 0x344 | 0x3b0 | 0x3c0 | 0x3fc | 0x40c => {
                ins.enc = Enc::Vop1;
                ins.op = 1;
                ins.mnemonic = "v_mov_b32".into();
                ins.src = vec![Opnd::ConstInt(0)];
            }
            0x2f0 | 0x3b8 => {
                ins.enc = Enc::Sop1;
                ins.op = 3;
                ins.mnemonic = "s_mov_b32".into();
                ins.src = vec![Opnd::ConstInt(0)];
            }
            // Only the issuing lane consumes its own pre-increment value.
            0x340 | 0x408 => {
                ins.enc = Enc::Sop1;
                ins.op = 3;
                ins.mnemonic = "s_mov_b32".into();
            }
            _ => {}
        }
    }
    true
}
