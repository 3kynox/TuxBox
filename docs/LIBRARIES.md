# Xbox library translation

The map covers the 16 Xbox-facing DLLs reviewed during *The Matrix Awakens* bring-up. Each bar shows how many exports TuxBox has verified out of all exports in that build of the DLL. Run `python tuxbox.py status --libraries` to view the bars and percentages in your terminal. [Progress bars](progress.svg) · [Counts and evidence](../evidence/library-status.json)

An export counts once when a test exercises it through a TuxBox provider. Totals include callable functions and exported data. For example, the reviewed D3D12 DLL exposes 22 functions and 32 data symbols. The terminal table shows both counts. A name and ordinal for the same export share one slot. The library color also accounts for methods on objects returned by a factory and behavior supplied by reference runtimes. A DLL can reach 100% of its exports while some methods still need work. To check the totals against your copy of the reviewed DLLs, run `python tools/check_library_exports.py /path/to/package` with the directory containing `Mount` and `EmbeddedXvd`.

| Status | Libraries | Criteria |
| --- | ---: | --- |
| 🟢 Complete | 0 | A replacement implements and verifies every identified console export and interface for the reviewed build. |
| 🟡 Partial | 16 | A project provider or named reference bridge implements at least one verified Xbox-facing operation. Other operations remain open. |
| 🔴 Incomplete | 0 | Xbox-specific replacement behavior remains unverified. The baseline may use the host or an original supplied library. |

## Graphics and memory

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `d3d12_xs.dll` | 🟡 Partial | TuxBox creates a device for the `0x20002` profile and reads and writes standard 1.0 and 1.1 root signatures. Thirty exported identifiers match the reference DLL byte for byte. Frosty’s `0x20004` profile still uses patched TomBox. [Device tests](../runtime/d3d12/README.md) · [Root signatures](../evidence/providers/d3d12-signatures.json) · [Identifiers](../evidence/providers/d3d12-guids.json) |
| `dxcompiler_xs.dll` | 🟡 Partial | TuxBox routes `DxcCreateInstance` and `DxcCreateInstance2` to desktop DXC. Tests verify that a blob retains its 257 bytes and that the second factory uses and releases a caller-supplied allocator. The two ray-tracing compiler factories remain open. [Blob test](../evidence/matrix-awakens/valve-dxc-utils.json) · [Allocator test](../evidence/providers/dxc-allocator.json) |
| `xmem.dll` | 🟡 Partial | Six exports cover two allocator pairs, working-set statistics and virtual memory. Both pairs passed guest writes, alignment and release checks for ordinary and 64 KiB-aligned allocations. Frosty’s retail run still uses TomBox for physical pages. [Memory tests](../runtime/xmem/README.md) · [Run results](../evidence/providers/xmem-valve.json) |
| `xg_xs.dll` | 🟡 Partial | TuxBox calculates linear and 4 KiB tiled layouts for single-slice 2D textures. Across 41 formats it matched the supplied DLL at 6,231,134 texel or block addresses. A separate test covers four texture shapes from Frosty. Arrays, additional formats and texture copies remain open. [Format comparison](../evidence/providers/xg-formats.json) · [Frosty shapes](../evidence/providers/xg-frosty-shapes.json) |

## Platform and storage

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `xgameruntime.dll` | 🟡 Partial | The ScarlettX reference runtime takes Frosty into Unreal startup. A task-queue control verifies termination. Several user and service interface queries remain unresolved. [GDK runtime results](../evidence/matrix-awakens/reference-runtime.json) |
| `xcrdapi.dll` | 🟡 Partial | TomBox bridges `DStorageGetFactory` to desktop DirectStorage. Frosty initialized its I/O dispatcher and mounted its IoStore container. Further console-specific storage operations need validation. [Storage run](../evidence/matrix-awakens/reference-runtime.json) |
| `gameconfighelper.dll` | 🟡 Partial | TuxBox builds package registry paths and opens both package and machine-wide Game Core configuration keys with the requested access. The tests cover both registry hives and read-only access. Frosty’s embedded runtime still needs `GetCurrentPackageGameConfig`. [Registry tests](../runtime/gameconfig/guest.c) · [Control results](../evidence/providers/platform-controls.json) |

## Input and audio

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `GameInput.dll` | 🟡 Partial | The TomBox bridge with TuxBox desktop mappings handled keyboard, mouse and an XInput device in the retail menu. Linux uinput supplied the Xbox controller for analog and hotplug tests. Additional device classes and physical controllers need coverage. [Retail menu](../evidence/matrix-awakens/retail-mixed-input.json) · [Controller test](../evidence/matrix-awakens/xinput-desktop.json) |
| `AcpHal.dll` | 🟡 Partial | The TomBox audio provider let Frosty start its mixer and submit an eight-channel buffer. The retail import trace still reports `AcpHalReleaseShapeContexts` unresolved. [Mixer control](../evidence/matrix-awakens/frosty-engine-control.json) · [Import trace](../evidence/matrix-awakens-launch.json) |
| `XAudio2_9.dll` | 🟡 Partial | TuxBox maps eight Xbox exports to system XAudio2, including all five Frosty imports at startup. Tests measure left and right output from the 2.9 and versioned factories. The 2.8 reverb factory, shared-context factory and XMA playback remain open. [Stereo and export comparisons](../evidence/providers/xaudio-control.json) |
| `GameChat2.dll` | 🟡 Partial | Eleven TuxBox exports cover chat sessions, bitrate settings and remote users. Each user has an Xbox ID and a separate caller-defined context. Voice capture and network chat remain open. [Roster tests](../runtime/gamechat/guest.c) · [Control results](../evidence/providers/platform-controls.json) |

## Media

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `mfplat.dll` | 🟡 Partial | A TuxBox sample bridge combines TomBox and Proton Media Foundation. D3D12 video-sample readback and captured retail movie frames validate part of the platform. [Sample tests](../evidence/matrix-awakens/media-valve.json) · [Movie comparisons](../evidence/matrix-awakens/retail-chase-cinematic.json) |
| `mfhevcdec.dll` | 🟡 Partial | A TuxBox transform adapter delivers synchronized D3D12 NV12 samples around the supplied HEVC decoder. Intro and chase frames were compared with a reference decode. [Intro](../evidence/matrix-awakens/hevc-valve.json) · [Chase](../evidence/matrix-awakens/wake-hevc-valve.json) |
| `mfh264dec.dll` | 🟡 Partial | A TuxBox transform adapter exposes the supplied H.264 decoder to the game. Both 1024 × 512 menu previews returned D3D12 samples. Other output formats remain unverified. [H.264 results](../evidence/matrix-awakens/h264-valve.json) |
| `mfreadwrite.dll` | 🟡 Partial | TuxBox reads PCM WAV audio from byte streams, local paths and file URLs. Tests check stereo samples, timestamps, seeking, end of stream and file-handle release. Frosty’s MP4 movies still need the existing video reader. [Source-reader tests](../runtime/mfreadwrite/guest.cpp) · [Path and URL comparisons](../evidence/providers/mfreadwrite-url-control.json) |
| `MSAudDecMFT.dll` | 🟡 Partial | TuxBox adapts GE-Proton’s AAC decoder. It produced 98,304 frames of six-channel PCM from 96 Frosty soundtrack packets and matched FFmpeg within 0.000016 per sample. Proton Experimental failed to decode the same audio format. [Frosty audio](../evidence/providers/aac-frosty-ge.json) · [Experimental result](../evidence/providers/aac-valve.json) |

The separate Scarlett package inventory includes game middleware, redistributable runtimes and GPU helpers. All 16 supplied DLLs loaded on Proton, and selected upscaler and BVH calls also passed behavioral tests. Those results track package compatibility outside the Xbox platform-library counts above. [Library loads](../evidence/scarlett-all-library-load.json) · [Functional checks](../evidence/scarlett-library-controls.json)

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

The AAC check loads the TuxBox transform directly and compares six generated tones channel by channel with FFmpeg. Pass `--video /path/to/movie.mp4` to check the first 96 packets of a 48 kHz 5.1 AAC-LC track. The measured Frosty run still uses its existing graphics, platform and media setup. [Generated audio results](../evidence/providers/aac-ge.json) · [Retail setup](MATRIX_AWAKENS.md)
