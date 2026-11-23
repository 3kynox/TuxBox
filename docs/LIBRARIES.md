# Xbox libraries used by Frosty

The progress map tracks **15 Xbox-facing DLLs**. Frosty imports 11 directly or by delay-load. A loader trace also shows the Game Runtime and the HEVC, H.264 and AAC decoders. The latter two were loaded under different filenames and matched to the package by SHA-256. Run `python tuxbox.py status --libraries` for the active list. [Progress map](progress.svg) · [Loader trace](../evidence/matrix-awakens/independent-menu.json)

Each bar counts exports with checked behavior against the occupied ordinals in that build of the DLL. A factory can pass while methods on the objects it returns still need work. The current original-settings blocker is shader 219 during Lumen scene lighting. [Retail comparison](../evidence/matrix-awakens/three-library-comparison.json)

The full audit covers 61 Xbox-facing DLLs among 249 scanned files. Use `python tools/library_gaps.py d3d12_xs.dll` to see individual ordinals and title imports for any catalogued DLL. Add `--all` for verified entries or `--provider /path/to/file.dll` to compare a provider's export table. [Full package inventory](../evidence/frosty-xbox-library-inventory.json)

## Xbox D3D12 interface methods

Frosty calls most graphics operations through objects created by `d3d12_xs.dll`. Run `python tuxbox.py status --d3d12-xs` to see the tracked slot numbers, test scope and number of handlers in each TuxBox proxy table. The GPU copy test writes a guest-backed RGBA8 texture through graphics command-list slots 84 (whole resource) and 83 (region). It exercises the slot 26 state transitions and checks the tiled bytes after a fence, including the padding. A second handle at the same address and a separately placed array slice both read back the expected pixels. [Interface inventory](../evidence/matrix-awakens/d3d12-xs-interfaces.json) · [GPU copy results](../evidence/matrix-awakens/command-list-copy.json)

The Xbox graphics command list supports direct mesh dispatch at slot 127 and indirect mesh dispatch at slot 59. Both GPU paths produced the expected pixels on NVIDIA and Radeon. [NVIDIA checks](../evidence/matrix-awakens/mesh-command-list-nvidia.json) · [Radeon checks](../evidence/matrix-awakens/mesh-command-list-radeon.json) Frosty also sets its default asynchronous compute budget at slot 96. Both GPUs accepted the title’s six zero words and ran the subsequent dispatch. The provider rejects nonzero budgets. [Retail call site](../evidence/matrix-awakens/micropoly-retail.json) · [Radeon budget check](../evidence/providers/async-compute-budget-amd.json) · [NVIDIA budget check](../evidence/providers/async-compute-budget-nvidia.json)

The denominator for an interface is the set of distinct slots catalogued from this Frosty build and its associated guest traces. Slot numbers are local to each interface. For example, slot 15 copies a buffer on the graphics command list and records a barrier on the compact DMA list. A checked method has a recorded TuxBox test for the behavior listed. Additional methods and argument combinations can still be discovered. Run `python tools/map_d3d12_xs.py` after changing a proxy table, and then regenerate the chart with `python tuxbox.py status --svg docs/progress.svg`. The mapper reads the compiled x64 tables and excludes their unsupported-method handlers.

## Frosty's imported Xbox-facing APIs

Frosty imports **60 symbols across 11 tracked DLLs**. The executable also imports ordinary Windows libraries. Across the whole executable there are 537 regular imports and another 15 delay imports, all from GameChat2. The table counts named and ordinal imports against the reviewed binary. A verified import has at least one tested TuxBox behavior. A factory can pass that test while a game still needs more methods on the object it returns.

| Library | Frosty imports | Verified in TuxBox | Import kind |
| --- | ---: | ---: | --- |
| `AcpHal.dll` | 7 | 0 | Regular |
| `d3d12_xs.dll` | 6 | 6 | Regular |
| `dxcompiler_xs.dll` | 1 | 1 | Regular |
| `GameChat2.dll` | 15 | 9 | Delay-load |
| `GameInput.dll` | 1 | 1 | Regular, ordinal |
| `mfplat.dll` | 13 | 1 | Regular |
| `mfreadwrite.dll` | 1 | 1 | Regular |
| `XAudio2_9.dll` | 5 | 5 | Regular, ordinal |
| `xcrdapi.dll` | 1 | 1 | Regular |
| `xg_xs.dll` | 1 | 1 | Regular |
| `xmem.dll` | 9 | 9 | Regular |
| **Total** | **60** | **35** | |

The packaged Xbox `xmem.dll` also imports `xbthunk.dll`. Frosty’s run with TuxBox XMem established its own dependency path. The full inventory retains these package relationships for reference. [Loader trace](../evidence/matrix-awakens/independent-menu.json) · [Import graph](../evidence/frosty-xbox-library-inventory.json)

Frosty uses the Series `d3d12_xs.dll`. Its six direct imports have checked behavior, and all 54 packaged export names resolve at the matching ordinals in TuxBox. `D3D12UploadCustomMicrocodeX` at ordinal 12 asks the Xbox graphics service to install three firmware images. The Xbox One `d3d12_x.dll` exposes a differently named upload at ordinal 1. Import checks found zero callers across 712 Frosty, Fortnite and ProjectScarlett binaries. TuxBox builds the 56-byte request and maps the driver reply to the guest result. On desktop the receiver is absent. Successful installation still requires a known accepted set of images and a reference for its effect. [Microcode controls and driver findings](../evidence/providers/d3d12-custom-microcode.json)

## Current Frosty gates

In an earlier reduced-settings run, Frosty displayed its menu and video preview in a fresh Proton prefix. TuxBox supplied D3D12, GameInput, storage, memory and media adapters; the title’s system image supplied XG and the HEVC, H.264 and AAC decoders. Keyboard input opened Settings and started the Wake Up sequence. With the original graphics settings, TuxBox XG, XMem and DXC loaded the Construct map and presented a 4K frame. Both that run and the packaged-XG control stopped on Lumen compute shader 219. [Earlier run](../evidence/matrix-awakens/independent-menu.json) · [Retail comparison](../evidence/matrix-awakens/three-library-comparison.json)

The new Rust AcpHal builds the seven functions Frosty imports. The earlier launch reported an unresolved `AcpHalReleaseShapeContexts`, and three static callers check its result. AcpHal behavior and GPU-visible XMem pages remain open. [AcpHal call sites](../evidence/matrix-awakens/acphal-call-sites.json) · [XMem call sites](../evidence/matrix-awakens/xmem-physical-pages.json)

## Graphics and memory

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `d3d12_xs.dll` | 🟡 Partial | **53/54 exports** have checked behavior (21/22 functions and 32/32 identifiers). The GPU placement check covers configured address bounds, cache flush and memory profiling. The private driver handles record updates and device disposal. PIX supports table prefixes through 32 bytes. Firmware installation, framebuffer backing, private descriptor heaps and active capture remain open. [GPU placement](../evidence/providers/d3d12-configured-placement.json) · [Driver operations](../evidence/providers/d3d12-private-driver.json) |
| `dxcompiler_xs.dll` | 🟡 Partial | All **4/4 factories** have checked behavior. The Xbox linker accepted five TuxBox-compiled Frosty miss shaders. General ray traversal and scheduling remain open. [Ray compiler controls](../runtime/dxc/README.md) |
| `xmem.dll` | 🟡 Partial | All **32/32 exports** have checked behavior. Frosty’s memory profile passed 512 simultaneous CPU mappings. Pools, partition accounting and the live history page are also tested. GPU-visible pages remain open. [Memory controls](../runtime/xmem/README.md) · [Partitions](../evidence/providers/xmem-partitions.json) |
| `xg_xs.dll` | 🟡 Partial | All **21/21 exports** have checked behavior. All 1,760 distinct texture layouts captured through Frosty’s second frame matched the Xbox DLL. DCC checks cover 45 formats, including per-pixel compression in the eight-bit profiles. [Layout replay](../evidence/providers/xg-frosty-trace-replay.json) · [DCC results](../evidence/providers/xg-dcc.json) |

## Platform and storage

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `xgameruntime.dll` | 🟡 Partial | TuxBox implements four tested runtime entry points, including the threading-interface query. Queue checks cover dispatch, delayed work, cancellation and termination. User, device association and other services remain open. [Queue control](../evidence/providers/xgameruntime-queues.json) · [Mapped GDK wrappers](../evidence/matrix-awakens/runtime-wrappers.csv) |
| `xcrdapi.dll` | 🟡 Partial | TuxBox reads raw and zlib-compressed data through Xbox storage queues. Two 64 KiB blocks from Frosty’s container matched an independent decoder on two Proton builds. BCPack and physical-page requests remain open. [Storage checks](../evidence/providers/xcrdapi-storage.json) |

## Input and audio

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `GameInput.dll` | 🟡 Partial | Lanes reads keyboard, mouse and XInput through the TuxBox factory. The provider handles Frosty’s blocking device enumeration and maps desktop input to a gamepad. Tests cover 48 key transitions, mouse aim and menu confirmation. [Device and desktop input checks](../evidence/providers/gameinput-devices.json) · [Lanes input](../evidence/lanes-xbox-input.json) |
| `AcpHal.dll` | 🟡 Partial | The new Rust provider builds the seven exports Frosty imports. It uses a host-backed APU heap and XMA context array, and returns an inert IAcpHal object; SHAPE/XMA DSP is not implemented. Its export bar remains at 0/10 until behavior is verified. The recorded mixer control used a reference provider. [Provider design](../runtime/acphal/README.md) · [Static call sites](../evidence/matrix-awakens/acphal-call-sites.json) · [Mixer control](../evidence/matrix-awakens/frosty-engine-control.json) |
| `XAudio2_9.dll` | 🟡 Partial | TuxBox maps eight Xbox exports to system XAudio2, including all five Frosty imports at startup. Tests measure left and right output from the 2.9 and versioned factories. The 2.8 reverb factory, shared-context factory and XMA playback remain open. [Stereo and export comparisons](../evidence/providers/xaudio-control.json) |
| `GameChat2.dll` | 🟡 Partial | Thirteen exports cover chat sessions, remote users, bitrate and outgoing endpoint announcements. The tests compare packet bytes, destination IDs and the lifetime of each batch. Voice and incoming packets remain open. The game supplies network delivery. [Microsoft’s outgoing frame contract](https://learn.microsoft.com/en-us/gaming/gdk/docs/services/multiplayer/chat/game-chat2/using-game-chat-2#processing-data-frames) · [Guest](../runtime/gamechat/guest.c) · [Original and TuxBox comparisons](../evidence/providers/platform-controls.json) |

## Media

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `mfplat.dll` | 🟡 Partial | The TuxBox bridge uses Proton Media Foundation and preserves 135 of the 140 Xbox export names and ordinals. The Xbox surface-buffer and device-manager entries passed GPU readback on desktop and TuxBox devices. [Independent controls](../evidence/matrix-awakens/independent-media.json) · [Movie comparisons](../evidence/matrix-awakens/retail-chase-cinematic.json) |
| `mfhevcdec.dll` | 🟡 Partial | A TuxBox transform adapter delivers synchronized D3D12 NV12 samples around the supplied HEVC decoder. Intro and chase frames were compared with a reference decode. [Intro](../evidence/matrix-awakens/hevc-valve.json) · [Chase](../evidence/matrix-awakens/wake-hevc-valve.json) |
| `mfh264dec.dll` | 🟡 Partial | A TuxBox transform adapter exposes the supplied H.264 decoder to the game. Both 1024 × 512 menu previews returned D3D12 samples. Other output formats remain unverified. [H.264 results](../evidence/matrix-awakens/h264-valve.json) |
| `mfreadwrite.dll` | 🟡 Partial | TuxBox reads PCM WAV audio from byte streams, local paths and file URLs. Tests check stereo samples, timestamps, seeking, end of stream and file-handle release. Frosty’s MP4 movies still need the existing video reader. [Source-reader tests](../runtime/mfreadwrite/guest.cpp) · [Path and URL comparisons](../evidence/providers/mfreadwrite-url-control.json) |
| `MSAudDecMFT.dll` | 🟡 Partial | TuxBox adapts GE-Proton’s AAC decoder. It produced 98,304 frames of six-channel PCM from 96 Frosty soundtrack packets and matched FFmpeg within 0.000016 per sample. Proton Experimental failed to decode the same audio format. [Frosty audio](../evidence/providers/aac-frosty-ge.json) · [Experimental result](../evidence/providers/aac-valve.json) |

The separate Scarlett package inventory includes game middleware, redistributable runtimes and GPU helpers. All 16 supplied DLLs loaded on Proton, and selected upscaler and BVH calls also passed behavioral tests. Those results track package compatibility outside the Xbox platform-library counts above. [Library loads](../evidence/scarlett-all-library-load.json) · [Functional checks](../evidence/scarlett-library-controls.json)

## Refresh the local inventory

The scan command expects a package root containing `EmbeddedXvd/Windows/System32` and `Mount/Frosty/Binaries/XSX/Frosty-XSX-Shipping.exe`. It inventories the entire package and refreshes metadata for reviewed providers. Add a DLL to the active list when a title import, runtime load or reproduced dependency establishes its role.

```sh
python tools/scan_frosty_xbox_libraries.py /path/to/package --update-map
python tuxbox.py status --svg docs/progress.svg
python tools/check_library_exports.py /path/to/package
```

The static graph comes from PE imports and delay imports. Runtime traces are needed to find DLLs loaded dynamically.

## Run the provider checks

Build the DLLs and run these checks on Linux with a stereo audio output. These tests use generated inputs and can run from this checkout alone. The AAC comparison also needs FFmpeg and a GE-Proton installation. [Build requirements](../examples/lanes/README.md#build-requirements) · [Proton Experimental results](../evidence/providers/valve.json) · [GE-Proton results](../evidence/providers/ge.json)

```sh
python build.py
python tuxbox.py verify --only xmem --only xg-provider --only gameconfig --only gamechat \
  --only xaudio --only mfreadwrite --output build/provider-results.json
python tools/check_aac_provider.py --proton "/path/to/GE-Proton" \
  --prefix build/aac-prefix --output build/aac-results.json
```

To time the XG format checks, run `python tuxbox.py run build/xg-provider/xg-texture.exe --bench-formats timing.txt`. The guest writes seven timings beside the executable after one warm-up. On a Ryzen 7 9800X3D under Proton Experimental the median was 39.1 ms for TuxBox and 219.9 ms for the supplied DLL. These timings cover the 41-format address and validation workload. [Measurements](../evidence/providers/xg-formats.json)

The AAC check loads the TuxBox transform directly and compares six generated tones channel by channel with FFmpeg. Pass `--video /path/to/movie.mp4` to check the first 96 packets of a 48 kHz 5.1 AAC-LC track. The recorded Frosty run used its staged graphics and platform providers and packaged media decoders. [Generated audio results](../evidence/providers/aac-ge.json) · [Retail setup](MATRIX_AWAKENS.md)
