//! Resource layout of the Xbox persistent hierarchy and cluster queue kernel.
use super::{Emitter, Enc, Opnd, Res};
use crate::meta::RootParamKind;

pub(super) fn prepare(em: &mut Emitter<'_>) -> Result<(), String> {
    // Four user-data SGPRs are descriptor table pointers, in this order.
    let root = em
        .ex
        .root_sig
        .as_ref()
        .ok_or("missing PersistentClusterCull root signature")?;
    let tables = [(0, 7), (2, 1), (3, 0), (1, 6)];
    if root.params.len() != tables.len()
        || !root.params.iter().zip(tables).all(|(p, (kind, minimum))| {
            p.visibility == 0
                && matches!(&p.kind, RootParamKind::Table(ranges)
                if matches!(ranges.as_slice(), [r] if r.range_type == kind
                    && r.space == 0 && r.base_register == 0 && r.num >= minimum
                    && matches!(r.offset_in_table, 0 | u32::MAX)))
        })
    {
        return Err("unsupported PersistentClusterCull descriptor tables".into());
    }
    let bindings = [
        (2, 0),
        (5, 0),
        (5, 1),
        (5, 2),
        (4, 3),
        (5, 4),
        (4, 5),
        (5, 6),
        (8, 0),
        (8, 1),
        (7, 2),
        (7, 3),
        (7, 4),
        (6, 5),
    ];
    if em.ex.resources.len() != bindings.len()
        || !bindings.iter().all(|&(kind, slot)| {
            em.ex
                .resources
                .iter()
                .any(|r| (r.res_type, r.space, r.lower, r.upper) == (kind, 0, slot, slot))
        })
    {
        return Err("unsupported PersistentClusterCull resource bindings".into());
    }
    // Vector accesses address float4 scene rows, uint instance lookups and
    // three-word streaming requests. All five queue counters are fields of
    // element zero, including the pending-node count at byte 20.
    for (key, words) in [
        ((false, 0), 4),
        ((false, 1), 4),
        ((false, 2), 1),
        ((true, 0), 3),
        ((true, 1), 6),
    ] {
        if em.sbuf.get(&key) != Some(&words) {
            return Err(format!(
                "unsupported PersistentClusterCull structured access {key:?}"
            ));
        }
    }
    // Views have 192 dwords and prior visible counts have two. The single-view
    // kernel reads view zero with scalar loads. Multiview uses the view index
    // from the queue entry with the same constant field offsets.
    for (slot, expected) in [(4, 192), (6, 2)] {
        let mut words = 0;
        for (&index, resources) in &em.res {
            if resources.as_slice() != [Res::Buf(slot, false)] {
                continue;
            }
            let load = &em.insns[index];
            let width = match load.enc {
                Enc::Smem
                    if (8..=12).contains(&load.op)
                        && load.raw.get(1).map(|w| w >> 25) == Some(0x7d) =>
                {
                    Some(1 << (load.op - 8))
                }
                Enc::Mubuf
                    if slot == 4
                        && load.aux.mask == 2
                        && matches!(
                            load.src.as_slice(),
                            [Opnd::Vgpr(_), Opnd::Sgpr(_), Opnd::ConstInt(0)]
                        )
                        && matches!(
                            load.mnemonic.as_str(),
                            "buffer_load_dword"
                                | "buffer_load_dwordx2"
                                | "buffer_load_dwordx3"
                                | "buffer_load_dwordx4"
                        ) =>
                {
                    Emitter::mubuf_dwords(&load.mnemonic)
                }
                _ => None,
            }
            .ok_or_else(|| format!("unsupported PersistentClusterCull t{slot} addressing"))?;
            if !(0..expected as i64 * 4).contains(&load.aux.imm) || load.aux.imm % 4 != 0 {
                return Err(format!(
                    "invalid PersistentClusterCull t{slot} field offset"
                ));
            }
            words = words.max(load.aux.imm as u32 / 4 + width);
        }
        if words != expected {
            return Err(format!("invalid PersistentClusterCull t{slot} extent"));
        }
        em.sbuf.insert((false, slot), words);
    }
    em.persistent_queue = true;
    Ok(())
}
