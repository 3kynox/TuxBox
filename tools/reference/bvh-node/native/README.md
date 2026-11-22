# Native RDNA2 node control

This optional Linux control runs the `image_bvh64_intersect_ray` instruction
on a gfx1036 Radeon using HSA. It checks two equal-distance box tests (BOX32
and BOX16) and uncompressed triangle tests including both windings, misses,
shared edges, zero ray distance, and determinants of magnitude four. The first
box lane misses; the other three hit at the same distance. Expected result
words are checked in the host program. These are synthetic inputs; passing this control does not
establish general opcode equivalence or game compatibility.

On gfx1036 the ordinary misses in this fixture preserve the signed determinant
and return infinity with the same sign. The control checks all six permutations
of three triangles against both Z-axis ray directions at their three edge
midpoints and three vertices (216 cases), 108 oblique-ray edge cases and 108
edge cases with tied dominant ray directions. A seeded set of 48 finite
BOX16/BOX32 inputs, including fractional origins and inverse directions, is
compared with CPU box calculations. The control also
records nine parallel or degenerate triangle cases and five zero, subnormal,
or infinite ray cases. Twenty additional box tests cover both signs of an
infinite inverse direction at slab interiors, boundaries and exterior points.
Another 90 cases exercise descriptor box growth against one- and two-step
floating-point gaps at five positive distances. The scalar helper still only
supports zero growth; other growth behavior needs broader validation.
The scalar shader matches the tested parallel-slab and edge ownership
and zero-determinant results; general special-value behavior and numerical
equivalence remain unverified. The control checks all three observed barycentric selectors
on each paired triangle type using unequal barycentrics. These cases do not
establish general instruction equivalence.

Requirements are an accessible `/dev/kfd`, an HSA runtime and its headers,
a clang with the AMDGPU target, and `ld.lld`. Compile and run from the project
root, using the actual HSA installation prefix in `HSA_ROOT`:

```sh
HSA_ROOT=/opt/rocm
clang -target amdgcn-amd-amdhsa -mcpu=gfx1036 -nogpulib -x cl -cl-std=CL2.0 -O2 -c tools/reference/bvh-node/native/probe.cl -o /tmp/tuxbox-bvh-probe.o
ld.lld -shared /tmp/tuxbox-bvh-probe.o -o /tmp/tuxbox-bvh-probe.hsaco
clang -I"$HSA_ROOT/include" tools/reference/bvh-node/native/probe.c -L"$HSA_ROOT/lib" -lhsa-runtime64 -lrocprofiler-register -o /tmp/tuxbox-bvh-probe
LD_LIBRARY_PATH="$HSA_ROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/tuxbox-bvh-probe /tmp/tuxbox-bvh-probe.hsaco
```

To compare those same 48 seeded node inputs and native instruction outputs
directly with the standalone HLSL helper, generate a local synthetic dump and
pass it to the differential tool. The tool requires the project's normal
DXC and Windows guest build dependencies plus test Proton prefixes for each
adapter; no game files are needed.

```sh
LD_LIBRARY_PATH="$HSA_ROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" /tmp/tuxbox-bvh-probe /tmp/tuxbox-bvh-probe.hsaco --dump /tmp/tuxbox-bvh-cases.bin
python tools/check_native_bvh_hlsl.py --dump /tmp/tuxbox-bvh-cases.bin --proton /path/to/proton --prefix-amd /path/to/amd-prefix --prefix-nvidia /path/to/nvidia-prefix --amd-vulkan /path/to/radv.json --output /tmp/tuxbox-bvh-diff.json
```

`--amd-vulkan` is optional when the default Vulkan driver exposes Radeon.
The result checks these cases only and does not establish full traversal or
instruction equivalence.

For six paired-triangle cases with unequal barycentrics, generate the dump
with `--dump-triangles /tmp/tuxbox-bvh-triangles.bin` instead. Pass that file
to the same differential tool with `--dump`.

To test the root node of a user-owned serialized Xbox BLAS, run the probe with
`--retail /path/to/serialized.bin /tmp/tuxbox-bvh-root.bin` and pass the output
to the same differential tool. This checks eight rays against one BOX16 or
BOX32 root; it validates the root's range in the file, not the full graph or
its safety for traversal. The generated dump contains game-owned node bytes;
keep it local.

The descriptor encodes a zero base, full 42-bit range, closest-first sorting
and barycentric triangle return mode. Except for the dedicated growth cases,
box growth is zero. The HSA host
checks that its GPU is gfx1036 and that the allocation meets the node alignment
before dispatch. The result is specific to the chosen inputs and GPU. Compare
with `tools/check_frosty_bvh_nodes.py`, whose corresponding synthetic cases
also run through D3D12 on Radeon and GeForce.

To inspect immediate BVH descriptor setup from a user-supplied cooked shader,
run the standalone Rust inspector from the project root. It recognises the
verified SGPR 68–71 setup and rejects an unsupported write or branch between
setup and use; it does not inspect dynamically generated descriptors:

```sh
cargo build --manifest-path tools/shader-compiler/Cargo.toml --target-dir /tmp/tuxbox-inspect-build --target x86_64-unknown-linux-gnu --lib --no-default-features
rustc --edition=2021 tools/reference/bvh-node/inspect_shader.rs --extern tuxbox_xshader="$(find /tmp/tuxbox-inspect-build/x86_64-unknown-linux-gnu/debug/deps -maxdepth 1 -name 'libtuxbox_xshader-*.rlib' -print -quit)" -L dependency=/tmp/tuxbox-inspect-build/x86_64-unknown-linux-gnu/debug/deps -o /tmp/tuxbox-inspect-shader
/tmp/tuxbox-inspect-shader /path/to/shader.dxbc
```

For the examined Frosty shader 219, both native intersection instructions
construct descriptors with zero box growth and sorting enabled. See
`evidence/matrix-awakens/bvh-descriptor.json` for the input fingerprint.
