# Frosty shader coverage

The progress map shows which shader stages TuxBox can translate and which programs Frosty requests while creating pipelines. The reviewed Xbox build contains 4,576 global entries and 38,831 material entries. A single native program can appear in several entries and material shader maps. [Progress map](progress.svg) · [Shader inventory](../evidence/matrix-awakens/shader-inventory.json)

| Column | What it measures |
| --- | --- |
| Packaged | Entries in the two shader archives shipped with this build. |
| HLSL / unique DXBC | Distinct non-ray programs for which the checkout compiler emitted HLSL. Desktop compilation, GPU results and visible rendering each need their own check. |
| TuxBox only | Distinct programs that the independent compiler emitted with legacy translation disabled. The survey compares its HLSL and profile with the regular build. |
| In profile | Distinct native programs listed in the launcher's menu and opening cinematic preparation profile. The linked vertex/geometry variants are counted separately. |
| In profile / requested | Profiled programs among the distinct hashes recorded by TuxBox during the sampled runs. Other scenes and graphics settings can request different shaders. |

The green code on the camera plane uses material shader map **136**. The verified main-menu trace includes its vertex shader at material entry 2 and pixel shader at entry 3530. The post-process filter uses map **66**, including pixel shader 1461. The inventory lists each map member, its archive index, stage and the runs that requested it. A request identifies a program used to create a pipeline. Checking a draw or a captured frame establishes whether that pipeline produced visible output. The packaged ray-tracing entries use Xbox serialized state objects and have their own translation path. [Rendered menu](../evidence/matrix-awakens/green-code-menu.webp) · [Cooked material references](../evidence/matrix-awakens/menu-materials.json) · [Ray-state samples](../evidence/matrix-awakens/retail-shaders.json)

All 19,634 distinct vertex shaders now translate to HLSL. GPU readbacks check the cubemap shadow shader’s vertex outputs and the compute instructions used by 94 newly translated programs. Across 23 traces covering startup, Nanite, the chase on stock Proton and the fork, and city free roam, all 2,433 observed programs passed standalone DXIL validation. The default first-boot profile includes 177 of those programs. The larger chase profile also prepares shaders that compiled on demand during play. [Cubemap shadow readback](../evidence/matrix-awakens/cubemap-shadow-vertex.json) · [Clip-distance outputs](../evidence/matrix-awakens/clip-distance-vertex.json) · [Compute instruction checks](../evidence/matrix-awakens/compute-instructions.json)

## Independent translation

TuxBox’s independent Rust compiler translates 99 of Frosty’s 12,271 distinct pixel shaders and seven of its 3,233 compute shaders. The pixel shaders write constant colors to one or more targets, preserve rasterized depth, or discard every sample. Three compute programs initialize specific words in structured buffers. The other four perform empty dispatches. All 106 passed readback checks on GeForce and Radeon, including untouched pixels and buffer words. The gameplay compiler still uses TomBox-derived HLSL generation for the rest of Frosty’s supported programs. [GPU results](../evidence/matrix-awakens/independent-shader-gpu.json) · [Compute instructions and strides](../evidence/matrix-awakens/independent-compute.json) · [Source attribution](REFERENCES.md#shader-compiler-attribution)

A cold rebuild validated all 223 prepared entries and linked variants. Four pixel programs and two linked vertex programs changed. The two updated pairs created depth-only pipelines with their original root signatures on both GPUs. [Prepared shader comparison](../evidence/matrix-awakens/independent-shader-profile.json)

Build the independent compiler into a separate directory to test a raw Xbox shader. It reports an error when a program needs an unsupported operation. This command compiles the supplied bytes directly and does not read the game’s shader cache.

```sh
cargo build --locked --release --no-default-features \
    --manifest-path tools/shader-compiler/Cargo.toml --target x86_64-unknown-linux-gnu \
    --target-dir build/independent-compiler
build/independent-compiler/x86_64-unknown-linux-gnu/release/xshader \
    shader.dxbc --hlsl-out shader.hlsl --profile-out shader.profile
```

## Vertex fetch

Three of Frosty’s vertex shaders give us expected positions and texture coordinates directly from their Xbox instructions. Global entry 1007 copies the input position and sets W to one. Entry 2450 passes through the full position and a pair of texture coordinates. Entry 2751 scales and flips coordinates into clip space. On stock Proton, the current gameplay compiler produced the expected bytes for all three with separate buffers and with interleaved attributes, on GeForce and Radeon. Deliberately swapping the two texture coordinates failed both layouts for entry 2450. This is a baseline for replacing the legacy vertex emitter. [Readbacks and reproduction](../evidence/matrix-awakens/vertex-fetch.json)

Frosty’s menu material at entry 2 illustrates the overlap with AnyPS5’s instruction profile. LLVM 23.1.5 decodes the shader’s call at byte offset `0x18` as `s_swappc_b64 s[4:5], s[4:5]` on both `gfx1013` and `gfx1030`. At `0x1cc`, the former recognizes `v_mad_f32` and the latter rejects the instruction encoding. TuxBox uses AMD’s GFX10.1 and GFX10.3 tables together. The Xbox shader signature and input layout supply the evidence for how a game binds its vertex data. [Menu material inventory](../evidence/matrix-awakens/menu-materials.json) · [Opcode table](../tools/shader-compiler/src/rdna2/opcodes.rs) · [AnyPS5 research notes](REFERENCES.md)

## Street materials and scene instances

Both the earlier run and the Proton fork run with the independent emitters show missing character surfaces in the rooftop scene. The standing character’s head and torso are absent in both captures. [Earlier rooftop frame](../evidence/matrix-awakens/rooftop-before.jpg) · [Current rooftop frame](../evidence/matrix-awakens/rooftop-independent.jpg)

Frosty's pixel shaders read triangle facing and fragment position from registers selected by the Xbox shader. TuxBox now supplies the front-face flag and packs the enabled position components in hardware order. On Radeon and NVIDIA, the same two-sided material produced the expected light front and dark back. A second material produced the expected depth values at both test distances. All 667 affected pixel programs requested during the recorded chase and city run passed DXIL validation. [Pixel shader GPU readbacks](../evidence/matrix-awakens/pixel-raster-inputs.json)

Material and compute shaders also need the byte distance between records in a structured buffer. TuxBox reads that distance from the Xbox binding metadata where available. In a GPU test, Frosty's scene-update shader previously wrote 64 of 128 primitive IDs and overwrote 64 adjacent IDs. It now writes all 128 requested IDs and preserves all 128 adjacent IDs. [Scene-update GPU readback](../evidence/matrix-awakens/scene-primitive-ids.json)

## Mesh shaders

All 192 mesh programs in Frosty’s global archive are permutations of `HWRasterizeMS`. TuxBox translates their Xbox instructions and output declarations to Shader Model 6.5. The original workgroup has 256 logical lanes. Each desktop workgroup uses 128 threads and processes the extra vertices in a second pass. TuxBox also reconstructs the packed triangle indices, per-primitive attributes and culling decisions. [Mesh translator](../tools/shader-compiler/src/hlsl/mesh.rs) · [DirectX mesh shader specification](https://microsoft.github.io/DirectX-Specs/d3d/MeshShader.html)

Both the RTX 4070 Ti SUPER and the Radeon integrated GPU created pipelines for all 192 validated programs. Separate paired draws exercise rasterization with original pixel shaders, including a triangle that references vertices 192–194. The graphics tests also cover direct and indirect Xbox dispatch and a portable amplification shader. Frosty’s archive contains no amplification programs. [NVIDIA results](../evidence/matrix-awakens/nanite-mesh-nvidia.json) · [Radeon results](../evidence/matrix-awakens/nanite-mesh-amd.json) · [Command-list checks](../evidence/matrix-awakens/mesh-command-list-nvidia.json)

To repeat the retail shader tests on a mesh-capable adapter, use its PCI vendor and device IDs.

```sh
python build.py d3d12
python tools/check_frosty_nanite_mesh.py /path/to/Mount --adapter 10de:2705 \
    --all-mesh --output build/nanite-mesh.json
```

## Nanite instance culling

Frosty calls `InstanceCull` to decide which scene instances enter Nanite’s work queue. Its 64-lane Xbox workgroup counts visible instances, reserves space in the queue, and writes each instance ID in lane order. TuxBox also handles the path that writes small projected instances directly into the visibility texture. Frosty’s first-boot profile includes the variant requested by the title, global entry 3444. [Retail comparison and list of variants](../evidence/matrix-awakens/instance-cull-retail.json)

Twelve of the 76 packaged `InstanceCull` variants pass GPU readback on both NVIDIA and Radeon. Each GPU ran 149 dispatches and matched 89,838 buffer words and 12,069 visibility texels. The checks cover ordered per-view queues, capacity limits, impostor depth and occlusion against a four-level depth pyramid. Paired controls confirm that selecting mip 1 changes which instances survive. Two second-pass variants recheck previously hidden instances against the current depth and append the survivors to the three-word node queue. [NVIDIA results](../evidence/matrix-awakens/instance-cull-nvidia.json) · [Radeon results](../evidence/matrix-awakens/instance-cull-amd.json)

```sh
python build.py d3d12
python tools/check_frosty_instance_cull.py /path/to/Mount --adapter 10de:2705 \
    --output build/instance-cull.json
```

## Nanite cluster culling

After instance culling, `PersistentClusterCull` walks the scene hierarchy, requests missing mesh data and builds the lists of visible clusters for hardware and software rasterization. TuxBox keeps the Xbox workgroup’s 64 lanes together during queue updates, including on GPUs that execute 32 lanes at a time. The translator handles the lane shuffles, shared memory and atomic updates used by these kernels. [AMD RDNA 2 instruction reference](https://docs.amd.com/api/khub/documents/Et~wpu9g~Ffl7d9q0QZ~Og/content)

Five of the 32 packaged variants pass DXIL validation and GPU readback on both Radeon and NVIDIA. Each GPU ran 86 dispatches and matched all 45,062 buffer words. The checks cover nested hierarchy traversal, streaming requests, camera selection and draw counts for both rasterizers. Entry 1827 uses the previous frame’s instance transforms and depth pyramid to defer hidden geometry. Its controls check which clusters survive at two mip levels, the records written across a 64-cluster batch boundary and the child masks saved for the next pass. [Retail run and family census](../evidence/matrix-awakens/persistent-cluster-retail.json) · [Radeon results](../evidence/matrix-awakens/persistent-cull-amd.json) · [NVIDIA results](../evidence/matrix-awakens/persistent-cull-nvidia.json)

```sh
python tools/check_frosty_persistent_cull.py /path/to/Mount --adapter 10de:2705 \
    --output build/persistent-cull.json
```

## Nanite micropolygon rasterization

`MicropolyRasterize` draws small triangles from Nanite’s visible-cluster list. The variant Frosty requests, global entry 3246, stores depth and triangle ID together in a 64-bit visibility value. When triangles overlap, one atomic maximum selects the winning pair. TuxBox preserves the Xbox shader’s choice of scanline loop for each 64-lane wave. [Shader Model 6.6 atomic operations](https://microsoft.github.io/DirectX-Specs/d3d/HLSL_SM_6_6_Int64_and_Float_Atomics.html)

All 96 packaged variants passed DXIL validation and created an Xbox compute pipeline on Radeon and NVIDIA. Entry 3246 also matched all 9,269 texels in 13 GPU tests per adapter. These cover narrow and wide triangles, interpolated depth, overlap, and pixels outside the viewport. The retail Radeon run passed this pipeline and reached graphics command-list slot 96. [Retail traces](../evidence/matrix-awakens/micropoly-retail.json) · [Radeon results](../evidence/matrix-awakens/micropoly-amd.json) · [NVIDIA results](../evidence/matrix-awakens/micropoly-nvidia.json)

```sh
python tools/check_frosty_micropoly.py /path/to/Mount --adapter 10de:2705 \
    --all-pipelines --output build/micropoly.json
```

## Nanite material classification

Frosty’s `ClassifyMaterials` shader reads the triangles visible in each 64 × 64 tile and records which materials to draw there. Tests of the packaged shader at global entry 2725 checked its draw arguments and material tile masks on both GPUs. The eight cases cover empty tiles, mixed materials, streamed cluster pages and tiles offset from the origin. [NVIDIA readbacks](../evidence/matrix-awakens/classify-materials-nvidia.json) · [Radeon readbacks](../evidence/matrix-awakens/classify-materials-amd.json)

## Pixel shader stencil export

Frosty’s global pixel shaders 1748 and 1763 read visibility data and write depth and a stencil byte for each surviving pixel. Radeon supports this output directly. On the tested RTX, TuxBox routes the stencil byte to an extra integer render target and copies it back to the depth/stencil texture. This path handles a single-sample texture with full-byte replacement on a passing fragment. In two draws per shader, all 128 depth and stencil pairs matched on each GPU, including pixels discarded by the shader and pixels rejected by the depth test. [NVIDIA readbacks](../evidence/matrix-awakens/stencil-export-nvidia.json) · [Radeon readbacks](../evidence/matrix-awakens/stencil-export-amd.json)

```sh
python tools/check_frosty_stencil.py /path/to/Mount --adapter 10de:2705 \
    --output build/stencil-export.json
```

## Temporal super-resolution

Frosty requested global compute entry 2653 when it loaded the city sandbox. The packaged shader map identifies it as `FTSRFilterFrequenciesCS`, a temporal super-resolution (TSR) filter. Its 256 threads use a value shared within each Xbox wave to select the image coordinates to process. The default first-boot profile now includes this permutation. [First-boot shader list](../tools/reference/frosty-menu-shaders.json)

The packaged shader wrote two color images and a filter weight on both NVIDIA and Radeon. Five GPU tests covered black, uniform and patterned images, swapped color inputs, and a clipped output rectangle. Each GPU returned the same bytes for all 15 output images. The tests check constant colors, local color bounds and untouched pixels outside the rectangle. [NVIDIA readbacks](../evidence/matrix-awakens/tsr-filter-nvidia.json) · [Radeon readbacks](../evidence/matrix-awakens/tsr-filter-amd.json)

The next flyover pass, `FTSRUpdateHistoryCS` at global entry 3385, combines current-frame color and translucency and can generate a lower-resolution mip. TuxBox translates its packed halfword arithmetic and the shared-memory exchanges across its 32-thread group. The default launch now prepares this shader too. On both GPUs the original Xbox dispatch produced identical images across five cases, including a half-transparent overlay and a patterned input. The tests compare the mip against a CPU 2 × 2 average and check the pixels outside the written region. [NVIDIA history tests](../evidence/matrix-awakens/tsr-history-nvidia.json) · [Radeon history tests](../evidence/matrix-awakens/tsr-history-amd.json)

All 34 compute programs in the game’s 14 packaged TSR shader maps passed standalone DXIL validation, including all eight history-update permutations. Eleven were observed during the retail replay that reached entry 3385. [Per-program survey and retail trace](../evidence/matrix-awakens/tsr-maps.json)

The map survey uses either Python’s `cityhash` module or the Abseil development libraries with `pkg-config` and a C++ compiler.

```sh
python tools/check_frosty_tsr_filter.py /path/to/Mount --adapter 10de:2705 \
    --output build/tsr-filter.json
python tools/check_frosty_tsr_filter.py /path/to/Mount --adapter 10de:2705 \
    --update-history --output build/tsr-history.json
python tools/map_frosty_tsr.py /path/to/Mount --output build/tsr-maps.json
```

## View or update the map

Run `python tuxbox.py status --shaders` to see the recorded counts and the hashes of requested programs outside the preparation profile. The JSON inventory includes each requested hash, stage, matching archive entries and observed runs. A standalone DXIL pass means the translator emitted HLSL for that individual shader, Microsoft DXC compiled it, and the DXIL validator accepted the result. Linked stages and the rendered frame still need GPU checks.

To record a new observation, set `TUXBOX_TRACE_SHADERS=1` and `TUXBOX_LOG=1`, then launch Frosty in the foreground with `tools/run_frosty.sh`. Pass its saved `steam-default.log` below. Each trace label describes the entire run from process startup. The current trace identifies the stripped vertex, pixel, geometry, mesh and compute programs that enter TuxBox’s shader cache lookup. The inventory labels older logs that record only the first hit of each stage and a limited number of misses.

```sh
python tools/map_frosty_shaders.py /path/to/Mount \
  --trace "Menu=/path/to/steam-default.log" \
  --reuse-survey evidence/matrix-awakens/shader-inventory.json \
  --index build/frosty-shaders.csv --output build/frosty-shaders.json
```

The CSV contains one row for every shipped entry. It records the Unreal shader ID, native DXBC SHA-256 when present, map memberships, preparation status and trace labels. To repeat the frontend survey, replace `--reuse-survey` with `--compiler build/shader-compiler/x86_64-unknown-linux-gnu/release/xshader` after building the Linux compiler. This also fills in the per-entry `hlsl_emitted` CSV column. Add `--dxc /path/to/dxc` to compile the distinct observed programs and check their output with the adjacent `dxv` executable. The CSV records those results under `standalone_dxil_validated`. Add `--independent-compiler build/independent-compiler/x86_64-unknown-linux-gnu/release/xshader` to compare both compilers on every distinct program and fill the `independent_hlsl` column. With DXC enabled, this also validates all independently emitted programs, including ones that have yet to appear in a trace. Reusing a survey carries forward the aggregate results for matching archive hashes and the measured results for known requests.

To publish updated counts, run the mapper with all the trace labels you want to cover, use `evidence/matrix-awakens/shader-inventory.json` as the output, then regenerate the SVG with `python tuxbox.py status --svg docs/progress.svg`.
