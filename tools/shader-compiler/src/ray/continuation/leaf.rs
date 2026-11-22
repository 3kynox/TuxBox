//! Leaf hit programs address the caller's payload directly and return through
//! a separate word. Validate both address calculations before indexing either.
use super::{alu, continuation, layout, resources, scratch, target};
use crate::rdna2::{instructions, Enc, Insn, Opnd};
use std::ops::Range;
use tuxbox_dxbc::ray::State;

pub(in crate::ray) struct Leaf {
    pub declarations: String,
    pub body: String,
}

pub(super) struct Stack {
    pub words: usize,
    pub return_read: usize,
}

fn writes(ins: &Insn, vgpr: u8) -> bool {
    if ins.mnemonic.contains("store")
        || matches!(
            ins.mnemonic.as_str(),
            "v_nop" | "v_readlane_b32" | "v_readfirstlane_b32"
        )
    {
        return false;
    }
    let width = if ins.enc == Enc::Mimg {
        ins.aux.mask.count_ones() as u8
    } else if ins.mnemonic.contains("dwordx4") {
        4
    } else if ins.mnemonic.contains("dwordx3") {
        3
    } else if ins.mnemonic.contains("dwordx2") || ins.mnemonic.ends_with("64") {
        2
    } else {
        1
    };
    ins.dst.iter().any(
        |op| matches!(op, Opnd::Vgpr(base) if vgpr.checked_sub(*base).is_some_and(|i| i < width)),
    )
}

impl Stack {
    fn analyze(isa: &[u8], span: &Range<usize>, words: usize) -> Result<Self, String> {
        if !(1..=64).contains(&words) {
            return Err("unsupported ray leaf payload width".into());
        }
        let mut ops: Vec<_> = instructions(&isa[span.clone()]).collect();
        for ins in &mut ops {
            ins.offset += span.start;
        }
        let prefix = ops
            .iter()
            .position(|i| target(i).is_some())
            .unwrap_or(ops.len());
        let initial = |operand: &Opnd, original: u8, before: usize| {
            let Opnd::Vgpr(reg) = *operand else {
                return false;
            };
            let prefix = &ops[..before.min(prefix)];
            if reg == original {
                !prefix.iter().any(|i| writes(i, original))
            } else {
                prefix
                    .iter()
                    .rposition(|i| writes(i, reg))
                    .is_some_and(|copy| {
                        prefix[copy].mnemonic == "v_mov_b32"
                            && prefix[copy].src == [Opnd::Vgpr(original)]
                            && !prefix[..copy].iter().any(|i| writes(i, original))
                    })
            }
        };
        let address = |ins: &Insn| {
            ins.mnemonic == "v_lshl_add_u32"
                && matches!(
                    ins.src.as_slice(),
                    [Opnd::Vgpr(_), Opnd::ConstInt(2), Opnd::Vgpr(_)]
                )
        };
        let payload = ops
            .iter()
            .enumerate()
            .position(|(i, op)| {
                address(op)
                    && op.dst == [Opnd::Vgpr(8)]
                    && initial(&op.src[0], 9, i)
                    && initial(&op.src[2], 26, i)
            })
            .ok_or("missing ray leaf payload address")?;
        let return_read = ops
            .iter()
            .rposition(|i| scratch(i) && i.aux.mask == 1)
            .ok_or("missing ray leaf return token")?;
        let read = &ops[return_read];
        let ret = ops
            .iter()
            .position(|i| i.mnemonic == "s_setpc_b64")
            .ok_or("missing ray leaf return")?;
        if read.mnemonic != "buffer_load_dword"
            || read.dst != [Opnd::Vgpr(8)]
            || read.aux.imm != 0
            || return_read >= ret
            || ops[return_read + 1..ret]
                .iter()
                .any(|i| i.src.contains(&Opnd::Vgpr(8)) || target(i).is_some())
        {
            return Err("unsupported ray leaf return token".into());
        }
        let exit = ops[..return_read]
            .iter()
            .rposition(|i| {
                address(i) && i.dst.first() == read.src.first() && i.src[2] == ops[payload].src[2]
            })
            .ok_or("missing ray leaf return address")?;
        // A large leaf may preserve SP in a VGPR, then reuse v7 for the
        // duration of the shader. On exit it restores v7 from that VGPR.
        let stack_source = &ops[exit].src[0];
        let sp = Opnd::Vgpr(7);
        let saved_sp = ops[..exit].iter().rev().find(|i| writes(i, 7));
        let native_sp = if stack_source == &sp {
            saved_sp.is_none()
        } else {
            let copied = ops[..prefix.min(exit)].iter().position(|i| {
                i.mnemonic == "v_mov_b32"
                    && i.dst.first() == Some(stack_source)
                    && i.src == [sp.clone()]
            });
            copied.is_some_and(|i| !ops[..i].iter().any(|op| writes(op, 7)))
                && saved_sp.is_some_and(|i| {
                    i.mnemonic == "v_mov_b32" && i.src.first() == Some(stack_source)
                })
        };
        if payload >= prefix
            || payload >= exit
            || !native_sp
            || ops[payload + 1..return_read]
                .iter()
                .enumerate()
                .any(|(i, op)| writes(op, 8) && payload + 1 + i != exit)
            || ops.iter().filter_map(target).any(|pc| {
                pc <= ops[payload].offset || (ops[exit].offset < pc && pc <= ops[ret].offset)
            })
        {
            return Err("unsupported ray leaf payload or return address lifetime".into());
        }
        for (index, ins) in ops[..return_read]
            .iter()
            .enumerate()
            .filter(|(_, i)| scratch(i) && i.aux.mask == 1)
        {
            let word_offset = ins.aux.imm;
            if index <= payload
                || (index >= exit && ops[exit].dst == [Opnd::Vgpr(8)])
                || ins.src[0] != Opnd::Vgpr(8)
                || !matches!(
                    ins.mnemonic.as_str(),
                    "buffer_load_dword" | "buffer_store_dword"
                )
                || !(0..(words * 4) as i64).contains(&word_offset)
                || word_offset % 4 != 0
            {
                return Err(format!(
                    "unsupported ray leaf scratch access at {:#x}",
                    ins.offset
                ));
            }
        }
        Ok(Self {
            words,
            return_read: read.offset,
        })
    }
}

pub(in crate::ray) fn leaf_body(state: &State<'_>, payload_words: usize) -> Result<Leaf, String> {
    let span = layout::leaf_span(state)?;
    let stack = Stack::analyze(state.program, &span, payload_words)?;
    let bindings = resources::Bindings::analyze(state, std::slice::from_ref(&span))?;
    let mut body = String::new();
    continuation(
        state.program,
        span.start,
        span.end,
        &mut body,
        &bindings,
        Some(&stack),
        false,
    )?;
    let mut declarations = bindings.declarations;
    declarations.push_str(alu::HLSL);
    Ok(Leaf { declarations, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_payload_offsets_and_return_token_have_distinct_bounds() {
        let mut words = vec![
            3611688968, 73991433, // v8 = v9 * 4 + v26 (incoming payload)
            3761246244, 67116296, // v29 = payload[9]
            3765440512, 67110408, // payload[0] = v6
            3611688968, 73991431, // v8 = v7 * 4 + v26 (return token)
            3761246208, 67110920,   // v8 = scratch[v8]
            3196067870, // return to scheduler
        ];
        let run = |words: &[u32], count| {
            let bytes: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            Stack::analyze(&bytes, &(0..bytes.len()), count)
        };
        assert_eq!(run(&words, 10).unwrap().return_read, 32);
        assert!(run(&words, 9).is_err());
        words[8] |= 4; // return token must be at offset zero
        assert!(run(&words, 10).is_err());
        words[8] &= !4;
        words.insert(0, 0xbf82_0002); // skip the payload address calculation
        assert!(run(&words, 10).is_err());
    }
}
