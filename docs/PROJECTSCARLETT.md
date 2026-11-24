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


## Memory and performance regression checks

The offline analysis tools below use CPU only. They do not start a game,
touch its prefix, probe the GPU, or change host settings. The memory sampler
reads an explicitly selected Linux PID; it does not discover other processes.

### Process memory

```sh
python tools/scarlett_memory.py capture <game-pid> --seconds 60 --interval 1 \
    --output memory.jsonl
python tools/scarlett_memory.py analyze memory.jsonl --json memory-report.json
```

The sampler checks the process start ticks before and after reading
`/proc/<pid>/smaps_rollup`, to avoid mixing a reused PID with the original.
It reports RSS, PSS (shared resident memory apportioned to this process),
private resident memory, swap and apportioned swap. Growth is evaluated
using PSS plus SwapPss, so swapping memory out does not appear to free it.
Reports include first/last-quarter medians, peak, peak time and retained
growth above the selected `--growth-kib` threshold (default 16 MiB).
A retained-growth flag requires at least 12 samples, the minimum capture
duration (default 30 s), no gap over three nominal intervals, and a
completed duration record. It is not proof of a leak: repeat the same
workload and allow caches to settle. Missing counters or access permission
are errors, not zero-valued memory. This does not measure process descendants,
GPU/driver memory, allocation ownership or inaccessible mappings.

Compare equivalent, complete captures with the same interval and similar
duration. Choose an allowed peak increase in KiB before testing:

```sh
python tools/scarlett_memory.py analyze candidate.jsonl --baseline baseline.jsonl \
    --peak-budget-kib 16384 --json memory-comparison.json
```

The optional allocation trace proposed in #51 can also be analyzed when
available. Use `TUXBOX_TRACE_ALLOCATION_BYTES=8192,16384,invalid` on a
build that includes that trace, then:

```sh
python tools/scarlett_memory.py allocations scarlett.log --json allocations.json
```

It joins interleaved header/item records by ID within each launcher session,
retains caller RVAs and resource descriptions, marks incomplete or conflicting
records, and flags the UINT64_MAX invalid-size sentinel. A capped trace
is incomplete evidence. These records are allocation *size queries*, not
allocations or frees: do not sum their returned bytes to estimate live memory.
The tool has no dependency on #51 and can report no observed queries.

### Repeatable frame-time regression gate

Use existing `--mangohud`, `--profile-pipelines` and
`tools/sample_process_cpu.py` captures as inputs, without altering the
existing launcher summary. Keep hardware, driver, backend, scene, settings,
resolution and cache state identical across the compared builds; alternate
baseline/candidate runs and record the actual process lifetime for each.
Create a JSON manifest (paths relative to the manifest, or absolute):

```json
{
  "schema": 1,
  "context": {
    "scenario": "fixed test scene and capture window",
    "hardware": "same machine and GPU",
    "driver": "same driver version",
    "backend": "same Proton version",
    "resolution": "1920x1080",
    "settings": "same graphics profile",
    "cache_state": "warm"
  },
  "budgets": {
    "mean_fps_drop_pct": 5,
    "p95_increase_pct": 10,
    "p99_increase_pct": 10,
    "one_percent_low_drop_pct": 10,
    "stutter_fraction_increase": 0.01
  },
  "stutter_ms": 50,
  "min_frames": 300,
  "max_skipped_fraction": 0.05,
  "pairs": [
    {"baseline": {"frames": "old1.csv", "seconds": 60},
     "candidate": {"frames": "new1.csv", "seconds": 60}},
    {"baseline": {"frames": "old2.csv", "seconds": 60},
     "candidate": {"frames": "new2.csv", "seconds": 60}},
    {"baseline": {"frames": "old3.csv", "seconds": 60},
     "candidate": {"frames": "new3.csv", "seconds": 60}}
  ]
}
```

```sh
python tools/compare_scarlett_performance.py runs.json --json performance-comparison.json
```

The tool reuses the launcher's frame-time parser, filters invalid and
uptime-like samples, rejects insufficient evidence, and calculates mean FPS
from total valid frame duration, nearest-rank percentiles, 1% low FPS
(inverse mean of the slowest ceil(1%) frames), and the fraction of frames
strictly above the chosen stutter threshold. Every individual pair must
stay within the declared budgets; a favorable median cannot hide a bad
run. Positive reported changes represent regressions. The stutter fraction
budget is an absolute fraction (0.01 is one percentage point).
Three to thirty independent pairs are required. Declared conditions are
not automatically verified; choose equivalent capture windows. A frame
time is not a measurement of game simulation progress.

Each paired run can optionally include a `"cpu": "cpu.jsonl"` and/or
`"pipelines": "timeline.json"` path. CPU usage is weighted by measured
interval duration, in percent of one core, with observed thread-name
totals. Pipeline timings use the existing vkd3d trace summarizer. They
are summed call durations (can overlap), not elapsed wall time or proof
of the bottleneck. These sidecars are diagnostic, not performance gates.
Compare tools return 1 for a violated budget and 2 for invalid input.
No GPU tests or retail frame captures are included in this contribution.

Reproduce real 32 MiB reservation/release in a disposable Linux process
and six synthetic frame-time pairs:

```sh
python tools/reference/scarlett_resource_fixture.py /tmp/resource-control
```

[Validation receipt](../evidence/scarlett-resource-analysis.json) records
the sampled memory and the improved/negative frame-time controls. In the
negative control two pairs improve but the third contains 100 ms stalls;
the all-pairs gate correctly fails despite the improved median.
