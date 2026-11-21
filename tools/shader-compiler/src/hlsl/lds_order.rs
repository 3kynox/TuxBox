use super::{Emitter, Enc, Insn};
use std::collections::BTreeSet;

/// A guest wave can exchange LDS data using LGKM waits alone. A workgroup of
/// up to 64 invocations may span multiple host subgroups, which must finish
/// reading a tile before any overwrite it, and publish writes before reading.
pub(super) fn exchange_waits(insns: &[Insn]) -> BTreeSet<usize> {
    let mut barriers = BTreeSet::new();
    let mut last_write = None;
    let mut drained = None;
    let mut convergent = true;
    for (index, ins) in insns.iter().enumerate() {
        let name = ins.mnemonic.as_str();
        if ins.enc == Enc::Sopp && (Emitter::branch_cond(ins.op).is_some() || name == "s_endpgm") {
            // A wait before this branch can still publish a tile consumed by
            // only some lanes. The branch itself can split host invocations.
            convergent = false;
        }
        if name == "s_waitcnt" && ins.aux.imm & 0x3f00 == 0 && convergent {
            drained.get_or_insert(index);
        }
        if name == "s_barrier" {
            last_write = None;
            drained = None;
        }
        if ins.enc != Enc::Ds {
            continue;
        }
        let write = name.starts_with("ds_write");
        if ins.raw[0] & (1 << 17) == 0 && (write || name.starts_with("ds_read")) {
            if last_write.is_some_and(|prior| prior != write) {
                barriers.extend(drained);
            }
            last_write = Some(write);
        } else {
            last_write = None;
        }
        drained = None;
    }
    barriers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rdna2::disassemble;

    #[test]
    fn order_lds_phases_at_convergent_draining_waits() {
        let write = [0xd8380100, 0x00020100]; // ds_write2_b32
        let read = [0xd8dc0100, 0x00000000]; // ds_read2_b32
        let drain = [0xbf8cc07f]; // lgkmcnt(0)
        let busy = [0xbf8cc17f]; // lgkmcnt(1)
        let branch = [0xbf880001]; // s_cbranch_execz
        let barrier = [0xbf8a0000]; // s_barrier
        let sites = |ops: &[&[u32]]| {
            let isa = ops
                .iter()
                .flat_map(|op| op.iter().flat_map(|w| w.to_le_bytes()))
                .collect::<Vec<_>>();
            exchange_waits(&disassemble(&isa))
                .into_iter()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            sites(&[&write, &write, &drain, &read, &drain, &write, &drain, &branch, &read]),
            [2, 4, 6]
        );
        assert!(sites(&[&write, &busy, &read]).is_empty());
        assert!(sites(&[&write, &branch, &drain, &read]).is_empty());
        assert!(sites(&[&write, &drain, &barrier, &read]).is_empty());
    }
}
