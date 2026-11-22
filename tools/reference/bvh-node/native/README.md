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
midpoints and three vertices (216 cases), and checks 108 oblique-ray edge
cases. A seeded set of 24 finite BOX16/BOX32 inputs is compared with CPU box
calculations. The scalar shader matches the tested unique-dominant-axis edge
ownership; tied axes, special values, and general numerical equivalence remain
unverified. The control also checks all three observed barycentric selectors
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

The descriptor encodes a zero base, full 42-bit range, zero box growth,
closest-first sorting and barycentric triangle return mode. The HSA host
checks that its GPU is gfx1036 and that the allocation meets the node alignment
before dispatch. The result is specific to the chosen inputs and GPU. Compare
with `tools/check_frosty_bvh_nodes.py`, whose corresponding synthetic cases
also run through D3D12 on Radeon and GeForce.
