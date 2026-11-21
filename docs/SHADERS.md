# Frosty shader coverage

The progress map shows which shader stages TuxBox can translate and which programs Frosty requests while creating pipelines. The reviewed Xbox build contains 4,576 global entries and 38,831 material entries. A single native program can appear in several entries and material shader maps. [Progress map](progress.svg) · [Shader inventory](../evidence/matrix-awakens/shader-inventory.json)

| Column | What it measures |
| --- | --- |
| Packaged | Entries in the two shader archives shipped with this build. |
| HLSL / unique DXBC | Distinct non-ray programs for which the checkout compiler emitted HLSL. Desktop compilation, GPU results and visible rendering each need their own check. |
| In profile | Distinct native programs listed in the launcher's menu and opening cinematic preparation profile. The linked vertex/geometry variants are counted separately. |
| In profile / requested | Profiled programs among the distinct hashes recorded by TuxBox during the sampled runs. Other scenes and graphics settings can request different shaders. |

The green code on the camera plane uses material shader map **136**. The verified main-menu trace includes its vertex shader at material entry 2 and pixel shader at entry 3530. The post-process filter uses map **66**, including pixel shader 1461. The inventory lists each map member, its archive index, stage and the runs that requested it. A request identifies a program used to create a pipeline. Checking a draw or a captured frame establishes whether that pipeline produced visible output. The packaged ray-tracing entries use Xbox serialized state objects and have their own translation path. [Rendered menu](../evidence/matrix-awakens/green-code-menu.webp) · [Cooked material references](../evidence/matrix-awakens/menu-materials.json) · [Ray-state samples](../evidence/matrix-awakens/retail-shaders.json)

All 19,634 distinct vertex shaders now translate to HLSL. GPU readbacks check the cubemap shadow shader’s vertex outputs and the compute instructions used by 94 newly translated programs. Across twelve traces covering startup, the menu, the city chase and Nanite, 1,063 of 1,064 requested shaders pass standalone DXIL validation. The remaining shader is Nanite’s `MicropolyRasterize` at global entry 3246. The first-boot profile includes 169 of the requested programs. [Cubemap shadow readback](../evidence/matrix-awakens/cubemap-shadow-vertex.json) · [Clip-distance outputs](../evidence/matrix-awakens/clip-distance-vertex.json) · [Compute instruction checks](../evidence/matrix-awakens/compute-instructions.json)

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

Four of the 76 packaged `InstanceCull` variants pass GPU readback on both NVIDIA and Radeon. Across 34 dispatches per adapter, all 11,424 buffer words and 2,754 visibility texels matched. The checks include an empty queue, instances on either side of lane 32, a full queue, and two instances competing for the same pixel at the same depth. The remaining 72 variants use instruction sequences the translator still rejects. [NVIDIA results](../evidence/matrix-awakens/instance-cull-nvidia.json) · [Radeon results](../evidence/matrix-awakens/instance-cull-amd.json)

```sh
python build.py d3d12
python tools/check_frosty_instance_cull.py /path/to/Mount --adapter 10de:2705 \
    --output build/instance-cull.json
```

## Nanite cluster culling

After instance culling, `PersistentClusterCull` walks the scene hierarchy, requests missing mesh data and builds the lists of visible clusters for hardware and software rasterization. TuxBox keeps the Xbox workgroup’s 64 lanes together during queue updates, including on GPUs that execute 32 lanes at a time. The translator handles the lane shuffles, shared memory and atomic updates used by these kernels. [AMD RDNA 2 instruction reference](https://docs.amd.com/api/khub/documents/Et~wpu9g~Ffl7d9q0QZ~Og/content)

Four of the 32 packaged variants pass DXIL validation and GPU readback on both Radeon and NVIDIA. Each GPU ran 58 dispatches and matched all 23,200 buffer words. The checks cover nested hierarchy traversal, clipping in two camera views, streaming requests and draw counts for both rasterizers. The remaining 28 variants use other resource layouts and sample textures. TuxBox prepares global entry 1825 before launching Frosty. [Retail run and family census](../evidence/matrix-awakens/persistent-cluster-retail.json) · [Radeon results](../evidence/matrix-awakens/persistent-cull-amd.json) · [NVIDIA results](../evidence/matrix-awakens/persistent-cull-nvidia.json)

```sh
python tools/check_frosty_persistent_cull.py /path/to/Mount --adapter 10de:2705 \
    --output build/persistent-cull.json
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

The CSV contains one row for every shipped entry. It records the Unreal shader ID, native DXBC SHA-256 when present, map memberships, preparation status and trace labels. To repeat the frontend survey, replace `--reuse-survey` with `--compiler build/shader-compiler/x86_64-unknown-linux-gnu/release/xshader` after building the Linux compiler. This also fills in the per-entry `hlsl_emitted` CSV column. Add `--dxc /path/to/dxc` to compile the distinct observed programs and check their output with the adjacent `dxv` executable. The CSV records those results under `standalone_dxil_validated`. Reusing a survey carries forward the aggregate results for matching archive hashes and the measured results for known requests.

To publish updated counts, run the mapper with all the trace labels you want to cover, use `evidence/matrix-awakens/shader-inventory.json` as the output, then regenerate the SVG with `python tuxbox.py status --svg docs/progress.svg`.
