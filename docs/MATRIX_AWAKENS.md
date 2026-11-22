# The Matrix Awakens

Frosty selects its original XSX graphics profile on stock Proton Experimental. On a GeForce RTX 4070 Ti SUPER it loaded the Construct map, rebuilt the packaged ray-tracing meshes and presented a 3840×2160 frame. The desktop capture showed a black window with MangoHud. Frosty created 136 ray-tracing collections covering 132 distinct shader exports. It linked the 117-collection pipeline for shadows, reflections and materials, then created several Lumen radiance-cache and radiosity collections. After passing LightGridInjectionCS and the shadow-page shaders, the game stopped on an inline Lumen ray-traversal compute shader during scene setup. The menu has not appeared. [Retail trace](../evidence/matrix-awakens/packaged-renderer.json)

TuxBox rebuilt the 528-triangle sphere through the Xbox command list. All 3,171 test rays matched desktop D3D12 on GeForce and Radeon. The Radeon’s compacted structure needed 56,704 bytes, so TuxBox translated the game’s 31,232-byte allocation to a larger host buffer. A street lamp with an empty material section also matched the original mesh, including the geometry indices used to select hit shaders. [Geometry checks](../evidence/matrix-awakens/retail-sphere-blas.json) · [Ray-shader checks](SHADERS.md)

## Earlier tests with reduced graphics settings

Frosty reached the main menu, highway chase and open city on Linux with a TuxBox graphics override. On an RTX 4070 Ti SUPER with the shaders and graphics pipelines cached, the shooting tutorial appeared 4 minutes 4 seconds after selecting **Start Chase**. The game finished the bridge finale and entered Sandbox at 7 minutes 43 seconds. At 11 minutes 2 seconds, the player was walking in the street. The chase accepts target lock and manually fired shots. In the city, a controller and W/S moved the same character forward and back. The right stick and mouse turned the camera in both directions. [Chase HUD](../evidence/matrix-awakens/chase-gameplay-hud.jpg) · [City street](../evidence/matrix-awakens/city-free-roam.jpg) · [Input and timing records](../evidence/matrix-awakens/chase-gameplay.json)

With temporal anti-aliasing on stock Proton, the shooting tutorial appeared 4m37s after selecting **Start Chase**, down from 9m59s in the previous replay. In the first 45 seconds with the controls visible, MangoHud counted 289 frames (6.42 FPS), up from 62 (1.38 FPS). The longest complete frame fell from 9.00 seconds to 741 ms. The new run still spent 37 of those 45 seconds in frames longer than 100 ms, and some road and building surfaces have gaps. Both runs used the same GPU and driver. Their caches and Unreal pipeline logging settings differed. [Frame times and CPU samples](../evidence/matrix-awakens/foreground-chase-grouped.json) · [Tutorial screenshot](../evidence/matrix-awakens/grouped-chase-tutorial.jpg)

The first temporal anti-aliasing run reached the city chase intro in 6m53s. With the driver cache populated and 41 more shaders prepared, an earlier repeat reached it in 2m33s. A CPU sample from that repeat found 710 of 803 samples in Proton’s conversion of shader control flow to SPIR-V. [Earlier desktop runs](../evidence/matrix-awakens/foreground-chase-repeat.json) · [Compiler profile](../evidence/matrix-awakens/chase-warmup.json)

With the corrected street shaders, Frosty’s own Sandbox test reached the on-foot view and exited successfully. Gaps remain in the street and buildings. In a ten-second CPU profile on that street, 168 of 800 samples came from TuxBox comparing the saved bytes of a texture mip level against guest memory copied for the current submission. [Street screenshot](../evidence/matrix-awakens/street-page-table.jpg) · [Texture readbacks and title run](../evidence/matrix-awakens/street-page-table.json) · [CPU samples](../evidence/matrix-awakens/foreground-chase-grouped.json)

The title reports an invalid local user when saving the city unlock. Closing the desktop game window after the controls test produced a viewport error and exit code 1. The game’s built-in Sandbox test also reached the street, accepted walking input and exited successfully through its own shutdown path. [Sandbox test](../evidence/matrix-awakens/sandbox-gauntlet.json)

## Run with TuxBox on Linux

Start with the original `Mount` and matching `EmbeddedXvd` directories. Install Valve’s Proton Experimental from Steam, then install umu-run, Vulkan headers, liblz4 and the project’s Rust, xwin, clang-cl and lld-link build tools. Shader preparation uses the Linux DXC tools `dxc` and `dxv`. The guest also uses the Windows x64 DXC release. The example accepts its zip file directly. Choose a working directory on the game’s filesystem to take advantage of copy-on-write. [Build prerequisites](../examples/lanes/README.md#build-requirements) · [Microsoft DXC v1.8.2505.1](https://github.com/microsoft/DirectXShaderCompiler/releases/tag/v1.8.2505.1)

```sh
python tools/prepare_frosty.py /path/to/Mount \
    --work /path/to/tuxbox-frosty \
    --windows-dxc /path/to/dxc_2025_07_14.zip
./tools/run_frosty.sh --desktop
journalctl --user -fu tuxbox-frosty.service -o cat
```

The Linux launcher uses TuxBox’s Vulkan layer with stock Proton to handle Xbox constant-buffer offsets. On Steam’s Proton Experimental build 26306234, the launcher read all 3,072 expected words at 16 offsets on both the RTX 4070 Ti SUPER and Radeon. With the layer disabled, the RTX read 768 correct words and Radeon read all 3,072. These measurements cover buffer reads in a compute dispatch. [Stock Proton results](../evidence/matrix-awakens/stock-proton-cbv.json)

The game selects its own graphics profile, including ray tracing and temporal super resolution. Preparation and launch preserve the graphics settings in the supplied working copy; they do not restore or replace a previously modified configuration. For a clean baseline, prepare from the original package. Run `python tools/prepare_frosty.py --check --work /path/to/tuxbox-frosty` to verify the staged binaries and check for known old graphics overrides.

The earlier chase and menu tests disabled several effects and rendered 3D at 1280 × 720 inside a 3840 × 2160 output. Their screenshots and frame times describe that configuration. [Menu comparison](../evidence/matrix-awakens/menu-quality.json) · [Chase input and timing](../evidence/matrix-awakens/chase-gameplay.json)

The preparer builds TuxBox’s Game Runtime, storage, input, memory, ACP, graphics and media adapters. The game supplies its Xbox texture-layout library and packaged video and audio decoders. The shader translator builds from this checkout. Preparation and launch both work with the TomBox and ScarlettX checkouts removed. The working directory records the origin and SHA-256 of each staged binary in `providers.json`. Omit `--desktop` to run with a live terminal log. [Shader source and attribution](../tools/shader-compiler/NOTICE) · [Media checks](../evidence/matrix-awakens/independent-media.json)

Before Frosty starts, TuxBox prepares shaders observed in the menu, the highway chase and the open city. It reads them from the game’s package. The gameplay profile covers all 2,099 distinct shaders requested in the recorded full chase to the city. Later scenes can still request new shaders. TuxBox translates those during play and saves the results. [Shader coverage](SHADERS.md) · [Runtime compilation check](../evidence/providers/shader-runtime-clear.json)

To prepare ahead of time, run `./tools/run_frosty.sh --prepare-only`. On the test PC, preparing all 2,509 entries and six compute pipelines from empty caches took 4 minutes 28 seconds. With the caches warm, the launcher checked the same profile in 1.89 seconds. For a shorter menu-only test, use `TUXBOX_SHADER_PREP=menu ./tools/run_frosty.sh --desktop`. [Preparation results](../evidence/matrix-awakens/chase-warmup.json)

TuxBox also creates six compute pipelines before play, including culling, upscaling and temporal anti-aliasing. All six cache keys matched Frosty’s retail runs. Creating them took 12.18 seconds total on GeForce and 6.86 seconds on Radeon with empty caches. In a second process the same calls took 5.52 milliseconds and 1.82 milliseconds respectively. [GPU timings and pipeline keys](../evidence/matrix-awakens/chase-warmup.json)

The NVIDIA driver also caches finished pipelines. With Frosty’s flyover filter and particle simulation, creating the two pipelines took 6.0 and 25.3 seconds after switching to an empty driver cache, even with Proton’s cache populated. Reusing both caches brought those calls down to 1.9 and 3.1 milliseconds. This helps explain why repeat runs with the same prepared shaders can reach the chase at different times. [Pipeline timings](../evidence/matrix-awakens/host-pipeline-cache.json)

The game draws the green code on a plane attached to its camera. Its film-grain compute shader combines 64 lanes of shared data to calculate how bright the grain texture should be. The Xbox lanes ran together, but on the RTX 4070 Ti SUPER the calculation crossed two hardware waves and read unfinished sums. Synchronizing the reads and writes restored the long trails with the game’s film grain enabled. In a captured frame, background pixels with green above 10 rose from 1.24% to 23.37%. Earlier fixes let the game select the visible plane, address the glyph texture and build its colour table. [GPU measurements](../evidence/matrix-awakens/film-grain-reduction.json) · [Live menu capture](../evidence/matrix-awakens/menu-code-trails.webp)

The chase exercises two Xbox graphics operations beyond the menu. The allocator copies whole GPU resources through command-list slot 84. Later, Frosty switches a texture at a guest virtual address from shader-read to copy-destination state. Adding the first operation got Frosty through the crowd sequence. Adding the second let it enter the chase intro and load the dialogue scene. This earlier run ended 11 minutes 22 seconds after Start Chase, during the dialogue. Separate NVIDIA and Radeon tests checked the copied texture on the GPU and in the original Xbox memory after a fence. [Retail comparison](../evidence/matrix-awakens/placed-copy-chase.json) · [NVIDIA copy tests](../evidence/matrix-awakens/command-list-copy.json) · [Radeon copy tests](../evidence/matrix-awakens/command-list-copy-amd.json)

When the driver rejects a vertex or geometry shader whose outputs occupy different registers from the pixel shader’s inputs, TuxBox rebuilds that pairing and caches it. An earlier retail run compiled 175 individual shaders and 33 shader pairs on demand before hitting slot 84 in Wake Up. [Wake Up frame](../evidence/matrix-awakens/shader-linked-wake-up.webp) · [Earlier retail run](../evidence/matrix-awakens/shader-linked-chase.json) · [Cold and warm pipeline checks](../evidence/matrix-awakens/runtime-shader-links.json)

### Test a Proton fork

Give each Proton build its own working directory so its prefix and media libraries stay paired. Prepare the fork with `--proton /path/to/proton-build/dist --no-select` alongside the usual preparation arguments. Then launch it with `./tools/run_frosty.sh --desktop --work /path/to/fork-work`. The launcher uses the Proton installation recorded in that directory. [Fork patches and build instructions](../tools/proton/README.md)

The Linux fork starts from the Valve Experimental release used for the chase. Its vkd3d-proton change handles 16-byte offsets in descriptor-table constant buffers. A build advertising `cbv_16_byte_alignment` in its `tuxbox-features` file uses that option automatically. Stock builds use the existing Vulkan layer. In the offset test the RTX 4070 Ti SUPER read all 3,072 words correctly with the fork option enabled, up from 768. Radeon read all 3,072 in both modes. The full fork reached Frosty’s menu on the RTX 4070 Ti SUPER with MangoHud enabled. [Desktop capture](../evidence/matrix-awakens/proton-menu.jpg) · [Valve source revision](https://github.com/ValveSoftware/Proton/tree/871b0bf1402dd9632a16495a2def7036161e771c) · [GPU readbacks](../evidence/matrix-awakens/proton-cbv.json)

On the fork, Frosty reached the shooting tutorial with the independent pixel and compute emitters enabled alongside the legacy fallback. With the menu cache prepared, the Switch Targets and Shoot prompts were visible about 25 minutes after selecting Start Chase. Seventeen distinct pixel programs requested by the title matched the independent compiler’s DXIL in the cache. The run had multi-second stalls and missing surfaces. On window close, the guest reported `DXGI_ERROR_ACCESS_LOST` and exited with code 1. After the session the chase cache reused all 1,701 entries on a repeat prepare-only check. [Tutorial capture](../evidence/matrix-awakens/independent-chase-tutorial.jpg) · [Run and cache measurements](../evidence/matrix-awakens/independent-chase.json)

### Measure performance

Launch with `./tools/run_frosty.sh --desktop`. With MangoHud installed, the game shows FPS, frame time, CPU and GPU load, RAM and VRAM. The live console prints the capture folder beside `tuxbox.log`. MangoHud starts recording one sample per present after one second. When the game exits, TuxBox writes `summary.json` with the frame count, average FPS and median, p95, p99 and maximum frame time in milliseconds for each recording. Higher frame times mean longer waits between frames. [MangoHud logging](https://github.com/flightlessmango/MangoHud/blob/v0.8.4/README.md#fps-logging)

Use **Left Shift + F2** to stop or start a recording at a scene boundary. For chase comparisons, record the same chase section with the same graphics profile and report whether the shader and driver caches were warm. Set `TUXBOX_MANGOHUD=0` for a comparison with the overlay disabled. On multi-GPU systems you can select the GPU used for hardware counters with `TUXBOX_MANGOHUD_PCI` set to its PCI address (for example, `0000:01:00.0`).

For pipeline timings, use `./tools/run_frosty.sh --desktop --profile-pipelines`. Each launch writes a separate vkd3d-proton timeline beside its game log. The first three minutes of a retail test used 192 MiB. The summary lists the 20 longest creation calls, their cache keys and Wine timestamps. Calls on different worker threads can overlap. To read a trace while the game is running or after an interrupted launch, run `python -m project_tuxbox.pipeline_profile /path/to/timeline.json`.

In the warm chase replay, three new graphics pipelines took 565, 341 and 279 ms to create. During a separate 1.82-second gap between presents, 29 Vulkan pipeline calls overlapped and occupied 188 ms of wall time. A 16-second CPU sample from the chase attributed 710 of 803 samples to the control-flow analysis in Proton’s DXIL-to-SPIR-V compiler. That work takes place before the Vulkan call. [Graphics shader pairs and timings](../evidence/matrix-awakens/chase-warmup.json)

## Nanite mesh rendering

Frosty creates its 680-byte Nanite mesh pipeline on both NVIDIA and Radeon. The opening pipeline pairs global mesh shader 3194 with visibility pixel shader 2853. TuxBox prepares both at first boot. The packaged shaders also ran together in a separate Xbox draw test with known cluster data. [Retail traces](../evidence/matrix-awakens/nanite-mesh-retail.json) · [Mesh shader coverage](SHADERS.md#mesh-shaders)

Frosty shades visible Nanite tiles using screen rectangles. The Xbox GPU takes three corners for each rectangle. TuxBox supplies the fourth and interpolates its attributes. The original tile vertex shader selected two tiles in the GPU test. All 72 covered pixels and 184 surrounding pixels matched on both Radeon and NVIDIA. [Rectangle GPU readbacks](../evidence/providers/screen-rectangles.json)

In the desktop Sandbox test, the central tower, building fronts, sidewalks and parked cars became visible in the same plaza. Surface gaps, character rendering and long frame stalls remain visible. [Before](../evidence/matrix-awakens/city-nanite-before.jpg) · [After](../evidence/matrix-awakens/city-nanite-after.jpg) · [Capture settings and measurements](../evidence/matrix-awakens/city-nanite-shading.json)

The Nanite pass also writes a stencil value for each fragment. Both packaged shader variants passed an Xbox draw test that compared every depth and stencil value with the expected result. [RTX stencil readbacks](../evidence/matrix-awakens/stencil-export-nvidia.json)

On Radeon the game presented a 3840 × 2160 frame, created its instance- and cluster-culling pipelines, then created the `MicropolyRasterize` pipeline at global entry 3246. Frosty then used graphics command-list slot 96 to request its default asynchronous compute budget. TuxBox accepts that request. The Radeon run subsequently loaded the Construct map with Nanite enabled. The GPU checks cover the default budget and rejection of unsupported values. [Radeon command-list checks](../evidence/providers/async-compute-budget-amd.json) · [NVIDIA command-list checks](../evidence/providers/async-compute-budget-nvidia.json) [Mesh pipeline results](../evidence/matrix-awakens/nanite-mesh-retail.json) · [Cluster culling](../evidence/matrix-awakens/persistent-cluster-retail.json) · [Micropolygon rasterization and slot 96](../evidence/matrix-awakens/micropoly-retail.json)

## What carries over to ProjectScarlett

The supplied ProjectScarlett package was inventoried, but its declared game executable was absent. The [historical investigation](MATRIX_HISTORY.md#what-carries-over-to-projectscarlett) includes a symbol lookup example and library checks.

## Earlier experiments

Earlier tests, including reduced-settings runs and comparisons with external reference runtimes, are documented in [Matrix Awakens experiment history](MATRIX_HISTORY.md). Those reference runtimes are not required by the current launcher.
