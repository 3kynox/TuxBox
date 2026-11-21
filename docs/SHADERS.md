# Frosty shader coverage

The progress map shows which shader stages TuxBox can translate and which programs Frosty requests while creating pipelines. The reviewed Xbox build contains 4,576 global entries and 38,831 material entries. A single native program can appear in several entries and material shader maps. [Progress map](progress.svg) · [Shader inventory](../evidence/matrix-awakens/shader-inventory.json)

| Column | What it measures |
| --- | --- |
| Packaged | Entries in the two shader archives shipped with this build. |
| HLSL / unique DXBC | Distinct non-ray programs for which the checkout compiler emitted HLSL. Desktop compilation, GPU results and visible rendering each need their own check. |
| In profile | Distinct native programs listed in the launcher's menu and opening cinematic preparation profile. The linked vertex/geometry variants are counted separately. |
| In profile / requested | Profiled programs among the distinct hashes recorded by TuxBox during the sampled runs. Other scenes and graphics settings can request different shaders. |

The green code on the camera plane uses material shader map **136**. The verified main-menu trace includes its vertex shader at material entry 2 and pixel shader at entry 3530. The post-process filter uses map **66**, including pixel shader 1461. The inventory lists each map member, its archive index, stage and the runs that requested it. A request identifies a program used to create a pipeline. Checking a draw or a captured frame establishes whether that pipeline produced visible output. The packaged ray-tracing entries use Xbox serialized state objects and have their own translation path. [Rendered menu](../evidence/matrix-awakens/green-code-menu.webp) · [Cooked material references](../evidence/matrix-awakens/menu-materials.json) · [Ray-state samples](../evidence/matrix-awakens/retail-shaders.json)

All 19,634 distinct vertex shaders now translate to HLSL. GPU readbacks check the cubemap shadow shader’s vertex outputs and the compute instructions used by 94 newly translated programs. Across seven traces of startup, the menu and Start Chase, all 349 requested shaders pass standalone DXIL validation. The first-boot profile includes 160 of them. [Cubemap shadow readback](../evidence/matrix-awakens/cubemap-shadow-vertex.json) · [Clip-distance outputs](../evidence/matrix-awakens/clip-distance-vertex.json) · [Compute instruction checks](../evidence/matrix-awakens/compute-instructions.json)

## View or update the map

Run `python tuxbox.py status --shaders` to see the recorded counts and the hashes of requested programs outside the preparation profile. The JSON inventory includes each requested hash, stage, matching archive entries and observed runs. A standalone DXIL pass means the translator emitted HLSL for that individual shader, Microsoft DXC compiled it, and the DXIL validator accepted the result. Linked stages and the rendered frame still need GPU checks.

To record a new observation, set `TUXBOX_TRACE_SHADERS=1` and `TUXBOX_LOG=1`, then launch Frosty in the foreground with `tools/run_frosty.sh`. Pass its saved `steam-default.log` below. Each trace label describes the entire run from process startup. The current trace identifies the stripped vertex, pixel, geometry and compute programs that enter TuxBox’s shader cache lookup. The inventory labels older logs that record only the first hit of each stage and a limited number of misses.

```sh
python tools/map_frosty_shaders.py /path/to/Mount \
  --trace "Menu=/path/to/steam-default.log" \
  --reuse-survey evidence/matrix-awakens/shader-inventory.json \
  --index build/frosty-shaders.csv --output build/frosty-shaders.json
```

The CSV contains one row for every shipped entry. It records the Unreal shader ID, native DXBC SHA-256 when present, map memberships, preparation status and trace labels. To repeat the frontend survey, replace `--reuse-survey` with `--compiler build/shader-compiler/x86_64-unknown-linux-gnu/release/xshader` after building the Linux compiler. This also fills in the per-entry `hlsl_emitted` CSV column. Add `--dxc /path/to/dxc` to compile the distinct observed programs and check their output with the adjacent `dxv` executable. The CSV records those results under `standalone_dxil_validated`. Reusing a survey carries forward the aggregate results for matching archive hashes and the measured results for known requests.

To publish updated counts, run the mapper with all the trace labels you want to cover, use `evidence/matrix-awakens/shader-inventory.json` as the output, then regenerate the SVG with `python tuxbox.py status --svg docs/progress.svg`.
