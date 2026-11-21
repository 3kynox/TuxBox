# DXC utility forwarding

Frosty imports `DxcCreateInstance` from `dxcompiler_xs.dll`. The PE forwarder routes that export to the x64 desktop `dxcompiler.dll`. The CPU fixture requests `CLSID_DxcUtils` and calls `IDxcUtils::CreateBlob` with a binary input containing every byte value. It overwrites and frees the source, releases the utility and original blob interface, then checks the size and reads and writes all 257 bytes through a retained `IDxcBlob`.

Run `python3 build.py dxc-utils` with the project's x64 Windows SDK toolchain. Extract `bin/x64/dxcompiler.dll` from the Microsoft Windows DXC release into `build/dxc-utils/` next to the guest and forwarder. The verified DLL is version 1.8.2505.32, SHA-256 `5888d3e590f5bfd8743484e7f31313996734adec5741506271a5c6368cbe8ba9`. [Microsoft DXC release v1.8.2505.1](https://github.com/microsoft/DirectXShaderCompiler/releases/tag/v1.8.2505.1)

On Windows, run `build\dxc-utils\dxc-utils.exe`. On Linux, use the project launcher with your Proton path.

```sh
python3 tuxbox.py verify --only dxc-utils --proton "/path/to/Proton" \
    --prefix build/prefix-dxc --output build/dxc-utils-results.json
```

The verifier records the guest receipt, exit status and hashes of both DLLs. Full verification includes this suite when the desktop DLL is staged. A passing run establishes the factory and binary blob contract used by this fixture. Shader compilation and retail execution require separate checks.
