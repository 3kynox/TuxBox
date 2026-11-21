//! Global 3333. The final native instructions elect the first active lane,
//! count VCC across the original wave, and issue one non-returning atomic add.
use super::{compute_group, packed_root::matches_program, Insn, ShaderExtract, Stage};

pub(super) fn prepare(ex: &ShaderExtract, insns: &mut Vec<Insn>) -> bool {
    if ex.stage != Stage::Compute || compute_group(ex) != [64, 1, 1]
        || ex.entry.as_deref() != Some("GenerateParticleSortKeys")
        || !matches_program(&ex.isa, 2320, 0x00f43d576c9ad43b)
    {
        return false;
    }
    // Preserve the keys/IDs and the s46 -> EXEC restore at 0x8b8.
    insns.retain(|ins| ins.offset <= 0x8b8 || ins.offset == 0x90c);
    true
}

pub(super) const DECLARATIONS: &str = r#"
groupshared uint sort_first, sort_count;
RWBuffer<uint> sort_counter : register(u2);
"#;

pub(super) const EPILOGUE: &str = r#"
    if (gtid.x == 0u) {
        sort_first = 64u;
        sort_count = 0u;
    }
    GroupMemoryBarrierWithGroupSync();
    if ((exec_lo & 1u) != 0u) {
        InterlockedMin(sort_first, gtid.x);
        if (v2 == 0u) InterlockedAdd(sort_count, 1u);
    }
    GroupMemoryBarrierWithGroupSync();
    if (gtid.x == sort_first) InterlockedAdd(sort_counter[cb0[3].y], sort_count);
"#;
