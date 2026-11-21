# Xbox-facing D3D12

The app-local `d3d12_xs.dll` provides device creation for one Xbox caller profile and forwards root-signature serialization to desktop D3D12. Three original x64 guest executables exercise these paths on a selected hardware adapter.

The Rust device factory validates the guest creation record and exports the device GUID as data. Each guest COM object holds its native D3D12 interface. The command queue translates guest objects to native handles and presents through DXGI. [Device and resource slots](src/device.rs) · [Queue and command list](src/commands.rs) · [Presentation](src/present.rs)

## Guest devices and frames

A retail executable imports `D3D12XboxCreateDevice` and passes a 64-byte creation record with SDK value `0x20002`. Its call sites select device slot 8 for a non-compute queue, slot 48 for a compute queue and slot 73 for a graphics command list. The fixture sets the two title-dependent creation fields to zero and supplies a DXGI adapter explicitly so each run can verify the selected GPU. The recorded retail factory call supplies a null adapter. Other SDK versions and option combinations need their own evidence. [Static call sites and executable hash](../../evidence/fortnite-device.json)

| Suite | What the guest checks |
|---|---|
| `guest-frame` | `xbox-frame.exe` requests a direct queue at slot 8 and a compute queue at slot 48. It signals and waits on a separate compute fence. On the direct queue it clears one 37 × 13 RGBA8 texture, snapshots it, then clears a 208-pixel rectangle and snapshots it again. After the second submission completes, it compares every channel of all 962 pixels and checks three guard regions. |
| `guest-frame` | `xbox-frame-negative.exe` creates two guest devices on one adapter. It checks rejection of foreign allocators and fences, invalid copies, a mixed-device batch and an open list. A valid copy and completed fence after a failed recording verify list recovery. |
| `present` | `xbox-frame.exe --present` also submits the texture via queue slot 30 (`PresentX`) after each clear. A single-plane RGBA8 copy feeds a double-buffered DXGI swap chain. The guest samples the corner and center of the displayed window on both frames, independently of its GPU readback. |

The guest-frame and presentation suites inspect the guest executables' PE imports before launch. Each must request the Xbox factory from the adjacent DLL and leave desktop D3D12 device creation to the provider. Passing results apply to the homebrew calls exercised on that adapter and provider. [Run results](../../docs/RESULTS.md)

The same guest binaries exercised the Rust provider and its C++ predecessor on Linux. Both passed the 962-pixel readback, command-error checks and Lanes replay on NVIDIA and AMD. Both displayed two frames on NVIDIA with Proton Experimental. On the isolated Xvfb display, AMD PresentX returned `0x80070057`, and GE-Proton on NVIDIA sampled incorrect colors intermittently with both providers. [Provider hashes, exports and paired results](../../evidence/d3d12-rust-comparison.json)

## Root-signature serialization

The `graphics-abi` suite runs `d3d12-root-smoke.exe`. Its `D3D12SerializeRootSignature` import resolves through a PE forwarder in `d3d12_xs.dll`. The fixture supplies four root constants at `b5, space2` and a root UAV at `u3, space4`. A desktop deserializer checks the version 1.0 descriptor layout and bindings. The guest uses desktop device and command-list interfaces to dispatch an original DXIL compute shader twice. It checks 48 output words and 16 guards. [Shader](affine.hlsl) · [Microsoft serialization contract](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-d3d12serializerootsignature) · [Scarlett import inventory](https://github.com/brcly/TomBox/blob/ca6cf6a6b44e36d3ca4420db55a660a7bc6d6d5a/tools/progress/progress.json)

## Build and verify

Use an x64 Visual Studio developer prompt on Windows, or clang-cl, lld-link and the xwin SDK on Linux. Put the host's `dxc` executable on PATH, set `DXC`, or pass `--dxc PATH` to the build. Run from the repository root. [Desktop build setup](../../graphics/README.md) · [DXC releases](https://github.com/microsoft/DirectXShaderCompiler/releases)

```sh
python build.py d3d12
python tuxbox.py verify --only graphics-abi --only guest-frame --output build/d3d12-results.json
```

The presentation check needs a visible, unobscured window. On Linux, use an isolated Xvfb display with a hardware Vulkan driver that supports presentation to it. Use a dedicated Proton prefix for the display so an existing Wine session cannot attach the guest to another desktop.

```sh
xvfb-run -a -s "-screen 0 1280x720x24" python tuxbox.py verify --only present --prefix build/present-prefix --output build/present-results.json
```

On Windows, run the same suite on the interactive desktop with `python tuxbox.py verify --backend native --only present --output build/present-results.json`. Add `--adapter VVVV:DDDD` to any verification command to select a GPU by hexadecimal PCI vendor and device IDs. The JSON records the provider, binary hashes, exit status and receipt for each gate.
