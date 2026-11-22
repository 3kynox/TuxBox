//! FSPX native compute sections observed in the Scarlett FidelityFX upscaler.
//! Version 1 stores a directory immediately after its four-word header. Section
//! addresses are absolute byte offsets from the first byte of the FSPX record.

use super::fssm::executable_extent;
use super::{NativeProgram, Stage};

fn words<const N: usize>(mut bytes: &[u8]) -> Result<[u32; N], String> {
    let mut values = [0; N];
    for value in &mut values {
        let (field, rest) = bytes
            .split_first_chunk::<4>()
            .ok_or("truncated FSPX record")?;
        *value = u32::from_le_bytes(*field);
        bytes = rest;
    }
    Ok(values)
}

pub(super) fn extract(body: &[u8], threads: [u32; 3]) -> Result<NativeProgram, String> {
    let [magic, version, count, total] = words(body)?;
    if magic != u32::from_le_bytes(*b"FSPX") || version != 1 {
        return Err("unrecognized FSPX header".into());
    }
    let file = body.get(..total as usize).ok_or("truncated FSPX image")?;
    let directory_end = (count as usize)
        .checked_mul(16)
        .and_then(|n| n.checked_add(16))
        .ok_or("FSPX directory size overflow")?;
    let directory = file
        .get(16..directory_end)
        .ok_or("truncated FSPX directory")?;

    let mut spans = Vec::with_capacity(directory.len() / 16);
    let mut config = None;
    let mut code = None;
    for descriptor in directory.chunks_exact(16) {
        let [kind, offset, length, _reserved] = words(descriptor)?;
        let start = offset as usize;
        let end = start
            .checked_add(length as usize)
            .ok_or("FSPX section length overflow")?;
        let section = file
            .get(start..end)
            .ok_or("FSPX section outside declared image")?;
        if !section.is_empty() {
            if start < directory_end {
                return Err("FSPX section overlaps directory".into());
            }
            spans.push((start, end));
        }
        match kind {
            0 => {
                if config.replace(words::<7>(section)?).is_some() {
                    return Err("multiple FSPX compute configurations".into());
                }
            }
            6 => {
                if code.replace(section).is_some() {
                    return Err("multiple FSPX code sections".into());
                }
            }
            _ => {}
        }
    }
    spans.sort_unstable();
    if spans.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err("overlapping FSPX sections".into());
    }
    let code = code.ok_or("missing FSPX code section")?;
    if code.len() % 4 != 0 {
        return Err("partial dword in FSPX code section".into());
    }
    let isa_bytes = executable_extent(code)?;
    Ok(NativeProgram {
        stage: Stage::Compute,
        sm: (0, 0),
        bitcode_stripped: false,
        fssm: Vec::new(),
        isa: code[..isa_bytes].to_vec(),
        isa_off: 0,
        isa_preamble: [0, threads[0], threads[1], threads[2]],
        isa_data: code.to_vec(),
        fspx_config: config,
        entry: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> Vec<u8> {
        let words = [
            u32::from_le_bytes(*b"FSPX"),
            1,
            2,
            108,
            6,
            76,
            32,
            0, // Deliberately place code first in the directory.
            0,
            48,
            28,
            0,
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            0xbf85_0001,
            0xbf81_0000,
            0xbe80_03ff,
            0xbf81_0000, // an endpgm literal
            0xbf81_0000,
            0xbf9f_0000,
            0x1a01,
            0xbf9f_0000,
        ];
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    #[test]
    fn extracts_code_and_preserves_the_complete_native_section() {
        let bytes = file();
        let program = extract(&bytes, [32, 8, 1]).unwrap();
        assert_eq!(program.isa, bytes[76..96]);
        assert_eq!(program.isa_data, bytes[76..108]);
        assert_eq!(program.isa_preamble, [0, 32, 8, 1]);
        assert_eq!(program.fspx_config, Some([1, 2, 3, 4, 5, 6, 7]));
    }

    #[test]
    fn rejects_damaged_directory_code_and_configuration() {
        let bytes = file();
        for n in 0..bytes.len() {
            assert!(extract(&bytes[..n], [1; 3]).is_err());
        }
        for (at, bad) in [
            (0, 0u32),
            (4, 2),
            (8, u32::MAX),
            (12, 40),
            (16, 5),
            (20, 32),
            (20, 72),
            (20, u32::MAX),
            (24, 31),
            (32, 6),
            (40, 24),
            (76, 0xbf82_7fff),
            (76, 0xbf82_0004),
        ] {
            let mut damaged = bytes.clone();
            damaged[at..at + 4].copy_from_slice(&bad.to_le_bytes());
            assert!(extract(&damaged, [1; 3]).is_err(), "field {at} value {bad}");
        }
    }
}
