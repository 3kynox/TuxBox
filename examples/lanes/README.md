# Lanes

Lanes is an original, turn-based homebrew game for Project TuxBox. Collect as many coins as possible over 16 turns and avoid the red barriers. The bottom row is the next turn. Left and Right move one lane and advance. Space advances while holding your lane. A click on a lane takes one step toward it, or holds position in the current lane. On an XInput controller, use the D-pad or left stick to move and A to hold. R or Start restarts the course, and Escape closes the window. You can also click the board to restart after a finish.

## Build requirements

Use an x86-64 PC with Python 3.10 or newer, Rust with Cargo, and the DirectX Shader Compiler (`dxc`) on your PATH. Add the Windows build target once with `rustup target add x86_64-pc-windows-msvc`. [DirectX Shader Compiler releases](https://github.com/microsoft/DirectXShaderCompiler/releases)

- **Linux** needs `clang-cl`, `lld-link` and the Windows SDK and C runtime staged with xwin. If the SDK is outside the cargo-xwin cache, set `XWIN_SDK_ROOT` to the directory containing `sdk/` and `crt/`. Install UMU (`umu-run`) and Proton Experimental to play. From the project folder, run `python3 -m venv .venv` and `source .venv/bin/activate` before installing the Python dependencies.
- **Windows** needs Visual Studio Build Tools with the C++ tools and Windows SDK. Run the commands in an **x64 Native Tools Command Prompt**.

## Build and play

Run these commands from the project folder.

```sh
python -m pip install -r requirements.txt
python build.py lanes
python tuxbox.py run --app-id Lanes build/lanes
```

The package in `build/lanes` contains the executable, two project-built DLLs, and a synthetic `MicrosoftGame.config`. The launcher defaults to Proton on x86-64 Linux and native execution on x64 Windows. Add `--backend`, `--prefix` and `--proton` to the launcher before the package path when selecting a different Linux setup. An optional guest argument `--adapter VVVV:DDDD` selects a hardware GPU by PCI IDs.

## What the game exercises

- The guest allocates the game state and its course with `xmem.dll!XMemAllocDefault` and frees it with `XMemFreeDefault`.
- At startup, an original GPU compute shader places the coin and hazard on each turn. The guest obtains its root signature through the static import `d3d12_xs.dll!D3D12SerializeRootSignature`. It binds a seed and row count and reads the resulting course into the XMem allocation. Playing further turns requires no GPU readback.
- The window uses Win32 mouse and keyboard events, XInput gamepad polling, and GDI drawing. The D3D12 device, compute command list and shader bytecode use the desktop ABI. The manifest describes a synthetic local fixture. It has no assigned commercial title identity or console certification.

## Deterministic replay

```sh
python tuxbox.py run --app-id Lanes build/lanes --replay --report replay.txt
```

The report is written in the guest directory. The replay pins the seed and checks all 16 GPU-generated rows against a fixed reference. It checks held buttons across connection and release, overlapping D-pad and A presses, and left-stick jitter. It uses the same movement function as the interactive window to collect every coin, step on an empty lane, and hit a barrier. It checks that further movement after a win or collision leaves the state unchanged, then releases the board.

A separate Linux run compared window pixels for equivalent mouse clicks, keyboard presses and XInput samples from an Xbox-style uinput controller. [Desktop input results](../../evidence/lanes-desktop-input.json)
