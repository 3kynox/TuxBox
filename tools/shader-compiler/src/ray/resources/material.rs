//! Local shader records and descriptor waterfalls in triangle hit programs.
//! Descriptor addresses below are internal values. The host reads the actual
//! root arguments associated with this hit record for each invocation.
use super::{local_record, Bindings};
use crate::rdna2::{instructions, Enc, Insn, Opnd};
use std::{collections::BTreeMap, fmt::Write, ops::Range};
use tuxbox_dxbc::ray::State;

const SRV: u32 = 0x1000_0000;
const CBV: u32 = 0x2000_0000;
const SAMPLER: u32 = 0x4000_0000;
const BVH_BASE: u32 = 0x3000_0000;
const TRIANGLE_BASE: u32 = 0x4000_0000;

const GEOMETRY: &str = r#"
ByteAddressBuffer tbIndices : register(t0,space2);
ByteAddressBuffer tbVertices : register(t1,space2);
cbuffer TriangleParameters : register(b0,space2) { uint4 tbGeometry; };
"#;

fn index(op: &Opnd) -> Option<usize> {
    match op {
        Opnd::Sgpr(n) if *n < 128 => Some(*n as usize),
        Opnd::Vgpr(n) if *n < 128 => Some(128 + *n as usize),
        _ => None,
    }
}

#[derive(Clone)]
struct Values([Option<u32>; 384]);

impl Values {
    fn new() -> Self {
        let mut words = [None; 384];
        for (i, v) in [
            (8, 0),
            (128 + 5, 0),
            (128 + 2, 8),
            (128 + 3, 8),
            (128 + 16, BVH_BASE >> 8),
            (128 + 17, 0),
        ] {
            words[i] = Some(v);
        }
        Self(words)
    }

    fn get(&self, op: &Opnd) -> Option<u32> {
        match op {
            Opnd::ConstInt(x) => Some(*x as u32),
            Opnd::Literal(x) => Some(*x),
            _ => self.0[index(op)?],
        }
    }

    fn pair(&self, op: &Opnd) -> Option<u64> {
        let k = index(op)?;
        Some(u64::from(self.0[k]?) | (u64::from(self.0[k + 1]?) << 32))
    }

    fn alu(&self, ins: &Insn) -> [Option<u32>; 2] {
        let x = ins.src.first().and_then(|x| self.get(x));
        let y = ins.src.get(1).and_then(|x| self.get(x));
        let z = ins.src.get(2).and_then(|x| self.get(x));
        let pair = match ins.mnemonic.as_str() {
            "s_lshr_b64" => ins
                .src
                .first()
                .and_then(|op| self.pair(op))
                .zip(y)
                .map(|(x, s)| x.wrapping_shr(s)),
            "v_lshlrev_b64" => ins
                .src
                .get(1)
                .and_then(|op| self.pair(op))
                .zip(x)
                .map(|(y, s)| y.wrapping_shl(s)),
            _ => None,
        };
        if let Some(pair) = pair {
            return [Some(pair as u32), Some((pair >> 32) as u32)];
        }
        let value = match ins.mnemonic.as_str() {
            "v_mov_b32" | "s_mov_b32" | "v_readfirstlane_b32" => x,
            "v_add_nc_u32" | "v_add_co_u32" | "s_add_u32" => {
                x.zip(y).map(|(x, y)| x.wrapping_add(y))
            }
            // Local tables use zero-based offsets smaller than 4 GiB. Carry
            // cannot cross the high dword while indexing one of these tables.
            "v_add_co_ci_u32" | "s_addc_u32" => x.zip(y).map(|(x, y)| x.wrapping_add(y)),
            "s_lshl_b32" => x.zip(y).map(|(x, y)| x.wrapping_shl(y)),
            "v_or_b32" => x.zip(y).map(|(x, y)| x | y),
            "v_and_b32" => x.zip(y).map(|(x, y)| x & y),
            "v_lshrrev_b32" => x.zip(y).map(|(x, y)| y.wrapping_shr(x)),
            "v_lshl_add_u32" => x
                .zip(y)
                .zip(z)
                .map(|((x, y), z)| x.wrapping_shl(y).wrapping_add(z)),
            "v_cndmask_b32" if x == y => x,
            _ => None,
        };
        [value, None]
    }
}

enum Word {
    Immediate(u32),
    Value(&'static str),
    InverseTransform(u32, u32),
}

fn local_word(address: u32) -> Option<Word> {
    // The virtual nodes have one 64-byte unit of offset, like v2/v3 at
    // entry. The native shader uses node+48, +60, +112 and triangle+52.
    Some(match address {
        32 | 40 | 68 | 76 | 84 => Word::Immediate(0),
        36 => Word::Immediate(1),
        44 => Word::Immediate(2),
        48 => Word::Value("tbGeometry.x"),
        52 => Word::Value("tbGeometry.y"),
        56 => Word::Value("tbGeometry.z"),
        60 => Word::Value("tbGeometry.w"),
        64 => Word::Immediate(SRV),
        72 => Word::Immediate(CBV),
        80 => Word::Immediate(SAMPLER),
        0x3000_0070 => Word::Value("InstanceID()"),
        0x3000_007c => Word::Immediate(TRIANGLE_BASE >> 8),
        // The sideband matrix lays its three translation words before
        // the row-major inverse linear transform. The hit shader multiplies
        // the object-space face normal by its transpose. DXR exposes the
        // uncompressed world-to-object transform for the intersected instance.
        0x3000_0080..=0x3000_0088 if address.is_multiple_of(4) => {
            Word::InverseTransform((address - 0x3000_0080) / 4, 3)
        }
        0x3000_008c..=0x3000_00ac if address.is_multiple_of(4) => {
            let element = (address - 0x3000_008c) / 4;
            Word::InverseTransform(element / 3, element % 3)
        }
        0x3000_00b0 => Word::Value("InstanceIndex()"),
        0x3000_00b8 => Word::Immediate(0), // uncompressed 3x3 matrix
        0x4000_0074 => Word::Value("PrimitiveIndex()"),
        _ => return None,
    })
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum View {
    Structured,
    Typed,
    Constant,
    Page,
    Texture,
    Sampler,
}

#[derive(Default)]
struct Layout {
    views: BTreeMap<(u32, u32), View>,
    emitted: BTreeMap<usize, String>,
    // Known VGPR contents saved at fixed offsets in the compiler-private frame.
    spills: BTreeMap<u32, Option<u32>>,
}

impl Layout {
    fn resource(&mut self, value: u32, bank: u32, kind: View) -> Option<String> {
        let stride = if bank == SAMPLER { 16 } else { 32 };
        let offset = value.checked_sub(bank)?;
        let slot = offset / stride;
        if !offset.is_multiple_of(stride) || slot >= if bank == SRV { 64 } else { 16 } {
            return None;
        }
        if self
            .views
            .insert((bank, slot), kind)
            .is_some_and(|old| old != kind)
        {
            return None;
        }
        Some(format!(
            "tb{}{}",
            match bank {
                SRV => "Srv",
                CBV => "Cb",
                SAMPLER => "Sampler",
                _ => return None,
            },
            slot
        ))
    }

    fn buffer(&mut self, ins: &Insn, regs: &Values, dst: u8) -> Option<String> {
        let [Opnd::Vgpr(at), base, Opnd::ConstInt(0)] = ins.src.as_slice() else {
            return None;
        };
        let pointer = regs.get(base)?;
        let count = match ins.op {
            0..=3 => 1 + ins.op,
            12 => 1,
            13 => 2,
            14 => 4,
            15 => 3,
            _ => return None,
        };
        let constant = (CBV..CBV + 512).contains(&pointer);
        let kind = if constant {
            View::Constant
        } else if ins.op < 4 {
            View::Typed
        } else {
            View::Structured
        };
        let name = self.resource(pointer, if constant { CBV } else { SRV }, kind)?;
        let offset = u32::try_from(ins.aux.imm).ok()?;
        if !offset.is_multiple_of(4) || offset / 4 + count > 4 {
            return None;
        }
        let element = match ins.aux.mask {
            0 => "0".to_string(),
            2 => format!("v[{at}]"),
            _ => return None,
        };
        let mut body = format!("{{ uint4 words=asuint({name}[{element}]);");
        for i in 0..count {
            write!(
                body,
                " v[{}]=words[{}];",
                u32::from(dst) + i,
                offset / 4 + i
            )
            .ok()?;
        }
        body.push_str(" }");
        Some(body)
    }

    fn image(&mut self, ins: &Insn, regs: &Values, dst: u8) -> Option<String> {
        if ins.aux.imm != 1 || ins.raw.get(1)? & (3 << 30) != 0 {
            return None;
        }
        let src = &ins.src;
        let base = src.get(src.len().checked_sub(2)?)?;
        let mip = ins.op == 1;
        let tex = self.resource(
            regs.get(base)?,
            SRV,
            if mip { View::Page } else { View::Texture },
        )?;
        let n = if matches!(ins.op, 1 | 36) { 3 } else { 2 };
        let c = super::lumen::coords(ins, n)?;
        let (x, y) = (c[0], c[1]);
        let sample = if mip {
            format!("{tex}.Load(int3(v[{x}],v[{y}],v[{}]))", c[2])
        } else {
            let level = match ins.op {
                39 => "0.0".to_string(),
                36 => format!("asfloat(v[{}])", c[2]),
                _ => return None,
            };
            let samp = self.resource(regs.get(src.last()?)?, SAMPLER, View::Sampler)?;
            format!("asuint({tex}.SampleLevel({samp},asfloat(uint2(v[{x}],v[{y}])),{level}))")
        };
        super::lumen::save_components(dst.into(), ins.aux.mask, &sample)
    }

    fn instruction(&mut self, ins: &Insn, regs: &mut Values) -> Option<()> {
        let dst = ins.dst.first().and_then(index);
        let dw = match ins.dst.first() {
            Some(Opnd::Sgpr(x) | Opnd::Vgpr(x)) => *x,
            _ => 0,
        };
        if ins
            .src
            .iter()
            .chain(&ins.dst)
            .any(|op| matches!(op, Opnd::Vgpr(128..=255) | Opnd::Sgpr(128..=255)))
        {
            return None;
        }
        if super::super::continuation::scratch(ins)
            && ins.aux.mask == 0
            && matches!(ins.op, 12 | 28)
        {
            let addr = u32::try_from(ins.aux.imm).ok()?;
            if !addr.is_multiple_of(4) {
                return None;
            }
            let k = dst?;
            if ins.op == 28 {
                self.spills.insert(addr, regs.0[k]);
            } else {
                regs.0[k] = self.spills.get(&addr).copied().flatten();
            }
            return Some(());
        }
        let mut words = [None; 16];
        let (n, body) = match ins.enc {
            Enc::Flat => {
                let [Opnd::Vgpr(addr), _, scalar] = ins.src.as_slice() else {
                    return None;
                };
                let count = match ins.op {
                    12 => 1,
                    13 => 2,
                    14 => 4,
                    15 => 3,
                    _ => return None,
                };
                if ins.aux.mask != 2 {
                    return None;
                }
                let offset = u32::try_from(ins.aux.imm).ok()?;
                let (lo, hi) = if matches!(scalar, Opnd::Null) {
                    (regs.get(&ins.src[0]), regs.0[128 + usize::from(*addr) + 1])
                } else {
                    (
                        regs.get(&ins.src[0])
                            .zip(regs.get(scalar))
                            .map(|(v, s)| v.wrapping_add(s)),
                        regs.0[index(scalar)? + 1],
                    )
                };
                if usize::from(dw) + count > 128 {
                    return None;
                }
                let mut b = String::from("{ ");
                if matches!(hi, Some(1 | 2)) {
                    let resource = if hi == Some(1) {
                        "tbIndices"
                    } else {
                        "tbVertices"
                    };
                    match scalar {
                        Opnd::Null => write!(b, "uint at = v[{addr}]; ").ok()?,
                        Opnd::Sgpr(base) => write!(b, "uint at = v[{addr}] + s[{base}]; ").ok()?,
                        _ => return None,
                    }
                    for lane in 0..count {
                        write!(
                            b,
                            "v[{}] = {resource}.Load(at + {}u); ",
                            usize::from(dw) + lane,
                            offset.checked_add(4 * lane as u32)?
                        )
                        .ok()?;
                    }
                } else {
                    if hi.is_some_and(|word| word != 0) {
                        return None;
                    }
                    let at = lo?.checked_add(offset)?;
                    for (i, out) in words[..count].iter_mut().enumerate() {
                        let value = match local_word(at.checked_add(4 * i as u32)?)? {
                            Word::Immediate(x) => {
                                *out = Some(x);
                                format!("{x}u")
                            }
                            Word::Value(expr) => expr.to_owned(),
                            Word::InverseTransform(row, col) => {
                                format!("asuint(WorldToObject3x4()[{row}][{col}])")
                            }
                        };
                        write!(b, "v[{}] = {value}; ", usize::from(dw) + i).ok()?;
                    }
                }
                b.push('}');
                (count, Some(b))
            }
            Enc::Smem => {
                if !matches!(ins.op, 2 | 3) || ins.aux.imm != 0 || (ins.raw.get(1)? >> 25) != 125 {
                    return None;
                }
                words[0] = regs.get(ins.src.first()?);
                words[0]?;
                // The following use consumes a descriptor through the table.
                // None of the native descriptor words are read as shader data.
                (1 << ins.op, Some(String::new()))
            }
            Enc::Mubuf if !super::super::continuation::scratch(ins) => (
                if ins.op < 4 {
                    (ins.op + 1) as usize
                } else {
                    match ins.op {
                        12 => 1,
                        13 => 2,
                        14 => 4,
                        15 => 3,
                        _ => return None,
                    }
                },
                Some(self.buffer(ins, regs, dw)?),
            ),
            Enc::Mimg => (
                ins.aux.mask.count_ones() as usize,
                Some(self.image(ins, regs, dw)?),
            ),
            _ => {
                if ins.mnemonic.contains("store")
                    || ins.mnemonic == "v_writelane_b32"
                    || ins.mnemonic == "v_nop"
                {
                    return Some(());
                }
                words[..2].copy_from_slice(&regs.alu(ins));
                let n = if ins.enc == Enc::Mubuf {
                    match ins.op {
                        12 | 13 | 14 | 15 => [1, 2, 4, 3][ins.op as usize - 12],
                        _ => 1,
                    }
                } else if ins.mnemonic.ends_with("_b64") {
                    2
                } else {
                    1
                };
                (n, None)
            }
        };
        if let Some(body) = body {
            self.emitted.insert(ins.offset, body);
        }
        if let Some(k) = dst {
            regs.0.get_mut(k..k + n)?.copy_from_slice(&words[..n]);
        }
        Some(())
    }

    fn finish(self, export: &str) -> Bindings {
        let mut declarations = local_record(export);
        declarations.push_str(GEOMETRY);
        for ((_, slot), view) in self.views {
            match view {
                View::Constant=>writeln!(declarations,"cbuffer RecordCb{slot} : register(b{slot},space0) {{ uint4 tbCb{slot}[4096]; }};"),
                View::Sampler=>writeln!(declarations,"SamplerState tbSampler{slot} : register(s{slot},space0);"),
                kind=>{
                    let ty=match kind{View::Structured=>"StructuredBuffer<uint4>",View::Typed=>"Buffer<float4>",View::Page=>"Texture2D<uint4>",View::Texture=>"Texture2D<float4>",_=>unreachable!()};
                    writeln!(declarations,"{ty} tbSrv{slot} : register(t{slot},space0);")
                }
            }.expect("writing a String");
        }
        Bindings {
            declarations,
            instructions: self.emitted,
        }
    }
}

pub(super) fn analyze(state: &State<'_>, span: &Range<usize>) -> Result<Bindings, String> {
    let mut regs = Values::new();
    let mut layout = Layout::default();
    let mut saved_exec = BTreeMap::new();
    let isa = state
        .program
        .get(span.clone())
        .ok_or("invalid ray material substate range")?;
    for mut ins in instructions(isa) {
        ins.offset += span.start;
        // The ELSE side of an EXEC-masked block sees the original VGPRs
        // in those lanes. The THEN path may have reused them for load data.
        if ins.mnemonic == "s_and_saveexec_b32" {
            if let [Opnd::Sgpr(saved)] = ins.dst.as_slice() {
                saved_exec.insert(*saved, regs.clone());
            }
        } else if ins.mnemonic == "s_andn2_b32" && ins.dst == [Opnd::ExecLo] {
            if let [Opnd::Sgpr(saved), Opnd::ExecLo] = ins.src.as_slice() {
                if let Some(entry) = saved_exec.get(saved) {
                    regs.0[128..].copy_from_slice(&entry.0[128..]);
                }
            }
        }
        layout.instruction(&ins, &mut regs).ok_or_else(|| {
            format!(
                "unresolved material resource {} at {:#x}",
                ins.mnemonic, ins.offset
            )
        })?;
    }
    Ok(layout.finish(&state.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(words: &[u32]) -> Insn {
        let code: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let instruction = instructions(&code).next().unwrap();
        instruction
    }

    #[test]
    fn flat_three_word_read_preserves_adjacent_live_descriptor() {
        let mut regs = Values::new();
        let mut layout = Layout::default();
        // global_load_dwordx3 v[51:53], v[51:52], off (triangle vertices).
        let load = decode(&[3694952448, 863830067]);
        regs.0[128 + 51] = None;
        regs.0[128 + 52] = Some(2);
        regs.0[128 + 54] = Some(SRV + 32);
        layout.instruction(&load, &mut regs).unwrap();
        assert_eq!(regs.0[128 + 54], Some(SRV + 32));
        assert!(layout.emitted[&0].contains("tbVertices.Load(at + 8u)"));
    }

    #[test]
    fn recovers_traversal_address_after_private_vgpr_spill() {
        let mut regs = Values::new();
        let mut layout = Layout::default();
        // Material shaders spill the traversal address in v16 while shading
        // and reload it for a second PrimitiveIndex lookup.
        let store = decode(&[3765436464, 83890176]);
        let load = decode(&[3761242160, 83890176]);
        layout.instruction(&store, &mut regs).unwrap();
        regs.0[128 + 16] = None;
        layout.instruction(&load, &mut regs).unwrap();
        assert_eq!(regs.0[128 + 16], Some(BVH_BASE >> 8));
    }

    #[test]
    fn separate_slot_types_share_numbers_in_different_tables() {
        let mut layout = Layout::default();
        assert_eq!(
            layout.resource(SRV + 64, SRV, View::Structured).as_deref(),
            Some("tbSrv2")
        );
        assert_eq!(
            layout.resource(CBV + 64, CBV, View::Constant).as_deref(),
            Some("tbCb2")
        );
        assert!(layout.resource(SRV + 64, SRV, View::Texture).is_none());
        assert!(layout
            .resource(SAMPLER + 3, SAMPLER, View::Sampler)
            .is_none());
    }
}
