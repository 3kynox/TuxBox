# Xbox library inventory and translation

The map now tracks **61 Xbox-facing DLLs** found in Frosty's Xbox system image and game folder. Each bar is the number of DLL exports verified through a TuxBox provider, out of the exports in that exact local binary. See the [progress map](progress.svg), run `python tuxbox.py status --libraries`, or open the [full PE inventory](../evidence/frosty-xbox-library-inventory.json).

The scan parsed all **245 system-image DLLs** and **4 game-folder DLLs** with no errors. It selected libraries by Xbox, GameCore, Durango, Game OS, PIX, or console-support metadata; known Xbox, Xtf, and PIX families; and the Xbox-facing DLLs already tracked by this project. The inventory records binary hashes, file descriptions and versions, exports and ordinals, imported symbols, reverse importers, and the static path from Frosty's executable. Export names that are aliases for one ordinal count once. Export coverage is a useful translation measure, but it does not count methods on COM objects or prove that every title path works.

Use `python tools/library_gaps.py xmem.dll xg_xs.dll` to list the unverified exports and Frosty's direct imports. Add `--all` to include verified entries, or compare a built provider with `python tools/library_gaps.py xmem.dll --provider /path/to/xmem.dll`. The comparison checks names and ordinals separately; an exported symbol is not necessarily implemented or behaviorally tested. `--json` produces machine-readable results.

| Status | Libraries | What it means |
| --- | ---: | --- |
| 🟢 Complete | 1 | All identified exports and Xbox-facing behavior for the reviewed build are implemented and verified. |
| 🟡 Partial | 17 | A TuxBox provider or named reference bridge covers some behavior; other operations remain open. |
| 🔴 Incomplete | 43 | The DLL is inventoried, but no TuxBox export behavior has been verified. |

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

The static graph reaches one more tracked library, `xbthunk.dll`, through `xmem.dll`. The other 49 have no static path from this executable. Runtime traces can still identify requirements among them. Frosty loads `xgameruntime.dll` dynamically. The independent menu run used TuxBox’s Game Runtime, GameInput and storage libraries. [Independent loader evidence](../evidence/matrix-awakens/independent-menu.json) [Runtime evidence](../evidence/matrix-awakens/reference-runtime.json) · [Import and dependency inventory](../evidence/frosty-xbox-library-inventory.json)

### Newly catalogued system-image modules

These 45 libraries were added from the on-disk scan. Export counts are occupied ordinals, including ordinal-only exports. The title-path column describes static imports. The progress map also tracks the verified exports in each library.

| Family | Library | Exports | Static title path |
| --- | --- | ---: | --- |
| Graphics and memory | `d3d12_x.dll` | 53 | None found |
| Graphics and memory | `d3dcompiler_47.dll` | 29 | None found |
| Graphics and memory | `dxbc2dxil_xs.dll` | 1 | None found |
| Graphics and memory | `dxcompiler_x.dll` | 2 | None found |
| Graphics and memory | `newbe_xs.dll` | 6 | None found |
| Graphics and memory | `scdxil.dll` | 7 | None found |
| Graphics and memory | `xbsc_xs.dll` | 41 | None found |
| Graphics and memory | `xg_x.dll` | 21 | None found |
| Graphics and memory | `xgs12.dll` | 17 | None found |
| Graphics and memory | `xgs12_10.dll` | 18 | None found |
| Graphics and memory | `xgs12_10d.dll` | 18 | None found |
| Graphics and memory | `xgs12_10i.dll` | 18 | None found |
| Graphics and memory | `xgs12_d.dll` | 17 | None found |
| Graphics and memory | `xgs12_i.dll` | 17 | None found |
| Platform and storage | `ConnectedStorageService.ProxyStub.dll` | 5 | None found |
| Platform and storage | `gamecoreclient.dll` | 3 | None found |
| Platform and storage | `gamecorecomproxy.dll` | 5 | None found |
| Platform and storage | `minappmodelx.dll` | 4 | None found |
| Platform and storage | `minuserx.dll` | 74 | None found |
| Platform and storage | `msave64xgs.dll` | 24 | None found |
| Platform and storage | `xbthunk.dll` | 11 | Through `xmem.dll` |
| Platform and storage | `xbtp.dll` | 132 | None found |
| Input and audio | `AudioHALWrapper.dll` | 32 | None found |
| Input and audio | `GCXAudioExt.dll` | 11 | None found |
| Input and audio | `xapu.dll` | 6 | None found |
| Input and audio | `XVMAudioService.dll` | 3 | None found |
| Diagnostics and tooling | `hix_xs.dll` | 8 | None found |
| Diagnostics and tooling | `pixrt.dll` | 3 | None found |
| Diagnostics and tooling | `pixrt10.dll` | 3 | None found |
| Diagnostics and tooling | `WerXboxGc.dll` | 8 | None found |
| Diagnostics and tooling | `xhit.dll` | 4 | None found |
| Diagnostics and tooling | `xhit_s.dll` | 4 | None found |
| Diagnostics and tooling | `pixevt.dll` | 16 | None found |
| Diagnostics and tooling | `pixlivemerge.dll` | 6 | None found |
| Diagnostics and tooling | `xtfbaseservices.dll` | 60 | None found |
| Diagnostics and tooling | `xtfconsolecontrol.dll` | 2 | None found |
| Diagnostics and tooling | `xtfdebugmonitor.dll` | 2 | None found |
| Diagnostics and tooling | `xtfdiaginfo.dll` | 2 | None found |
| Diagnostics and tooling | `xtfeventserver.dll` | 2 | None found |
| Diagnostics and tooling | `xtffileio.dll` | 2 | None found |
| Diagnostics and tooling | `xtfpix.dll` | 9 | None found |
| Diagnostics and tooling | `xtfremoterun.dll` | 2 | None found |
| Other Xbox support | `xfp_gc.dll` | 8 | None found |
| Other Xbox support | `xfrontpaneldisplay.dll` | 8 | None found |
| Other Xbox support | `ConIoSrv.dll` | 1 | None found |

The metadata identifies several useful families: `d3d12_x.dll` is the UWP D3D12 extension; the `xgs12*` files are UMD12 driver variants; `newbe_xs.dll`, `xbsc_xs.dll`, and the DXIL compilers are shader/compiler support; `hix_xs.dll` and `xhit*.dll` are hang diagnostics; and the `pixrt*`, `pixevt`, and `xtf*` files are Xbox tooling. The XTF modules import shared `xbtp.dll` services, and PIX runtime modules also import `xbtp.dll` and `xmem.dll`. The `msave64xgs.dll` export table has 24 ordinal-only entries, so the scanner preserves their ordinals without guessing names.

Static import edges also show shared dependencies: the `xgs12_10*` drivers import `xbsc_xs`, `xg_xs`, and `d3d12_xs`; `xgameruntime.dll` imports `gameconfighelper`, `xcrdapi`, and `xmem`; audio extensions import `AcpHal` and `xmem`; and media DLLs import `mfplat` and `xmem`. These are candidate translation clusters, not proof that Frosty loads every member.

## Current Frosty gates

In an earlier reduced-settings run, Frosty displayed its menu and video preview in a fresh Proton prefix. TuxBox supplied D3D12, GameInput, storage, memory and media adapters; the title’s system image supplied XG and the HEVC, H.264 and AAC decoders. Keyboard input opened Settings and started the Wake Up sequence. With the original graphics settings, the latest recorded run presented a black frame and stopped during Lumen shader creation. [Earlier run](../evidence/matrix-awakens/independent-menu.json) · [Original-settings run](../evidence/matrix-awakens/packaged-renderer.json)

The new Rust AcpHal builds the seven functions Frosty imports. The earlier launch reported an unresolved `AcpHalReleaseShapeContexts`, and three static callers check its result. AcpHal behavior and GPU-visible XMem pages remain open. [AcpHal call sites](../evidence/matrix-awakens/acphal-call-sites.json) · [XMem call sites](../evidence/matrix-awakens/xmem-physical-pages.json)

## Graphics and memory

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `d3d12_xs.dll` | 🟡 Partial | **47/54 exports** (15/22 functions and 32/32 identifiers). The CPU mode of `D3D12ZeroMemoryX` matches the packaged DLL. Frosty creates graphics pipelines from Xbox `ID3D12Device2` streams and displays its main menu. GPU readbacks verify indexed draws, depth and stencil planes, decoded video copies, indirect compute and guest-backed placed textures. [CPU clear comparison](../evidence/providers/d3d12-cpu-clear.json) · [GPU checks](../runtime/d3d12/README.md#what-runs-on-the-gpu) · [Export comparison](../evidence/providers/d3d12-exports.json) · [Retail startup](../evidence/matrix-awakens/tuxbox-menu-startup.json) |
| `dxcompiler_xs.dll` | 🟡 Partial | Two of four Scarlett factories are verified for owned blobs, caller allocation, portable shader compilation and compiler diagnostics. The two ray-tracing factories remain open. [Compiler checks](../evidence/providers/dxc-factories.json) |
| `dxcompiler_x.dll` | 🟡 Partial | Both Xbox One factories work for the same tests at ordinals 1 and 2. Xbox-specific GPU code generation and compiler intrinsics remain open. [Original and TuxBox comparisons](../evidence/providers/dxc-factories.json) |
| `xmem.dll` | 🟡 Partial | All nine Frosty imports have CPU-side checks. The three physical-page calls retain data when 64 KiB pages are unmapped and remapped in a different order. GPU-visible backing and retail use of these mappings remain open. [Memory tests](../runtime/xmem/README.md) · [Page remapping](../evidence/providers/xmem-pages.json) |
| `xg_xs.dll` | 🟡 Partial | Linear and 4 KiB tiled textures match the packaged DLL at 9,231,018 texel or block addresses across 78 formats. Tests include packed mips, array slices and eight descriptors from Frosty. Additional layouts and texture copies remain open. [Layout and address comparisons](../evidence/providers/xg-placement-layouts.json) |

## Platform and storage

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `xgameruntime.dll` | 🟡 Partial | TuxBox implements four tested runtime entry points, including the threading-interface query. Queue checks cover dispatch, delayed work, cancellation and termination. User, device association and other services remain open. [Queue control](../evidence/providers/xgameruntime-queues.json) · [Mapped GDK wrappers](../evidence/matrix-awakens/runtime-wrappers.csv) |
| `xcrdapi.dll` | 🟡 Partial | TuxBox reads raw and zlib-compressed data through Xbox storage queues. Two 64 KiB blocks from Frosty’s container matched an independent decoder on two Proton builds. BCPack and physical-page requests remain open. [Storage checks](../evidence/providers/xcrdapi-storage.json) |
| `gameconfighelper.dll` | 🟡 Partial | TuxBox builds package registry paths and opens both package and machine-wide Game Core configuration keys with the requested access. The tests cover both registry hives and read-only access. Frosty’s embedded runtime still needs `GetCurrentPackageGameConfig`. [Registry tests](../runtime/gameconfig/guest.c) · [Control results](../evidence/providers/platform-controls.json) |

## Input and audio

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `GameInput.dll` | 🟡 Partial | Lanes reads keyboard, mouse and XInput through the TuxBox factory. The provider handles Frosty’s blocking device enumeration and maps desktop input to a gamepad. Tests cover 48 key transitions, mouse aim and menu confirmation. [Device and desktop input checks](../evidence/providers/gameinput-devices.json) · [Lanes input](../evidence/lanes-xbox-input.json) |
| `AcpHal.dll` | 🟡 Partial | The new Rust provider builds the seven exports Frosty imports. It uses a host-backed APU heap and XMA context array, and returns an inert IAcpHal object; SHAPE/XMA DSP is not implemented. Its export bar remains at 0/10 until behavior is verified. The recorded mixer control used a reference provider. [Provider design](../runtime/acphal/README.md) · [Static call sites](../evidence/matrix-awakens/acphal-call-sites.json) · [Mixer control](../evidence/matrix-awakens/frosty-engine-control.json) |
| `XAudio2_9.dll` | 🟡 Partial | TuxBox maps eight Xbox exports to system XAudio2, including all five Frosty imports at startup. Tests measure left and right output from the 2.9 and versioned factories. The 2.8 reverb factory, shared-context factory and XMA playback remain open. [Stereo and export comparisons](../evidence/providers/xaudio-control.json) |
| `GameChat2.dll` | 🟡 Partial | Thirteen exports cover chat sessions, remote users, bitrate and outgoing endpoint announcements. The tests compare packet bytes, destination IDs and the lifetime of each batch. Voice and incoming packets remain open. The game supplies network delivery. [Microsoft’s outgoing frame contract](https://learn.microsoft.com/en-us/gaming/gdk/docs/services/multiplayer/chat/game-chat2/using-game-chat-2#processing-data-frames) · [Guest](../runtime/gamechat/guest.c) · [Original and TuxBox comparisons](../evidence/providers/platform-controls.json) |
| `xfrontpaneldisplay.dll` | 🟢 Complete | All eight front-panel exports match the supplied DLL with the panel absent or with a controlled connected transport. The tests cover 32 light masks and all 16,384 frame bytes. The lower-level `xfp_gc.dll` transport is separate. Physical hardware remains untested. [Front-panel comparisons](../evidence/providers/frontpanel-control.json) |

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

The scan command expects a package root containing `EmbeddedXvd/Windows/System32` and `Mount/Frosty/Binaries/XSX/Frosty-XSX-Shipping.exe`. It writes the raw PE inventory, adds new candidates to the coverage map as incomplete, and preserves reviewed provider records.

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
  --only xaudio --only mfreadwrite --only frontpanel --output build/provider-results.json
python tools/check_aac_provider.py --proton "/path/to/GE-Proton" \
  --prefix build/aac-prefix --output build/aac-results.json
```

To time the XG format checks, run `python tuxbox.py run build/xg-provider/xg-texture.exe --bench-formats timing.txt`. The guest writes seven timings beside the executable after one warm-up. On a Ryzen 7 9800X3D under Proton Experimental the median was 39.1 ms for TuxBox and 219.9 ms for the supplied DLL. These timings cover the 41-format address and validation workload. [Measurements](../evidence/providers/xg-formats.json)

The AAC check loads the TuxBox transform directly and compares six generated tones channel by channel with FFmpeg. Pass `--video /path/to/movie.mp4` to check the first 96 packets of a 48 kHz 5.1 AAC-LC track. The recorded Frosty run used its staged graphics and platform providers and packaged media decoders. [Generated audio results](../evidence/providers/aac-ge.json) · [Retail setup](MATRIX_AWAKENS.md)
