# Xbox library translation

This baseline covers 16 Xbox-facing libraries reviewed during *The Matrix Awakens* bring-up. A library's color describes its **replacement coverage**. The provider column identifies the code that supplied the observed behavior. A movie, input device or texture query can work through an external DLL while its TuxBox replacement is still incomplete. [Functionality and library map](progress.svg) · [Machine-readable records and evidence](../evidence/library-status.json)

| Status | Libraries | Criteria |
| --- | ---: | --- |
| 🟢 Complete | 0 | A replacement implements and verifies every identified console export and interface for the reviewed build. |
| 🟡 Partial | 16 | A project provider or named reference bridge implements at least one verified Xbox-facing operation. Other operations remain open. |
| 🔴 Incomplete | 0 | Xbox-specific replacement behavior remains unverified. The baseline may use the host or an original supplied library. |

## Graphics and memory

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `d3d12_xs.dll` | 🟡 Partial | The TuxBox Rust D3D12 provider supports the `0x20002` profile. Frosty's `0x20004` retail run uses patched TomBox. Movies render, but the real-time chase remains near-black. [Native device and frame tests](../runtime/d3d12/README.md) · [Retail run](../evidence/matrix-awakens/retail-chase-cinematic.json) |
| `dxcompiler_xs.dll` | 🟡 Partial | The TuxBox PE forwarder routes `DxcCreateInstance` to desktop DXC. A utility blob preserved all 257 bytes after its source was released. Three other console factories are outstanding. [Forwarder](../runtime/dxc/dxcompiler_xs.def) · [DXC test](../evidence/matrix-awakens/valve-dxc-utils.json) |
| `xmem.dll` | 🟡 Partial | The TuxBox Rust provider implements six exports for allocation, release, working-set statistics and virtual memory. Frosty's physical-page allocation and mapping calls remain open. Its retail reference run uses TomBox. [Provider and ABI tests](../runtime/xmem/README.md) |
| `xg_xs.dll` | 🟡 Partial | TuxBox calculates linear and 4 KiB tiled layouts for single-slice 2D textures. It matched the supplied DLL on 13 layouts and 273,263 texel or block addresses, including four texture shapes from Frosty. Arrays, more formats and texture copies remain open. [Layout comparison](../evidence/providers/xg-control.json) · [Frosty shapes](../evidence/providers/xg-frosty-shapes.json) |

## Platform and storage

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `xgameruntime.dll` | 🟡 Partial | The ScarlettX reference runtime takes Frosty into Unreal startup. A task-queue control verifies termination. Several user and service interface queries remain unresolved. [GDK runtime results](../evidence/matrix-awakens/reference-runtime.json) |
| `xcrdapi.dll` | 🟡 Partial | TomBox bridges `DStorageGetFactory` to desktop DirectStorage. Frosty initialized its I/O dispatcher and mounted its IoStore container. Further console-specific storage operations need validation. [Storage run](../evidence/matrix-awakens/reference-runtime.json) |
| `gameconfighelper.dll` | 🟡 Partial | TuxBox builds a package’s registry path and opens its configuration key. Both machine and user hives matched the Windows Game Runtime helper. Frosty’s embedded runtime still needs `GetCurrentPackageGameConfig`. [Registry tests](../runtime/gameconfig/guest.c) · [Control results](../evidence/providers/platform-controls.json) |

## Input and audio

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `GameInput.dll` | 🟡 Partial | The TomBox bridge with TuxBox desktop mappings handled keyboard, mouse and an XInput device in the retail menu. Linux uinput supplied the Xbox controller for analog and hotplug tests. Additional device classes and physical controllers need coverage. [Retail menu](../evidence/matrix-awakens/retail-mixed-input.json) · [Controller test](../evidence/matrix-awakens/xinput-desktop.json) |
| `AcpHal.dll` | 🟡 Partial | The TomBox audio provider let Frosty start its mixer and submit an eight-channel buffer. The retail import trace still reports `AcpHalReleaseShapeContexts` unresolved. [Mixer control](../evidence/matrix-awakens/frosty-engine-control.json) · [Import trace](../evidence/matrix-awakens-launch.json) |
| `XAudio2_9.dll` | 🟡 Partial | TuxBox maps seven Xbox exports to system XAudio2, including all five Frosty imports at startup. The test creates three effects and measures a source playing first on the left, then on the right. XMA playback and three factory exports remain open. [Stereo and export comparisons](../evidence/providers/xaudio-control.json) |
| `GameChat2.dll` | 🟡 Partial | TuxBox can start a chat manager, add and remove remote users, and change the encoding bitrate. Seven of Frosty’s 15 chat imports are covered. Voice capture and network chat remain open. [Roster tests](../runtime/gamechat/guest.c) · [Control results](../evidence/providers/platform-controls.json) |

## Media

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `mfplat.dll` | 🟡 Partial | A TuxBox sample bridge combines TomBox and Proton Media Foundation. D3D12 video-sample readback and captured retail movie frames validate part of the platform. [Sample tests](../evidence/matrix-awakens/media-valve.json) · [Movie comparisons](../evidence/matrix-awakens/retail-chase-cinematic.json) |
| `mfhevcdec.dll` | 🟡 Partial | A TuxBox transform adapter delivers synchronized D3D12 NV12 samples around the supplied HEVC decoder. Intro and chase frames were compared with a reference decode. [Intro](../evidence/matrix-awakens/hevc-valve.json) · [Chase](../evidence/matrix-awakens/wake-hevc-valve.json) |
| `mfh264dec.dll` | 🟡 Partial | A TuxBox transform adapter exposes the supplied H.264 decoder to the game. Both 1024 × 512 menu previews returned D3D12 samples. Other output formats remain unverified. [H.264 results](../evidence/matrix-awakens/h264-valve.json) |
| `mfreadwrite.dll` | 🟡 Partial | TuxBox reads and seeks in uncompressed PCM WAV files. A guest checked the exact stereo samples, timestamps and end of stream. Frosty’s MP4 movies still need the existing video reader. [Source-reader tests](../runtime/mfreadwrite/guest.cpp) · [Run results](../evidence/providers/valve.json) |
| `MSAudDecMFT.dll` | 🟡 Partial | TuxBox adapts GE-Proton’s AAC decoder. It produced 98,304 frames of six-channel PCM from 96 Frosty soundtrack packets and matched FFmpeg within 0.000016 per sample. Proton Experimental failed to decode the same audio format. [Frosty audio](../evidence/providers/aac-frosty-ge.json) · [Experimental result](../evidence/providers/aac-valve.json) |

The separate Scarlett package inventory includes game middleware, redistributable runtimes and GPU helpers. All 16 supplied DLLs loaded on Proton, and selected upscaler and BVH calls also passed behavioral tests. Those results track package compatibility outside the Xbox platform-library counts above. [Library loads](../evidence/scarlett-all-library-load.json) · [Functional checks](../evidence/scarlett-library-controls.json)

## Run the provider checks

Build the DLLs and run the five guest tests on Linux with a stereo audio output. These tests use generated inputs and can run from this checkout alone. The AAC comparison also needs FFmpeg and a GE-Proton installation. [Build requirements](../examples/lanes/README.md#build-requirements) · [Proton Experimental results](../evidence/providers/valve.json) · [GE-Proton results](../evidence/providers/ge.json)

```sh
python build.py
python tuxbox.py verify --only xg-provider --only gameconfig --only gamechat \
  --only xaudio --only mfreadwrite --output build/provider-results.json
python tools/check_aac_provider.py --proton "/path/to/GE-Proton" \
  --prefix build/aac-prefix --output build/aac-results.json
```

The AAC check loads the TuxBox transform directly and compares six generated tones channel by channel with FFmpeg. Pass `--video /path/to/movie.mp4` to check the first 96 packets of a 48 kHz 5.1 AAC-LC track. The measured Frosty run still uses its existing graphics, platform and media setup. [Generated audio results](../evidence/providers/aac-ge.json) · [Retail setup](MATRIX_AWAKENS.md)
