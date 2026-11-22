//! One Nanite virtual-shadow instance-cull group is one AMD wave64. All
//! scalar branches control the whole group, including backedges whose SCC
//! comes from a vector compare mask. Reconstruct these votes on wave32 hosts.
use super::{compute_group, packed_root::matches_program, r, Emitter, Insn, Opnd, ShaderExtract, Stage};

pub(super) fn recognize(ex: &ShaderExtract) -> bool {
    if ex.stage == Stage::Compute && compute_group(ex) == [8, 8, 1]
        && ex.entry.as_deref() == Some("ScreenProbeConvertToIrradianceCS")
        && matches_program(&ex.isa, 3624, 0x63d66671fc0a1366)
    {
        return true;
    }
    ex.stage == Stage::Compute && compute_group(ex) == [64, 1, 1]
        && (ex.entry.as_deref() == Some("VirtualSmInvalidateInstancePagesCS")
            && matches_program(&ex.isa, 4604, 0x3a8e759a33ecc608)
        || ex.entry.as_deref() == Some("AllocateCommandInstanceOutputSpaceCs")
            && matches_program(&ex.isa, 468, 0xf082cd3768ec31f0)
        || ex.entry.as_deref() == Some("CompactFeedbackHashTableCS")
            && matches_program(&ex.isa, 348, 0x5976decd660b00a3)
        || ex.entry.as_deref() == Some("CullToVoxelClipmapCS")
            && matches_program(&ex.isa, 404, 0xcc708f80a9096e5d)
        || ex.entry.as_deref() == Some("CullPerPageDrawCommandsCs")
            && matches_program(&ex.isa, 8052, 0x5d796603fd1dea38)
        || ex.entry.as_deref() == Some("BuildCardsUpdateListCS") && [
            (2440, 0x4d1c4843bac1c845),
            (2640, 0x72b5ee7a38d3445d),
        ].into_iter().any(|(size, hash)| matches_program(&ex.isa, size, hash))
        || ex.entry.as_deref() == Some("InstanceCullVSM") && [
            (7740, 0x2cb56c8263df11b9), (5644, 0x2a06c03631aa33d4),
            (8068, 0x0d2da7ed428e363a), (5636, 0xdcf5938525abc31a),
            (8092, 0x1ef46547771249a3), (5380, 0x0815f4188e78d1ad),
            (7740, 0x138ce6201f23fcc9), (5396, 0x83f25286a0b05e8d),
        ].into_iter().any(|(size, hash)| matches_program(&ex.isa, size, hash)))
}

// The PC and all scalar branch decisions are identical for the 64 invocations,
// so the group can rendezvous inside the branches and loops actually taken.
pub(super) fn prepare_insn(em: &mut Emitter<'_>, idx: usize, i: &Insn) -> Result<(), String> {
    if let Some(code) = super::page_waves::rendezvous(idx, i)? {
        em.line(&code);
    }
    if matches!(i.mnemonic.as_str(), "s_cbranch_scc0" | "s_cbranch_scc1") {
        let prev = &em.insns[idx - 1];
        if prev.mnemonic == "s_andn2_b64" {
            em.line(&format!("scc = any(tb_page_ballot(tb_wave_tid, ({} & 1u) != 0u));", r(&prev.dst[0])?));
        }
    }
    Ok(())
}

pub(super) fn emit(em: &mut Emitter<'_>, idx: usize, i: &Insn, target: Option<usize>) -> Result<bool,String> {
    let op = i.mnemonic.as_str();
    match op {
        "s_cmp_lg_u32" if i.src == [Opnd::ExecHi, Opnd::ConstInt(0)] => {
            em.line("scc = (tb_page_ballot(tb_wave_tid, (exec_lo & 1u) != 0u).y != 0u);");
        }
        "s_cselect_b32" if i.src == [Opnd::ExecHi, Opnd::ExecLo] => {
            em.line(&format!("uint2 exec_words{idx} = tb_page_ballot(tb_wave_tid, (exec_lo & 1u) != 0u);"));
            em.ws(&i.dst[0], &format!("(scc != 0u ? exec_words{idx}.y : exec_words{idx}.x)"))?;
        }
        "s_cbranch_vccnz" => {
            let target = target.ok_or("backward VCC branch in instance cull")?;
            em.line(&format!("if (any(tb_page_ballot(tb_wave_tid, (vcc_lo & 1u) != 0u))) j{target} = true;"));
        }
        "v_readfirstlane_b32" => {
            em.line(&format!("uint2 first_mask{idx} = tb_page_ballot(tb_wave_tid, (exec_lo & 1u) != 0u);"));
            let mask = format!("first_mask{idx}");
            let lane = format!("({mask}.x != 0u ? uint(firstbitlow({mask}.x)) : 32u + uint(firstbitlow({mask}.y)))");
            em.ws(&i.dst[0], &format!("tb_page_read(tb_wave_tid, {}, {lane})", r(&i.src[0])?))?;
        }
        "s_mov_b32" if matches!(i.dst.first(), Some(Opnd::ExecLo | Opnd::ExecHi)) => {
            let side = if i.dst[0] == Opnd::ExecLo { "<" } else { ">=" };
            em.line(&format!("if (tb_wave_tid {side} 32u) exec_lo = ({} >> (tb_wave_tid & 31u)) & 1u;", r(&i.src[0])?));
        }
        "v_permlanex16_b32" => {
            let selector = match (&i.src[1..], i.raw[0] & 0xff00) {
                ([Opnd::ConstInt(0), Opnd::ConstInt(0)], 0) => 0,
                ([Opnd::ConstInt(-1), Opnd::ConstInt(-1)], 0x4800) => 15,
                _ => return Err("unrecognized cross-row cull permutation".into()),
            };
            let src = r(&i.src[0])?;
            em.line(&format!("uint other_row{idx} = tb_page_read(tb_wave_tid, {src}, ((tb_wave_tid & ~15u) ^ 16u) + {selector}u);"));
            em.wv(&i.dst[0], &format!("other_row{idx}"))?;
        }
        "v_add_nc_u32" | "v_add_f32" if matches!(i.src.first(), Some(Opnd::Raw(233 | 250))) => {
            let code = i.raw[1] >> 8;
            let src = format!("v{}", i.raw[1] & 255);
            let mut valid = "true".to_owned();
            let lane = match (i.src[0], code) {
                (Opnd::Raw(233), 0xde54c1 | 0x936012 | 0x000924) =>
                    format!("(tb_wave_tid & ~7u) | (({code:#x}u >> (3u * (tb_wave_tid & 7u))) & 7u)"),
                (Opnd::Raw(250), 0xff0940) => "(tb_wave_tid ^ 15u)".into(),
                (Opnd::Raw(250), 0xff0911 | 0xff0112 | 0xff0114 | 0xff0118) => {
                    let k = code & 15;
                    valid = format!("((tb_wave_tid & 15u) >= {k}u)");
                    format!("tb_wave_tid - {k}u")
                }
                _ => return Err("unrecognized cull DPP control".into()),
            };
            em.line(&format!("uint shifted{idx} = tb_page_read(tb_wave_tid, {src}, {lane});"));
            let other = r(&i.src[1])?;
            let result = if op == "v_add_f32" {
                format!("asuint(asfloat(shifted{idx}) + asfloat({other}))")
            } else {
                format!("(shifted{idx} + {other})")
            };
            em.wv(&i.dst[0], &format!("({valid} ? {result} : {other})"))?;
        }
        _ => return super::page_waves::emit(em, idx, i, target),
    }
    Ok(true)
}
