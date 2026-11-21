# Xbox library translation

This baseline covers 16 Xbox-facing libraries reviewed during *The Matrix Awakens* bring-up. A library's color describes its **replacement coverage**. The provider column identifies the code that supplied the observed behavior. A movie, input device or texture query can work through an external DLL while its TuxBox replacement is still incomplete. [Functionality and library map](progress.svg) · [Machine-readable records and evidence](../evidence/library-status.json)

| Status | Libraries | Criteria |
| --- | ---: | --- |
| 🟢 Complete | 0 | A replacement implements and verifies every identified console export and interface for the reviewed build. |
| 🟡 Partial | 10 | A project provider or named reference bridge implements at least one verified Xbox-facing operation. Other operations remain open. |
| 🔴 Incomplete | 6 | Xbox-specific replacement behavior remains unverified. The baseline may use the host or an original supplied library. |

## Graphics and memory

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `d3d12_xs.dll` | 🟡 Partial | The TuxBox Rust D3D12 provider supports the `0x20002` profile. Frosty's `0x20004` retail run uses patched TomBox. Movies render, but the real-time chase remains near-black. [Native device and frame tests](../runtime/d3d12/README.md) · [Retail run](../evidence/matrix-awakens/retail-chase-cinematic.json) |
| `dxcompiler_xs.dll` | 🟡 Partial | The TuxBox PE forwarder routes `DxcCreateInstance` to desktop DXC. A utility blob preserved all 257 bytes after its source was released. Three other console factories are outstanding. [Forwarder](../runtime/dxc/dxcompiler_xs.def) · [DXC test](../evidence/matrix-awakens/valve-dxc-utils.json) |
| `xmem.dll` | 🟡 Partial | The TuxBox Rust provider implements six exports for allocation, release, working-set statistics and virtual memory. Frosty's physical-page allocation and mapping calls remain open. Its retail reference run uses TomBox. [Provider and ABI tests](../runtime/xmem/README.md) |
| `xg_xs.dll` | 🔴 Incomplete | The **supplied console DLL** calculated four RGBA8 and BC7 texture layouts using TuxBox allocation. A replacement XG texture computer is outstanding. [Texture-layout guest](../runtime/xg/guest.cpp) · [Run results](../evidence/valve-nvidia.json) |

## Platform and storage

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `xgameruntime.dll` | 🟡 Partial | The ScarlettX reference runtime takes Frosty into Unreal startup. A task-queue control verifies termination. Several user and service interface queries remain unresolved. [GDK runtime results](../evidence/matrix-awakens/reference-runtime.json) |
| `xcrdapi.dll` | 🟡 Partial | TomBox bridges `DStorageGetFactory` to desktop DirectStorage. Frosty initialized its I/O dispatcher and mounted its IoStore container. Further console-specific storage operations need validation. [Storage run](../evidence/matrix-awakens/reference-runtime.json) |
| `gameconfighelper.dll` | 🔴 Incomplete | The supplied configuration helper loaded in the embedded-runtime trial. A replacement for its configuration and package-identity queries remains open. [Loader trials](../evidence/matrix-awakens-launch.json) |

## Input and audio

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `GameInput.dll` | 🟡 Partial | The TomBox bridge with TuxBox desktop mappings handled keyboard, mouse and an XInput device in the retail menu. Linux uinput supplied the Xbox controller for analog and hotplug tests. Additional device classes and physical controllers need coverage. [Retail menu](../evidence/matrix-awakens/retail-mixed-input.json) · [Controller test](../evidence/matrix-awakens/xinput-desktop.json) |
| `AcpHal.dll` | 🟡 Partial | The TomBox audio provider let Frosty start its mixer and submit an eight-channel buffer. The retail import trace still reports `AcpHalReleaseShapeContexts` unresolved. [Mixer control](../evidence/matrix-awakens/frosty-engine-control.json) · [Import trace](../evidence/matrix-awakens-launch.json) |
| `XAudio2_9.dll` | 🔴 Incomplete | Proton supplies XAudio2. The title submitted a mixer buffer, and a stereo loopback captured the intro soundtrack. Xbox-specific XAudio2 behavior still needs independent coverage. [Audio results](../evidence/matrix-awakens-launch.json) |
| `GameChat2.dll` | 🔴 Incomplete | Frosty declares 15 delay imports from the supplied title library. A chat provider and a working voice session remain open. [Import inventory](../evidence/matrix-awakens-launch.json) |

## Media

| Library | Status | Provider and observed behavior |
| --- | --- | --- |
| `mfplat.dll` | 🟡 Partial | A TuxBox sample bridge combines TomBox and Proton Media Foundation. D3D12 video-sample readback and captured retail movie frames validate part of the platform. [Sample tests](../evidence/matrix-awakens/media-valve.json) · [Movie comparisons](../evidence/matrix-awakens/retail-chase-cinematic.json) |
| `mfhevcdec.dll` | 🟡 Partial | A TuxBox transform adapter delivers synchronized D3D12 NV12 samples around the supplied HEVC decoder. Intro and chase frames were compared with a reference decode. [Intro](../evidence/matrix-awakens/hevc-valve.json) · [Chase](../evidence/matrix-awakens/wake-hevc-valve.json) |
| `mfh264dec.dll` | 🟡 Partial | A TuxBox transform adapter exposes the supplied H.264 decoder to the game. Both 1024 × 512 menu previews returned D3D12 samples. Other output formats remain unverified. [H.264 results](../evidence/matrix-awakens/h264-valve.json) |
| `mfreadwrite.dll` | 🔴 Incomplete | The Proton source reader opens movie byte streams and obtains their video media types. Console source-reader behavior still relies on the host. [Source-reader control](../evidence/matrix-awakens/media-valve.json) |
| `MSAudDecMFT.dll` | 🔴 Incomplete | Registering the supplied decoder produced six-channel PCM for 96 AAC access units. TuxBox still requires the external codec. [AAC comparison](../evidence/matrix-awakens/aac-valve.json) |

The separate Scarlett package inventory includes game middleware, redistributable runtimes and GPU helpers. All 16 supplied DLLs loaded on Proton, and selected upscaler and BVH calls also passed behavioral tests. Those results track package compatibility outside the Xbox platform-library counts above. [Library loads](../evidence/scarlett-all-library-load.json) · [Functional checks](../evidence/scarlett-library-controls.json)
