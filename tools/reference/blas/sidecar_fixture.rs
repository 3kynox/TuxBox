//! Generate the synthetic leaf bytes and directory using the runtime encoder.
#[path = "../../../runtime/d3d12/src/device/blas/sidecar_directory.rs"]
mod directory;
use directory::{Candidate, InstanceRange, encode};
use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os().nth(1).ok_or("missing output directory")?;
    let path = std::path::Path::new(&path);
    let vertices: [[f32; 12]; 2] = [
        [0., 0., 0., 1., 0., 0., 0., 1., 0., 1., 1., 0.],
        [2., 0., 0., 3., 0., 0., 2., 1., 0., 3., 1., 0.],
    ];
    let mut leaves = vec![0u8; 128];
    for (i, triangle) in vertices.iter().enumerate() {
        for (j, value) in triangle.iter().enumerate() {
            leaves[i * 64 + j * 4..i * 64 + j * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        leaves[i * 64 + 56..i * 64 + 60].copy_from_slice(&u32::MAX.to_le_bytes());
        leaves[i * 64 + 60..i * 64 + 64].copy_from_slice(&9u32.to_le_bytes());
    }
    let candidates = [
        Candidate {
            leaf_offset: 0,
            side: 0,
        },
        Candidate {
            leaf_offset: 64,
            side: 0,
        },
    ];
    let instances = [InstanceRange { first: 0, count: 2 }; 3];
    let directory = encode(&leaves, &instances, &candidates).map_err(|e| format!("{e:?}"))?;
    fs::write(path.join("sidecar-leaves.bin"), leaves)?;
    fs::write(path.join("sidecar-directory.bin"), directory)?;
    Ok(())
}
