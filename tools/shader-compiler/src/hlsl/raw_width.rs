//! Raw buffer transfers stay one dword per HLSL `Load`/`Store`.
//!
//! The MUBUF address is 32-bit (`vaddr + soffset + offset`), so a title can produce an offset
//! that wraps, such as 0xFFFFFFFC. Under vkd3d-proton on NVIDIA, a multi-dword raw load
//! (`Load2`..`Load4`) is lowered through `VK_NV_raw_access_chains`; one 16-byte load at
//! 0xFFFFFFFC lost the device (Xid 109) on an RTX 5080 with Proton Experimental 11.0-20261112.
//! Four 4-byte loads at the same offsets completed and returned zero for the out-of-range word.
//! Merging the per-dword transfers would reintroduce that fault, so this test pins the form.
use super::Emitter;
use crate::rdna2::disassemble;
use crate::{ShaderExtract, Stage};

fn compute_extract(words: &[u32]) -> ShaderExtract {
    let isa: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    ShaderExtract {
        stage: Stage::Compute,
        shader_model: (6, 0),
        required_features: None,
        bitcode_stripped: true,
        fspx_config: None,
        ps09_pixel_inputs: None,
        fssm: Vec::new(),
        entry: Some("main".into()),
        isa_data: isa.clone(),
        isa,
        isa_offset_in_fssm: 0,
        psv_threads: None,
        isa_preamble: [0, 64, 1, 1],
        has_input_sig: false,
        has_output_sig: false,
        has_psv0: false,
        has_root_sig: false,
        input_sig: Vec::new(),
        output_sig: Vec::new(),
        resources: Vec::new(),
        gs_info: None,
        mesh_info: None,
        primitive_sig: Vec::new(),
        root_sig: None,
    }
}

#[test]
fn raw_buffer_transfers_are_emitted_one_dword_at_a_time() {
    // vaddr v2 (OFFEN), vdata v[4:7], srsrc s[8:11], soffset 0.
    let cases = [
        ([0xe038_1000, 0x8002_0402], "buffer_load_dwordx4", "Load"),
        ([0xe078_1000, 0x8002_0402], "buffer_store_dwordx4", "Store"),
    ];
    for (words, mnemonic, op) in cases {
        let ex = compute_extract(&words);
        let insns = disassemble(&ex.isa);
        assert_eq!(insns[0].mnemonic, mnemonic);
        let mut em = Emitter::new(&ex, insns.clone());
        em.raw_transfer_on(&insns[0], "raw_ubuf0".into(), true).unwrap();
        assert_eq!(em.body.matches(&format!("raw_ubuf0.{op}(")).count(), 4, "{mnemonic}:\n{}", em.body);
        for wide in ["2(", "3(", "4("] {
            assert!(!em.body.contains(&format!("{op}{wide}")), "{mnemonic} merged:\n{}", em.body);
        }
    }
}

#[test]
fn static_zero_scalar_offset_matches_immediate_descriptor_load() {
    // s_mov_b32 s66, 0; s_load_dwordx4 s[64:67], s[0:1], 0.
    // The offset register overlaps the destination, so the proof must use
    // the value BEFORE the load, not the outgoing dataflow state.
    let variants = [
        [0xbec2_0380, 0xf408_1000, 0xfa00_0000, 0xbf81_0000],
        [0xbec2_0380, 0xf408_1000, 66u32 << 25, 0xbf81_0000],
    ];
    let mut loads = Vec::new();
    for words in variants {
        let ex = compute_extract(&words);
        let insns = disassemble(&ex.isa);
        let mut em = Emitter::new(&ex, insns);
        em.resolve_resources().unwrap();
        loads.push(em.dsel_loads.clone());
    }
    assert_eq!(loads[0], loads[1]);
    assert!(!loads[0].is_empty());
    let ex = compute_extract(&[0xbec2_0381, 0xf408_1000, 66u32 << 25, 0xbf81_0000]);
    let mut em = Emitter::new(&ex, disassemble(&ex.isa));
    let reason = em.resolve_resources().unwrap_err();
    assert!(reason.contains("dynamic soffset unsupported"), "{reason}");
}

#[test]
fn exact_nonzero_scalar_offset_matches_static_descriptor_and_unknown_refuses() {
    // s66 = 32; compare a dynamic encoding with the equivalent immediate.
    let variants = [
        [0xbec2_03a0, 0xf408_1000, 0xfa00_0020, 0xbf81_0000],
        [0xbec2_03a0, 0xf408_1000, 66u32 << 25, 0xbf81_0000],
    ];
    let mut loads = Vec::new();
    for words in variants {
        let ex = compute_extract(&words);
        let mut em = Emitter::new(&ex, disassemble(&ex.isa));
        em.resolve_resources().unwrap();
        loads.push(em.dsel_loads.clone());
    }
    assert_eq!(loads[0], loads[1]);
    assert_eq!(loads[0].get(&1), Some(&(64, 32)));
    // A path can skip the definition; no constant can be inferred.
    let ex = compute_extract(&[0xbf84_0001, 0xbec2_03a0, 0xf408_1000,
                               66u32 << 25, 0xbf81_0000]);
    let mut em = Emitter::new(&ex, disassemble(&ex.isa));
    assert!(em.resolve_resources().unwrap_err().contains("dynamic soffset"));
}
