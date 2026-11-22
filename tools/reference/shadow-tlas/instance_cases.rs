//! Independent CPU fixture for the GPU instance encoder.
#[path = "../blas/shadow_instance.rs"]
mod shadow_instance;
use shadow_instance::{Blas, Bounds, Instance, build};

fn main() {
    let cases = [
        (
            [[1., 0., 0., 4.], [0., 1., 0., -2.], [0., 0., 1., 7.]],
            0xff001234,
            0x12345600,
        ),
        (
            [[0., -2., 0.5, 512.], [0.5, 0., 0.25, 0.], [0., 0., 1.5, 0.]],
            0xff000008,
            0x12345600,
        ),
        (
            [[2., 1., 0., 1.], [0., 3., 0., 2.], [0., 0., 4., 3.]],
            0x10000001,
            0x12345600,
        ),
        (
            [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            0x00000009,
            0x12345600,
        ),
        (
            [[1., 0., 0., 0.], [0., 0., 0., 0.], [0., 0., 1., 0.]],
            0xff000009,
            0x12345600,
        ),
        (
            [[-1., 0., 0., 100.], [0., 1., 0., -200.], [0., 0., 1., 300.]],
            0xff00000a,
            0x12345600,
        ),
        (
            [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            0xff00000b,
            0,
        ),
        (
            [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            0xff00000c,
            0x12345601,
        ),
        (
            [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            0xff00000d,
            0x10000000000,
        ),
        (
            [[1., 2., 3., -11.], [0., 4., 5., 13.], [0., 0., 6., -17.]],
            0x8000000e,
            0x12345600,
        ),
    ];
    let blas = Blas {
        bounds: Bounds {
            min: [-1.; 3],
            max: [1.; 3],
        },
        root: 0xa94,
        compression: 0x007f7f7f,
    };
    for (index, (transform, id_and_mask, address)) in cases.into_iter().enumerate() {
        let instance = Instance {
            transform,
            id_and_mask,
            contribution_and_flags: 0x01000002,
            blas_address: address,
        };
        let mut input = [0u32; 16];
        for (dst, f) in input[..12].iter_mut().zip(transform.into_iter().flatten()) {
            *dst = f.to_bits();
        }
        input[12] = id_and_mask;
        input[13] = instance.contribution_and_flags;
        input[14] = address as u32;
        input[15] = (address >> 32) as u32;
        let (status, output) = match build(instance, blas, index as u32, 0x33) {
            Ok(Some(value)) => (1, value.words),
            Ok(None) => (0, [0; 32]),
            Err(_) => (2, [0; 32]),
        };
        for n in input {
            print!("{n:08x} ");
        }
        print!("{status:08x} ");
        for n in output {
            print!("{n:08x} ");
        }
        println!();
    }
}
