//! Submit unprofiled console shaders to the running launcher's compiler.
use super::{Stage, read_portable_file};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

fn session() -> Option<&'static PathBuf> {
    static SESSION: OnceLock<Option<PathBuf>> = OnceLock::new();
    SESSION
        .get_or_init(|| {
            let name = std::env::var("TUXBOX_SHADER_SESSION").ok()?;
            if !name.starts_with("run-")
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            {
                return None;
            }
            Some(std::env::temp_dir().join("tuxbox_shader_cache").join(name))
        })
        .as_ref()
}

pub(super) fn on_demand(shader: &[u8], source: &str, stage: Stage) -> Option<Vec<u8>> {
    let dir = session()?;
    let result = dir.join(format!("{source}.cso"));
    if let Some(bytes) = read_portable_file(&result, stage) {
        return Some(bytes);
    }
    // Pipeline workers can request the same program concurrently. Serialize
    // publication for the same digest while allowing unrelated jobs to overlap.
    static LOCKS: [Mutex<()>; 32] = [const { Mutex::new(()) }; 32];
    let bucket = usize::from_str_radix(&source[..2], 16).ok()? % LOCKS.len();
    let _guard = LOCKS[bucket]
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(bytes) = read_portable_file(&result, stage) {
        return Some(bytes);
    }
    let failed = dir.join(format!("{source}.failed"));
    if failed.exists() {
        return None;
    }
    let pending = dir.join(format!("{source}.{}.tmp", std::process::id()));
    fs::write(&pending, shader).ok()?;
    fs::rename(pending, dir.join("requests").join(format!("{source}.dxbc"))).ok()?;
    let start = Instant::now();
    loop {
        if let Some(bytes) = read_portable_file(&result, stage) {
            return Some(bytes);
        }
        if failed.exists() || start.elapsed() >= Duration::from_secs(90) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
