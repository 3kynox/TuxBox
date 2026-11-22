//! Reflection compaction uses a wave32 exclusive scan twice. In the first
//! scan the guest enables four lanes, then temporarily restores the full wave
//! before permuting; keep every host lane present through the scan.
use super::{compute_group, packed_root::matches_program, Enc, Insn, Opnd, ShaderExtract, Stage};

pub(super) fn prepare(ex: &ShaderExtract, insns: &mut [Insn]) -> bool {
    if ex.stage != Stage::Compute || compute_group(ex) != [128, 1, 1]
        || ex.entry.as_deref() != Some("ReflectionCompactTracesCS")
        || !matches_program(&ex.isa, 1572, 0x352cb470906c4d15)
    {
        return false;
    }
    for op in insns {
        if op.offset == 0x4c0 {
            // The EXEC-zero branch skips inactive lanes on a per-invocation
            // host. They must remain present for the subsequent wave scan.
            op.mnemonic = "v_nop".into();
            op.enc = Enc::Vop1;
            op.dst = vec![Opnd::Vgpr(0)];
        } else if matches!(op.offset, 0x4e0 | 0x554) {
            op.mnemonic = "tb_reflection_prefix32".into();
            op.src = vec![Opnd::Vgpr(1)];
        } else if (0x4e4..=0x51c).contains(&op.offset) || (0x558..=0x590).contains(&op.offset) {
            op.mnemonic = "v_nop".into();
            op.enc = Enc::Vop1;
        }
    }
    true
}
