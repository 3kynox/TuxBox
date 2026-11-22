//! Workgroup-wide compaction of hair visibility samples.
//!
//! Both programs restrict the reservation to local lane zero before the
//! election. Its active-lane count is one, its prefix is zero, and it consumes
//! its own returning atomic value. This remains true when the 64-thread group
//! spans multiple host waves. The three waits publish the initialization,
//! per-pixel additions and reservation, respectively, before their consumers.
use super::{compute_group, packed_root::matches_program, Emitter, Enc, Insn, Opnd, Res, ShaderExtract, Stage};

pub(super) fn prepare(ex: &ShaderExtract, insns: &mut [Insn]) -> Option<bool> {
    if ex.stage != Stage::Compute || compute_group(ex) != [8, 8, 1] || ex.entry.as_deref() != Some("MainCS") { return None; }
    let indirect = if matches_program(&ex.isa, 1836, 0x9f0e1f2c8563a83b) { true }
                   else if matches_program(&ex.isa, 1720, 0x63424472876e231b) { false }
                   else { return None; };
    for ins in insns {
        let sync = if indirect { [0x054, 0x4b8, 0x578] } else { [0x0ac, 0x460, 0x51c] };
        let zero = if indirect { [0x4e4, 0x4f4, 0x53c, 0x54c] } else { [0x488, 0x498, 0x4e0, 0x4f0] };
        let first = if indirect { 0x4ec } else { 0x490 };
        let broadcast = if indirect { 0x548 } else { 0x4ec };
        if sync.contains(&ins.offset) { ins.mnemonic = "tb_subsurface_group_sync".into(); }
        else if zero.contains(&ins.offset) { ins.enc = Enc::Vop1; ins.op = 1; ins.mnemonic = "v_mov_b32".into(); ins.src = vec![Opnd::ConstInt(0)]; }
        else if ins.offset == first { ins.enc = Enc::Sop1; ins.op = 3; ins.mnemonic = "s_mov_b32".into(); ins.src = vec![Opnd::ConstInt(0)]; }
        else if ins.offset == broadcast { ins.enc = Enc::Sop1; ins.op = 3; ins.mnemonic = "s_mov_b32".into(); }
    }
    Some(indirect)
}

pub(super) fn resources(em: &mut Emitter<'_>, indirect: bool) -> Result<(), String> {
    let bindings: &[(usize, Res)] = if indirect { &[(0x058, Res::TBuf(6, false)), (0x0e0, Res::TBuf(7, false)), (0x69c, Res::TBuf(3, true)), (0x718, Res::Uav(1, 1))] }
        else { &[(0x63c, Res::TBuf(3, true)), (0x6a4, Res::Uav(1, 1))] };
    for &(offset, expected) in bindings {
        let Some((idx, _)) = em.insns.iter().enumerate().find(|(_, i)| i.offset == offset) else { return Err("missing hair compaction binding".into()); };
        if em.res.get(&idx).and_then(|r| r.first()).copied() != Some(expected) { return Err(format!("unexpected hair compaction binding at {offset:#x}")); }
    }
    // Count and tile coordinates, output pixel coordinates, and packed pixel
    // index/counts are integer views. Their bit patterns are not floats.
    if indirect { em.tbuf_ty.extend([((false, 6), 1), ((false, 7), 1)]); }
    em.tbuf_ty.insert((true, 3), 1);
    em.img_ty.insert((true, 1), 1);
    Ok(())
}
