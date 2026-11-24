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


### CPU scalar reference and generated semantic tests

The independent reference model (`tools/reference/rdna2_scalar.py`) decodes
native words and models ordinary integer SGPR operations using the
[AMD RDNA 2 ISA reference](https://www.amd.com/content/dam/amd/en/documents/radeon-tech-docs/instruction-set-architectures/rdna2-shader-instruction-set-architecture.pdf)
(SOP1, SOP2 and SOPC). It is intentionally not a complete Xbox GPU simulator.
The model covers 77 instruction forms, ordinary registers s2..s9, SCC,
32/64-bit arithmetic, bit operations, shifts, comparisons and conditional
moves. It rejects unsupported selectors, incomplete programs, uninitialized
reads, and unsupported instruction classes instead of inventing results.
It does not simulate memory, textures, wave execution, vector lanes,
floating point, control flow or hardware timing.

Generate deterministic boundary-value and random programs, including
overlapping registers and chains. The core profile covers 65 forms accepted
by the baseline; extended adds the 12 forms proposed in #58.

```sh
python tools/check_scalar_encodings.py --llvm-mc /path/to/llvm-mc
python tools/generate_scalar_tests.py /tmp/scalar-corpus --per-op 128 --chains 256
cargo test --manifest-path tools/shader-compiler/Cargo.toml \
    --target x86_64-unknown-linux-gnu --lib --no-run
```

Use the Rust library test executable printed by Cargo (the
`tuxbox_xshader-...` binary, not the CLI executable):

```sh
python tools/check_scalar_oracle.py --corpus /tmp/scalar-corpus \
    --bridge /path/to/tuxbox_xshader-test-binary \
    --dxc /path/to/dxc --dxv /path/to/dxv \
    --work /tmp/scalar-check --output /tmp/scalar-receipt.json --jobs 2
```

The bridge runs the production native decoder and full translator. It batches
complete emitted HLSL function bodies only when their helper headers match,
and adds stores for all eight registers and the condition code. All input
registers and SCC are initialized by each synthetic program. DXC folds the
constant computations; DXV validates the resulting DXIL. Missing, duplicate,
nonconstant or mismatched stores fail the gate, as do translation refusals or
tool failures. The Python checker recomputes expected values independently
from the native words, not from the translator's HLSL or a trusted manifest.
The test does not execute generated DXIL on hardware.

Inspect any case step-by-step without invoking a compiler:

```sh
python tools/reference/rdna2_scalar.py /tmp/scalar-corpus/programs.txt --case 000000
```

Results on the measured CPU-only campaign:
- LLVM confirms all 77 instruction encodings.
- Core: 8,577/8,577 programs, 77,193 register/flag values, no mismatches;
  also passes on the separate #58 translator test build.
- Extended on #58: 10,026/10,113 programs, 90,234 compared values, no
  numerical mismatches. 87 chained programs are conservatively refused,
  and the gate correctly fails. These are reproducible remaining limitations,
  not confirmed failures in the retail game.
- A deliberately faulty addition-to-subtraction translator in a temporary
  test worktree produced five mismatches. The mutation is not in this PR.

See [the receipt](../evidence/scarlett-scalar-oracle.json) for case IDs,
seeds, exact corpus/compiler digests and counts. This is a reusable test tool,
not a change to the shipping translator or a claim of retail compatibility.

Rechecked the same core corpus after updating to upstream `3086e0b`: all
8,577 cases passed again. The earlier #58 and mutation receipts retain their
original source fingerprints.
