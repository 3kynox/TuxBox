//! DXBC container and raster signature readers used by the shader translator and D3D12 guest.
//! Derived from the Apache-2.0 licensed shader reader. See tools/shader-compiler/NOTICE.

fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes
        .get(offset..)
        .and_then(|tail| tail.first_chunk::<4>())
        .ok_or_else(|| format!("DXBC word out of bounds at {offset:#x}"))?;
    Ok(u32::from_le_bytes(*value))
}

/// A DXBC container chunk.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub fourcc: [u8; 4],
    pub offset: usize,
    pub size: usize,
}

/// Parsed DXBC container (the outer wrapper used by both PC and Xbox shaders).
#[derive(Debug, Clone)]
pub struct Dxbc {
    pub digest: [u8; 16],
    pub total_size: u32,
    pub chunks: Vec<Chunk>,
}

impl Dxbc {
    pub fn parse(bytes: &[u8]) -> Result<Dxbc, String> {
        if !bytes.starts_with(b"DXBC") {
            return Err("not a DXBC container (bad magic)".into());
        }
        let total_size = word(bytes, 24)?;
        let container = bytes
            .get(..total_size as usize)
            .ok_or("truncated DXBC container")?;
        let nchunks = word(container, 28)? as usize;
        if nchunks > 64 {
            return Err(format!("implausible chunk count {nchunks}"));
        }
        let directory_end = 32 + nchunks * 4;
        let directory = container
            .get(32..directory_end)
            .ok_or("truncated DXBC chunk directory")?;
        let digest = container[4..20].try_into().unwrap();
        let mut chunks = Vec::with_capacity(nchunks);
        for &entry in directory.as_chunks::<4>().0 {
            let offset = u32::from_le_bytes(entry) as usize;
            if offset < directory_end {
                return Err("DXBC chunk overlaps directory".into());
            }
            let chunk = container
                .get(offset..)
                .ok_or("DXBC chunk offset out of bounds")?;
            let size = word(chunk, 4)? as usize;
            if size > chunk.len() - 8 {
                return Err(format!(
                    "chunk at {offset:#x} size {size} exceeds DXBC container"
                ));
            }
            chunks.push(Chunk {
                fourcc: chunk[..4].try_into().unwrap(),
                offset: offset + 8,
                size,
            });
        }
        Ok(Dxbc {
            digest,
            total_size,
            chunks,
        })
    }

    pub fn chunk(&self, fourcc: &[u8; 4]) -> Option<&Chunk> {
        self.chunks.iter().find(|c| c.fourcc == *fourcc)
    }
}

/// One element of an ISG1/OSG1 signature.
#[derive(Debug, Clone)]
pub struct SigElement {
    pub name: String,
    pub index: u32,
    /// D3D_NAME system value (0 = none, 1 = SV_Position, 64 = SV_Target, ...).
    pub sysval: u32,
    /// D3D_REGISTER_COMPONENT_TYPE: 1 = uint, 2 = int, 3 = float.
    pub comp_type: u32,
    pub register: u32,
    pub mask: u8,
    pub rw_mask: u8,
}

/// Parse an ISG1/OSG1 chunk body (32-byte elements).
pub fn parse_signature(body: &[u8]) -> Result<Vec<SigElement>, String> {
    let count = word(body, 0)?;
    if count > 64 {
        return Err(format!("implausible signature element count {count}"));
    }
    read_signature(body, 32, 4)
}

fn read_signature(
    body: &[u8],
    stride: usize,
    name_field: usize,
) -> Result<Vec<SigElement>, String> {
    let count = word(body, 0)? as usize;
    let header = word(body, 4)? as usize;
    if header < 8 || header > body.len() || count > (body.len() - header) / stride {
        return Err("signature elements exceed chunk bounds".into());
    }
    let names_start = header + count * stride;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = header + i * stride + name_field;
        let name_off = word(body, e)? as usize;
        let tail = body
            .get(name_off..)
            .filter(|_| name_off >= names_start)
            .ok_or("signature name offset out of bounds")?;
        let end = tail
            .iter()
            .position(|&c| c == 0)
            .ok_or("unterminated signature name")?;
        let name = std::str::from_utf8(&tail[..end]).map_err(|_| "invalid signature name")?;
        if name.is_empty() {
            return Err("empty signature name".into());
        }
        out.push(SigElement {
            name: name.to_owned(),
            index: word(body, e + 4)?,
            sysval: word(body, e + 8)?,
            comp_type: word(body, e + 12)?,
            register: word(body, e + 16)?,
            mask: body[e + 20],
            rw_mask: body[e + 21],
        });
    }
    Ok(out)
}

fn signature(code: &[u8], tags: &[&[u8; 4]]) -> Option<Vec<SigElement>> {
    let container = crate::Dxbc::parse(code).ok()?;
    let chunk = tags.iter().find_map(|tag| container.chunk(tag))?;
    // SM4/5 records have six words. OSG5 adds a leading stream index,
    // and the SM6 signature format also carries a trailing minimum precision.
    let (stride, name_field) = match &chunk.fourcc {
        b"ISGN" | b"OSGN" => (24, 0),
        b"OSG5" => (28, 4),
        _ => (32, 4),
    };
    read_signature(
        &code[chunk.offset..chunk.offset + chunk.size],
        stride,
        name_field,
    )
    .ok()
}

/// Physical register and component order from the compiled pixel input signature.
pub fn pixel_input_order(code: &[u8]) -> Vec<String> {
    let mut inputs = signature(code, &[b"ISG1", b"ISGN"]).unwrap_or_default();
    inputs.sort_by_key(|e| (e.register, e.mask.trailing_zeros()));
    inputs
        .into_iter()
        .map(|e| format!("{}{}", e.name, e.index))
        .collect()
}

/// Only recompile the producer when a shared semantic has different physical packing.
/// Extra producer outputs and rasterizer-generated pixel inputs can legitimately be
/// absent from the opposite stage. D3D12 still validates the complete PSO.
pub fn has_packing_mismatch(producer: &[u8], pixel: &[u8]) -> bool {
    let Some(outputs) = signature(producer, &[b"OSG1", b"OSG5", b"OSGN"]) else {
        return false;
    };
    let Some(inputs) = signature(pixel, &[b"ISG1", b"ISGN"]) else {
        return false;
    };
    inputs.iter().any(|input| {
        outputs.iter().any(|out| {
            input.index == out.index
                && input.name.eq_ignore_ascii_case(&out.name)
                && (input.register != out.register || input.mask & !out.mask != 0)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature_container(tag: &[u8; 4], stride: usize, register: u32, mask: u8) -> Vec<u8> {
        let mut data = vec![0; 44 + 8 + stride];
        data[0..4].copy_from_slice(b"DXBC");
        data[28..32].copy_from_slice(&1u32.to_le_bytes());
        data[32..36].copy_from_slice(&36u32.to_le_bytes());
        data[36..40].copy_from_slice(tag);
        data[44..48].copy_from_slice(&1u32.to_le_bytes());
        data[48..52].copy_from_slice(&8u32.to_le_bytes());
        let name_field = if stride == 24 { 52 } else { 56 };
        data[name_field..name_field + 4].copy_from_slice(&((stride + 8) as u32).to_le_bytes());
        data[name_field + 16..name_field + 20].copy_from_slice(&register.to_le_bytes());
        data[name_field + 20] = mask;
        data.extend_from_slice(b"TEXCOORD\0");
        let len = data.len() as u32;
        data[24..28].copy_from_slice(&len.to_le_bytes());
        data[40..44].copy_from_slice(&(len - 44).to_le_bytes());
        data
    }

    #[test]
    fn checks_legacy_and_modern_signature_packing() {
        let pixel = signature_container(b"ISGN", 24, 3, 2);
        assert_eq!(pixel_input_order(&pixel), ["TEXCOORD0"]);
        for (tag, stride) in [(b"OSGN", 24), (b"OSG5", 28), (b"OSG1", 32)] {
            let matching = signature_container(tag, stride, 3, 3);
            assert!(!has_packing_mismatch(&matching, &pixel));
            assert!(has_packing_mismatch(
                &signature_container(tag, stride, 2, 3),
                &pixel
            ));
            assert!(has_packing_mismatch(
                &signature_container(tag, stride, 3, 1),
                &pixel
            ));
        }
    }

    #[test]
    fn chunk_directory_and_payload_must_fit_declared_container() {
        let valid = signature_container(b"ISG1", 32, 0, 1);
        let mut padded = valid.clone();
        padded.extend_from_slice(&[0xff; 16]);
        assert_eq!(
            Dxbc::parse(&padded).unwrap().total_size as usize,
            valid.len()
        );
        for (offset, value) in [
            (24, 24),
            (24, 32),
            (24, valid.len() as u32 - 1),
            (24, u32::MAX),
            (28, u32::MAX),
            (32, 28),
            (32, u32::MAX),
            (40, u32::MAX),
        ] {
            let mut bad = valid.clone();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(Dxbc::parse(&bad).is_err(), "offset {offset}, value {value}");
        }
    }

    #[test]
    fn rejects_truncated_records_and_invalid_names() {
        let shader = signature_container(b"ISG1", 32, 0, 1);
        let body = &shader[44..];
        assert_eq!(parse_signature(body).unwrap()[0].name, "TEXCOORD");
        for (offset, value) in [
            (0, u32::MAX),
            (4, 4),
            (4, u32::MAX),
            (12, 8),
            (12, u32::MAX),
        ] {
            let mut bad = body.to_vec();
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                parse_signature(&bad).is_err(),
                "offset {offset}, value {value}"
            );
        }
        assert!(parse_signature(&body[..body.len() - 1]).is_err());
        assert!(parse_signature(&body[..39]).is_err());
    }
}
