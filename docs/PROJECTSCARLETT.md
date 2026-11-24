# Set up ProjectScarlett (GTA VI) on Linux

This guide stages GTA VI 1.0.2.1 (`ProjectScarlett`) with TuxBox's providers and starts it on Valve's stock Proton Experimental. The current result is the player selection prompt at 3840×2160. [Title progress](PROGRESS.md) · [Run record](../evidence/projectscarlett-startup.json)

You supply the game package. TuxBox doesn't include any game files. The recorded runs use package `RockstarGamesInc.28872443D20A7` 1.0.2.1, whose `game_scarlett_master.exe` has SHA-256 `e95020b2fa4a721db3df59fdecf75621730ac559d8835252f6c3b658d5a08993`. Other builds may need different providers or shader requests.

## Requirements

- The project's Linux build tools: Python 3.10 or newer, Rust with the `x86_64-pc-windows-msvc` target, `clang-cl`, `lld-link` and the xwin Windows SDK. [Build requirements](../examples/lanes/README.md#build-requirements)
- Proton Experimental from Steam and `umu-run`.
- Microsoft's Linux DXC release with `dxc` and `dxv`, for shader preparation and runtime translation. Arch's `directx-shader-compiler` 1.10.2605.37 fails on a minimal compute shader. Use Microsoft's release instead. [DirectX Shader Compiler releases](https://github.com/microsoft/DirectXShaderCompiler/releases)
- A C compiler and Vulkan headers. The launcher builds TuxBox's constant-buffer Vulkan layer on first use.

Run `python tuxbox.py inspect /path/to/package` to confirm the package. It should report one executable entry, `GTAO  Scarlett  game_scarlett_master.exe  present`.

## Build the providers

```sh
python -m pip install -r requirements.txt
python build.py
```

To build only what ProjectScarlett loads, run `python build.py <target>` for each of `xmem`, `d3d12`, `xg`, `xapu`, `acphal`, `xcrdapi`, `gameinput` and `xgameruntime`. Some Linux distributions currently need the build workarounds described in [issue #4](https://github.com/brcly/TuxBox/issues/4).

## Stage a working copy

Keep the original package unchanged and launch a copy. The package doesn't contain any of the provider DLL names, so TuxBox's copies are added beside the game files rather than replacing them.

```sh
cp -a --reflink=auto /path/to/GTA6.Xbox.v1.0.2.1 /path/to/scarlett-run
cp build/xmem-smoke/{xmem,tuxbox_xhttp}.dll \
   build/d3d12-smoke/d3d12_xs.dll \
   build/xg-provider/xg_xs.dll \
   build/xapu/{xapu,chad}.dll \
   build/acphal/AcpHal.dll \
   build/xcrdapi/xcrdapi.dll \
   build/gameinput/GameInput.dll \
   build/xgameruntime/xgameruntime.dll \
   /path/to/scarlett-run/
```

On a copy-on-write filesystem such as Btrfs or XFS, `--reflink=auto` copies the package almost instantly without using more disk space. If you use a hard-link copy (`cp -al`) to save space, never write into the linked game files. Writing to one also changes the original package.

| Provider | Build output | Loaded by |
| --- | --- | --- |
| `xmem.dll` | `build/xmem-smoke/` | the executable, `Bink2Scarlett.dll`, `XboxBvhBuilder.dll`, `amd_fidelityfx_upscaler_xbox.dll` |
| `tuxbox_xhttp.dll` | `build/xmem-smoke/` | `xmem.dll`, which forwards `XMemSetWinHttpHooks` to it |
| `d3d12_xs.dll` | `build/d3d12-smoke/` | the executable, `XboxBvhBuilder.dll`, `amd_fidelityfx_upscaler_xbox.dll` |
| `xg_xs.dll` | `build/xg-provider/` | the executable |
| `xapu.dll`, `chad.dll` | `build/xapu/` | the executable |
| `AcpHal.dll` | `build/acphal/` | the executable |
| `xcrdapi.dll` | `build/xcrdapi/` | the executable |
| `GameInput.dll` | `build/gameinput/` | the executable |
| `xgameruntime.dll` | `build/xgameruntime/` | the executable, at startup by name |

Copy the providers again after each rebuild. The launcher enables the constant-buffer layer only when the staged `d3d12_xs.dll` is byte-identical to `build/d3d12-smoke/d3d12_xs.dll`.

## Launch offline

The game installs a Breakpad crash handler that uploads minidumps to an online crash service. Run it without network access so that crashes during testing are not uploaded. On Linux, start the launcher in a new network namespace:

```sh
UMU_RUNTIME_UPDATE=0 unshare --user --map-current-user --net -- \
    python tuxbox.py run --log-file build/scarlett.log /path/to/scarlett-run
```

Inside the namespace the only network device is a loopback interface that is down. `UMU_RUNTIME_UPDATE=0` stops umu-run from trying to update the Steam Runtime. Offline launches need unprivileged user namespaces.

Add `--mangohud` before the package path to record frame times. The prefix defaults to `~/.local/share/project-tuxbox/prefixes/Proton - Experimental`. Pass `--prefix` to use another one.

## Shader preparation

The first launch records each shader the game requests in the prefix. Before later launches, the launcher compiles the recorded requests and shows its progress, so the game reuses those translations instead of compiling them during play. Pass `--prepare-only` to prepare without starting the game, or `--no-shader-prep` to skip preparation for one launch. Place either option before the package path. [Preparation results](../evidence/projectscarlett-shader-preparation.json)

## What to expect

The game opens its Americas window, loads its archives and presents 3840×2160 frames. The recorded run reached the player selection prompt, with Account Picker, Screen Narration and TuxBox options. It stopped at a title breakpoint 97 seconds after launch. Intro video, input after player selection and gameplay aren't verified yet. [Current priorities](PROGRESS.md#current-priorities)

In a new prefix, the game stops at a breakpoint during System Init because it reads its package through the `G:` drive, as on the console, and the launcher doesn't map that drive yet. A fix is in review. [Pull request #6](https://github.com/brcly/TuxBox/pull/6)

When you report a run, include the package version, executable hash, GPU and driver, Proton build, and the log printed in the first line of the launcher's output.


## Offline launch and visual diagnostics

These tools consume existing logs or captures; neither starts the title,
touches the Wine prefix, changes driver settings, or uses a GPU.

### Locate a launch failure

```sh
python tools/diagnose_scarlett_run.py build/scarlett.log --json build/diagnosis.json
```

The diagnostic reads the launcher's actual status format, including appended
sessions, recorded D3D12 milestones, shader compiler outcomes, explicit
pipeline/resource/command failures, missing imports, survey call-site RVAs
and unhandled Wine exceptions. It reports line numbers and timestamps
where available, counts diagnostic events separately from distinct shader
requests, and suggests the next check for each observed fault type. Terminal
exception/device-loss evidence is listed first, then recent diagnostic
groups by line number; this is an investigation order, not causal ranking.
An absolute fault address is not a title RVA. Survey asserts that were
continued are not terminal errors. A missing exit record remains incomplete,
not a diagnosed hang. The first presentation does not prove visible output.
The input file should be complete and stable; mixed process output cannot
establish exact timing or causal attribution. Files over 512 MiB are rejected
and lines over 64 KiB are counted and skipped. Reports omit raw log messages
and paths, but may include module names and addresses; review before sharing.

In the public issue #39 log, the tool identifies the first presentation at
line 514, page mapping at 548, the slot 86 module-not-found HRESULT at 541,
64 logged compute-pipeline failure events and 836 distinct refused shaders.
The log has no exit record; the missing module is not identified by its
HRESULT, and these observations do not establish the ultimate stop.

### Compare rendered frames

Use a Python environment with Pillow installed (the existing project
requirements are unchanged; the frame analysis is optional).
Capture reference and candidate at the same camera, resolution, frame,
UI state and color settings, preferably lossless RGB or RGBA 8-bit SDR PNG.
The tool cannot automatically establish this equivalence.

```sh
python tools/compare_scarlett_frames.py reference.png candidate.png \
    --json frame-comparison.json --heatmaps frame-errors
```

For a sequence, pass two directories of equally named PNGs; names must
match exactly. The tool compares in lexicographic name order and counts
adjacent identical pixel digests, not elapsed time or proven freezes.
Dimensions, color metadata and modes must match. No automatic alignment,
resizing or color conversion. RGBA compares all four straight channels,
including transparent RGB. Optional `--roi X0 Y0 X1 Y1` uses half-open
coordinates to exclude deliberately chosen UI/overlay areas.

Default gates require exact agreement. Set `--tolerance` to a maximum
per-channel integer difference and both `--max-fraction` and
`--max-tile-fraction` to the allowed fraction of pixels exceeding it,
globally and in each tile. Choose thresholds before testing; the local
gate protects small defects from being hidden by global averages.
The report includes MAE, MSE, PSNR (null for exact equality), bounding box,
worst tiles, pixel digests, near-black/flat-color observations and optional
absolute-error grayscale PNGs (0=no difference, 255=maximum difference).
It returns 1 for a failed comparison, 2 for invalid input. Output paths
must be fresh. Maximum capture size is 16,777,216 pixels and maximum
sequence length 256 pairs. HDR/16-bit captures, animation, perceptual
quality and gameplay assessment are out of scope.

Reproduce six synthetic 4K comparisons and two independent launch sessions:

```sh
python tools/reference/scarlett_observability_fixture.py /tmp/observability-control
```

[Validation receipt](../evidence/scarlett-observability.json) includes the
public log fingerprint, controlled diagnostic outcomes and six 4K pairs
(identical, one changed pixel, local defect, black, tolerated one-level
changes, and swapped color channels). These are CPU-side checks, not real
title rendering comparisons.
