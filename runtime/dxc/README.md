# DXC utility forwarding

Frosty imports `DxcCreateInstance` from `dxcompiler_xs.dll`. The PE forwarder routes that export to the x64 desktop `dxcompiler.dll`. The CPU fixture requests `CLSID_DxcUtils` and calls `IDxcUtils::CreateBlob` with a binary input containing every byte value. It overwrites and frees the source, releases the utility and original blob interface, then checks the size and reads and writes all 257 bytes through a retained `IDxcBlob`.

The Xbox One variant, `dxcompiler_x.dll`, exposes the same two standard factories at ordinals 1 and 2. The Scarlett DLL exposes them at 3 and 4. Each guest imports its platform's DLL by name, checks both ordinals, and passes a counting allocator to `DxcCreateInstance2`. The compilation check builds a Shader Model 6.0 compute shader with a root signature, inspects the DXIL output, and requests a missing entry point to exercise compiler diagnostics.

Run `python3 build.py dxc-utils` with the project's x64 Windows SDK toolchain. Extract `bin/x64/dxcompiler.dll` from the Microsoft Windows DXC release into `build/dxc-utils/` next to the guest and forwarder. The verified DLL is version 1.8.2505.32, SHA-256 `5888d3e590f5bfd8743484e7f31313996734adec5741506271a5c6368cbe8ba9`. [Microsoft DXC release v1.8.2505.1](https://github.com/microsoft/DirectXShaderCompiler/releases/tag/v1.8.2505.1)

On Windows, run `build\dxc-utils\dxc-utils.exe`. On Linux, use the project launcher with your Proton path.

```sh
python3 tuxbox.py verify --only dxc-utils --proton "/path/to/Proton" \
    --prefix build/prefix-dxc --output build/dxc-utils-results.json
```

Full verification includes this suite when the desktop DLL is staged. The TuxBox forwarders generate desktop DXIL. Xbox GPU instruction generation, Xbox-specific shader intrinsics and the two Scarlett XDXR factories remain open. The paired control uses the same guest executables beside the supplied console compiler and its shader backends. [Control results](../../evidence/providers/dxc-factories.json)

The supplied Series compiler exposes two XDXR factories at ordinals 1 and 2. An original-DLL control verified object construction, COM identity, `INoMarshal`, rejected class and interface IDs, caller allocator lifetime, and release. Binary inspection identifies the object as `IXdxrCompiler` and its apparent 13-slot table; the control records slot addresses but does not call the compiler methods. Those methods and their inputs and outputs remain unimplemented and are not counted as covered. [Factory control and limitations](../../evidence/providers/xdxr-factory-abi.json)
