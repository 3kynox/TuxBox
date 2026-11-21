//! Prepared desktop replacements for stripped Xbox shader containers.
//! The host publishes a validated translation under the SHA-256 of all bytes
//! in the original guest shader. Portable input never opens the cache.
use std::collections::HashSet;
mod compile;

use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::sync::{LazyLock, Mutex, Once};
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};

const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Pixel,
    Vertex,
    Geometry,
    Compute,
}

impl Stage {
    fn report(self, source: &str, hit: bool) {
        static PIXEL: Once = Once::new();
        static VERTEX: Once = Once::new();
        static GEOMETRY: Once = Once::new();
        static COMPUTE: Once = Once::new();
        let (name, first_hit) = match self {
            Self::Pixel => ("pixel", &PIXEL),
            Self::Vertex => ("vertex", &VERTEX),
            Self::Geometry => ("geometry", &GEOMETRY),
            Self::Compute => ("compute", &COMPUTE),
        };
        static TRACE: LazyLock<bool> =
            LazyLock::new(|| std::env::var("TUXBOX_TRACE_SHADERS").as_deref() == Ok("1"));
        if *TRACE {
            static SEEN: LazyLock<Mutex<HashSet<String>>> =
                LazyLock::new(|| Mutex::new(HashSet::new()));
            let mut seen = SEEN.lock().unwrap_or_else(|error| error.into_inner());
            if !seen.contains(source) {
                seen.insert(source.to_owned());
                drop(seen);
                crate::diagnostics::log_once(&Once::new(), || {
                    format!("shader request {name} {source}")
                });
            }
        }
        if hit {
            crate::diagnostics::log_once(first_hit, || {
                format!("prepared {name} shader loaded, source SHA-256 {source}")
            });
            return;
        }
        // Parallel pipeline jobs can miss several distinct shaders before the
        // title stops. Record each source once, with bounded process lifetime storage.
        static MISSES: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let mut misses = MISSES.lock().unwrap_or_else(|error| error.into_inner());
        if misses.len() < 64 && !misses.iter().any(|seen| seen == source) {
            misses.push(source.to_owned());
            drop(misses);
            crate::diagnostics::log_once(&Once::new(), || {
                format!("prepared {name} shader missing or invalid, source SHA-256 {source}")
            });
        }
    }
}

#[derive(PartialEq)]
enum ShaderCode {
    Portable,
    Xbox,
}

fn word(data: &[u8], offset: usize) -> Option<usize> {
    Some(u32::from_le_bytes(data.get(offset..offset + 4)?.try_into().ok()?) as usize)
}

fn shader_code(data: &[u8]) -> Option<(Stage, ShaderCode)> {
    if data.len() > MAX_BYTES || !data.starts_with(b"DXBC") || word(data, 24)? != data.len() {
        return None;
    }
    let table_end = 32 + word(data, 28)? * 4;
    let mut program = None;
    for offset in data.get(32..table_end)?.as_chunks::<4>().0 {
        let start = u32::from_le_bytes(*offset) as usize;
        if start < table_end {
            return None;
        }
        let tag = data.get(start..start + 4)?;
        let length = word(data, start + 4)?;
        let body = data.get(start + 8..start + 8 + length)?;
        if tag == b"DXIL" && program.replace(body).is_some() {
            return None;
        }
    }
    let dxil = program?;
    // ProgramVersion encodes the shader stage in its upper half and shader
    // model 6.x in its low byte. BitcodeOffset is relative to DXIL magic.
    let version = word(dxil, 0)?;
    let stage = match version >> 16 {
        0 => Stage::Pixel,
        1 => Stage::Vertex,
        2 => Stage::Geometry,
        5 => Stage::Compute,
        _ => return None,
    };
    if version & 0xf0 != 0x60 || word(dxil, 4)? * 4 != dxil.len() || dxil.get(8..12)? != b"DXIL" {
        return None;
    }
    let offset = 8 + word(dxil, 16)?;
    if offset < 24 {
        return None;
    }
    let bitcode = dxil.get(offset..offset + word(dxil, 20)?)?;
    if bitcode.starts_with(b"BC\xc0\xde") {
        Some((stage, ShaderCode::Portable))
    } else if dxil.get(24..28)? == b"XDAT" && bitcode == b"NOBC" {
        Some((stage, ShaderCode::Xbox))
    } else {
        None
    }
}

fn read_portable(directory: &str, source: &str, stage: Stage) -> Option<Vec<u8>> {
    let path = std::env::temp_dir()
        .join("tuxbox_shader_cache")
        .join(directory)
        .join(format!("{source}.cso"));
    read_portable_file(&path, stage)
}

fn read_portable_file(path: &std::path::Path, stage: Stage) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(metadata.len() as usize).ok()?;
    // A concurrent change must not turn a bounded cache lookup into an
    // unbounded read. The host normally publishes by atomic replacement.
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    (shader_code(&bytes) == Some((stage, ShaderCode::Portable))).then_some(bytes)
}

pub(super) fn source_digest(shader: &[u8]) -> Option<String> {
    if shader.len() > MAX_BYTES {
        return None;
    }
    let mut digest = [0u8; 32];
    // SAFETY: the system SHA-256 pseudo-handle takes no secret, and the
    // bounded input and 32-byte output remain live through this call.
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, shader, &mut digest) }
        .ok()
        .ok()?;
    let mut source = String::with_capacity(64);
    for byte in digest {
        write!(source, "{byte:02x}").ok()?;
    }
    Some(source)
}

pub(super) fn linked_vertex(vertex: &str, pixel: &str) -> Option<Vec<u8>> {
    read_portable("by_vs_ps", &format!("{vertex}_{pixel}"), Stage::Vertex)
}

pub(super) fn linked_geometry(geometry: &str, pixel: &str) -> Option<Vec<u8>> {
    read_portable("by_gs_ps", &format!("{geometry}_{pixel}"), Stage::Geometry)
}

pub(super) fn prepared(shader: &[u8]) -> Option<Vec<u8>> {
    let (stage, ShaderCode::Xbox) = shader_code(shader)? else {
        return None;
    };
    let source = source_digest(shader)?;
    let translation = read_portable("by_source", &source, stage)
        .or_else(|| compile::on_demand(shader, &source, stage));
    stage.report(&source, translation.is_some());
    translation
}
