# Xbox memory provider

The [Rust provider](src/lib.rs) implements two CPU allocation forms. `XMemAlloc` and `XMemFree` (ordinals 1 and 6) use the default allocation path when no hooks are installed. The default exports retain ordinals 2 and 7. Both pairs passed the alignment, write and release checks on Proton Experimental and GE-Proton. Custom allocator hooks and GPU-visible mappings are pending. [Console export inventory](../../evidence/library-status.json) · [Valve test](../../evidence/providers/xmem-valve.json) · [GE test](../../evidence/providers/xmem-ge.json)

| Attributes | Guest request | CPU backing | Verified alignment |
| --- | ---: | --- | ---: |
| `0x0480009e` | 12 bytes | Process heap | 16 bytes |
| `0x30804000` | 65,536 bytes | A committed virtual memory reservation | 64 KiB |

Both size/attribute pairs were observed in the fork's Coin Run allocation log. Its retail analysis independently recorded 469 texture allocations using the second attribute word. Win64 supplies 16-byte heap alignment and aligns new reservations to the system allocation granularity. Returning a CPU address for the second form establishes only CPU access and alignment. Binding the allocation to a GPU resource needs a separate test. [Previous homebrew allocator call](https://github.com/brcly/TomBox/blob/ca6cf6a6b44e36d3ca4420db55a660a7bc6d6d5a/crates/compat/xmem/examples/coin/game.rs) · [Title trace](https://github.com/brcly/TomBox/blob/ca6cf6a6b44e36d3ca4420db55a660a7bc6d6d5a/docs/DEVELOPMENT_LOG.md) · [Windows heap alignment](https://learn.microsoft.com/en-us/windows/win32/api/heapapi/nf-heapapi-heapalloc) · [Windows virtual memory reservations](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualalloc)

The packaged HEVC, H.264 and AAC decoders request the process heap with attributes `0x04820000`. The allocator guest checks this form and the packaged decoder controls compare real video frames and PCM output with FFmpeg. [Independent media controls](../../evidence/matrix-awakens/independent-media.json)

The packaged D3D12 drivers also use 16-byte-aligned process heap allocations for initialization (`0x04800092`), blob objects (`0x04800040`), and writable blob data (`0x04800078`). Both drivers created and released blobs with the TuxBox allocator on Linux. [D3D12 control runs](../../evidence/providers/d3d12-exports.json)

Callers must supply the original attributes when releasing a live allocation. A zero-byte request returns null. Unknown attribute words return null with `ERROR_NOT_SUPPORTED`. Freeing null is a no-op.

Frosty requests `XMemGetWorkingSetStatistics` (ordinal 16) with selector `0x20000001` during static initialization. Its 120-byte result uses the process commit counters and host-available memory to estimate a title budget. The guest checks the result against host measurements after touching an 8 MiB allocation.

Frosty's CPU binned allocator calls `XMemVirtualAlloc` (ordinal 31) with profile 1. The fourth argument selects the profile and the fifth supplies page protection. The provider maps reservation, commit and top-down placement to Windows. The guest checks that a committed page is writable and its reserved neighbors remain inaccessible. Console-specific GPU page flags still need a separate implementation. [Virtual memory provider](src/virtual_memory.rs)

Frosty uses `XMemAllocatePhysicalPages` (ordinal 3), `XMemMapPhysicalPages` (21), and `XMemFreePhysicalPages` (8) with 64 KiB pages and allocation flags `0x20400000`. TuxBox gives each page an owned token, saves its bytes when unmapped, and restores them at the next mapping. The guest remaps two pages in reversed order, checks every byte, and checks that stale, duplicate and foreign mappings are rejected. Run `python tuxbox.py verify --only xmem` to exercise both memory paths. [Page implementation](src/physical_pages.rs) · [Guest results](../../evidence/providers/xmem-pages.json)

These checks cover the CPU path. The packaged DLL stopped before the guest started because its `xbthunk.dll` dependency uses a privilege operation unavailable on either tested Proton build. GPU access, simultaneous aliases, Xbox partition allocation and retail use of the replacement pages remain open. [Package ABI and title call sites](../../evidence/matrix-awakens/xmem-physical-pages.json)

## Build and run

Install the Rust toolchain and add the `x86_64-pc-windows-msvc` target using rustup. From the repository root on x64 Linux, use Python, `clang-cl`, `lld-link` and Windows SDK/CRT libraries installed via xwin. The default SDK root is `~/.cache/cargo-xwin/xwin`. Set `XWIN_SDK_ROOT` to use another installation. On native Windows, run the same Python build in an **x64 Native Tools Command Prompt** with a recent MSVC toolset and Windows SDK.

```sh
python build.py xmem
```

`cargo build --release -p tuxbox-xmem` builds the Rust archive. The Python build links it into the PE DLL and builds the independent C guest. The earlier `python runtime/xmem/build.py` command runs the same target. The executable and app-local DLL are in `build/xmem-smoke`. The smoke test checks the actual provider path, keeps both allocations live, writes and verifies all 65,548 bytes, checks alignment and checks that the released small block fails heap validation and observes the virtual reservation returning to the free state. [Heap validation](https://learn.microsoft.com/en-us/windows/win32/api/heapapi/nf-heapapi-heapvalidate) The guest also checks zero-size and unsupported-attribute results.

On Windows, from the build directory:

```bat
xmem-smoke.exe --report xmem-result.txt
```

On x86-64 Linux, from the build directory, select an installed Proton distribution and an isolated writable prefix:

```sh
WINEPREFIX="$HOME/.local/share/project-tuxbox/xmem-prefix" \
PROTONPATH="/path/to/proton-directory" umu-run "$PWD/xmem-smoke.exe" --report xmem-result.txt
```

A passing guest exits zero and writes `xmem-default: PASS (allocator pairs 1/6 and 2/7, ordinary and 64 KiB allocations, host working set v2, virtual reserve/commit)` to both stdout and the optional result file. To audit the binary boundary, run `llvm-readobj --coff-imports xmem-smoke.exe` and `llvm-readobj --coff-exports xmem.dll` in the same directory. Record native Windows and individual Proton providers separately. [UMU launch variables](https://github.com/Open-Wine-Components/umu-launcher#usage)
