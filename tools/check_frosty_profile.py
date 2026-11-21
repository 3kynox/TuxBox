#!/usr/bin/env python3
r"""Verify graphics call sites and their symbols in the audited Frosty shipping build.

  python tools/check_frosty_profile.py "/path/to/MA/Mount" --output evidence/frosty-device.json

Reads the manifest, shipping PE and its matching Symbols file. Requires pefile
and llvm-objdump. Emits static facts, without paths or binary content.
"""
import argparse
from contextlib import closing
import hashlib
import json
from pathlib import Path, PureWindowsPath
import re
import struct
import subprocess
import sys
from uuid import UUID
from xml.etree import ElementTree
import zlib

import pefile

from check_frame_profile import DEVICE, QUEUE, LIST, decoded, interface, require, rip


BUILD = "ae39eb1e92fa806df189c768c1478a53dfb54a2cc9769a9f2301ca7921e3eb04"
SYMBOLS = "b886b339a77da68874a44d107e366aeb4b7c16695dd67e8bbd44d6fcbab13c2c"

DXC_UTILS_CLASS = UUID("6245d6af-66e0-48fd-80b4-4d271796748c")
DXC_UTILS_IID = UUID("4605c4cb-2019-492a-ada4-65f20bb7d67f")
DXC_FACTORIES = (0x5767C54, 0x5767CAE)
# (GDxcUtils load, input pointer -> RDX, input pointer -> RBX, CreateBlob call)
DXC_BLOB_CALLS = ((0x5753EFB, 0x5753F0D, 0x5753F10, 0x5753F16),
                  (0x57545F1, 0x5754603, 0x5754606, 0x575460C),
                  (0x5754879, 0x5754888, 0x575488B, 0x5754896))

# Factory, get_layout, plane copy stride, copy size, release.
XG_CALLS = ((0x575CCFA, 0x575CD45, 0x575CDF8, 0x575CE09, 0x575CE28),
            (0x575ED9B, 0x575EDE5, 0x575EE9D, 0x575EEB1, 0x575EED0))


def check_rows(instructions, expected):
    for rva, wanted in expected.items():
        require(instructions.get(rva) == wanted, f"Unexpected profile instruction at {rva:#x}")


def symbols(path, executable_name, sites):
    compressed = path.read_bytes()
    require(hashlib.sha256(compressed).hexdigest() == SYMBOLS, "Unknown shipping symbol file")
    size, packed_size = struct.unpack_from("<II", compressed)
    require(packed_size == len(compressed) - 8, "Unexpected compressed size")
    raw = zlib.decompress(compressed[8:])
    require(len(raw) == size, "Unexpected symbol data size")
    version, name_len = struct.unpack_from("<II", raw)
    require(version == 1 and raw[8:8+name_len] == executable_name.encode() + b"\0", "Symbol image mismatch")
    count_offset = 8 + name_len + 8
    count, = struct.unpack_from("<I", raw, count_offset)
    first = count_offset + 4
    names = first + count * 12
    require(struct.unpack_from("<I", raw, names)[0] == count, "Symbol-name count mismatch")
    covered = {}
    for i, (start, length, _) in enumerate(struct.iter_unpack("<III", memoryview(raw)[first:names])):
        for site in sites:
            if start <= site < start + length:
                require(site not in covered, "Ambiguous symbol range")
                covered[site] = (i, start)
    require(set(sites) == set(covered), "Missing symbol for a call site")
    needed = {index for index, _ in covered.values()}
    functions = {}
    pos = names + 4
    for i in range(count):
        length, = struct.unpack_from("<I", raw, pos)
        pos += 4
        require(length > 0 and pos + length <= len(raw) and raw[pos + length - 1] == 0, "Malformed symbol name")
        if i in needed:
            functions[i] = raw[pos:pos+length-1].decode("utf-8")
        pos += length
    require(pos == len(raw), "Unexpected bytes after symbol table")
    return count, {site: {"function": functions[i], "function_rva": hex(start)} for site, (i, start) in covered.items()}


def factory_record(rows):
    check_rows(rows, {
        0x5752AAB: "mov r15, rcx", 0x5752AB2: "mov dword ptr [rcx + 0x980], 0x1",
        0x5752ABC: "mov ecx, 0x1", 0x5752AC3: "mov ecx, dword ptr [rcx + 0x980]", 0x5752ACF: "xor esi, esi",
        0x5752B68: "mov eax, dword ptr [r15 + 0x980]", 0x5752B88: "mov eax, esi", 0x5752B91: "setne al",
        0x5752BB4: "lea r9, [r15 + 0x18]", 0x5752BBF: "lea rdx, [rbp - 0x80]", 0x5752BC3: "xor ecx, ecx",
    })
    require(rows.get(0x5752B8A, "").startswith("cmp byte ptr [rip + ") and rows[0x5752B8A].endswith("], sil"),
            "Expected the boolean field to be loaded from a global byte")
    fields = [None] * 18
    # The caller initializes the 72 bytes beginning at its RDX argument.
    # Obtain each offset, width and value from LLVM's decoded stores.
    for rva, row in rows.items():
        if not 0x5752B4F <= rva <= 0x5752B97:
            continue
        match = re.fullmatch(r"mov (dword|qword) ptr \[rbp - (0x[0-9a-f]+)\], (.+)", row)
        if not match:
            continue
        first, count = (0x80 - int(match[2], 16)) // 4, 1 if match[1] == "dword" else 2
        source = match[3]
        if source == "eax":
            value = "title field at +0x980 (also passed to D3D12XboxSetProcessDebugFlags)" if rva == 0x5752B6F else "global byte != 0"
        elif source in ("esi", "rsi"):
            value = 0
        else:
            value = int(source, 0) & 0xffffffff
        require(0 <= first and first + count <= len(fields) and all(v is None for v in fields[first:first+count]),
                f"Invalid record store at {rva:#x}")
        fields[first:first+count] = [value] * count
    require(None not in fields, "Incomplete device record")
    return fields


def dxc_usage(pe, llvm, path, context, factory_iat):
    global_rva = rip(pe, DXC_FACTORIES[0] - 21, b"\x4c\x8d\x05")
    for call in DXC_FACTORIES:
        # LEA R8 (output), RDX (IID), RCX (CLSID), then CALL [import].
        require(rip(pe, call - 21, b"\x4c\x8d\x05") == global_rva, "Different DXC output globals")
        interface(pe, call - 14, b"\x48\x8d\x15", DXC_UTILS_IID)
        interface(pe, call - 7, b"\x48\x8d\x0d", DXC_UTILS_CLASS)
        require(rip(pe, call, b"\xff\x15") == factory_iat, "Wrong DXC factory import")
    free_iat, = (imp.address - pe.OPTIONAL_HEADER.ImageBase for mod in pe.DIRECTORY_ENTRY_IMPORT
                 if mod.dll.lower() == b"api-ms-win-crt-heap-l1-1-0.dll" for imp in mod.imports if imp.name == b"free")
    calls = []
    for load, data, saved, call in DXC_BLOB_CALLS:
        require(rip(pe, load, b"\x48\x8b\x0d") == global_rva, "Wrong IDxcUtils receiver")
        rows = decoded(llvm, path, pe.OPTIONAL_HEADER.ImageBase, load, call + 13)
        check_rows(rows, {load + 7: "xor r9d, r9d", data: "mov rdx, rax", saved: "mov rbx, rax",
                          call - 3: "mov r10, qword ptr [rcx]", call: "call qword ptr [r10 + 0x30]",
                          call + 4: "mov rcx, rbx"})
        require(rip(pe, call + 7, b"\xff\x15") == free_iat, "Expected caller to free the source after CreateBlob")
        calls.append({"function": context[call]["function"], "call_rva": hex(call)})
    return {"import": "dxcompiler_xs.dll!DxcCreateInstance", "class": "CLSID_DxcUtils", "clsid": str(DXC_UTILS_CLASS),
            "interface": "IDxcUtils", "iid": str(DXC_UTILS_IID),
            "factory_calls": [{"call_rva": hex(call), "function": context[call]["function"]} for call in DXC_FACTORIES],
            "method": "IDxcUtils::CreateBlob", "slot": 6, "code_page": 0,
            "source_freed_after_call": True, "blob_calls": calls}


def xg_usage(pe, llvm, path, context, factory_iat):
    calls = []
    for create, get_layout, stride, size, release in XG_CALLS:
        require(rip(pe, create, b"\xff\x15") == factory_iat, "Wrong XG factory import")
        rows = decoded(llvm, path, pe.OPTIONAL_HEADER.ImageBase, create, release + 3)
        check_rows(rows, {get_layout: "call qword ptr [rax + 0x10]", stride: "imul rdx, rax, 0x6c8",
                          size: "mov r8d, 0x6c8", release: "call qword ptr [rax + 0x8]"})
        calls.append({"function": context[create]["function"], "factory_rva": hex(create),
                      "get_layout_rva": hex(get_layout), "release_rva": hex(release)})
    return {"import": "xg_xs.dll!XGCreateTextureComputer", "get_layout_slot": 2, "release_slot": 1,
            "copied_bytes_per_plane": 0x6c8, "calls": calls}


def command_signatures(pe, llvm, path):
    rows = decoded(llvm, path, pe.OPTIONAL_HEADER.ImageBase, 0x5753180, 0x57533D9)
    signatures = []
    for call, slot_load, type_store, stride_store, log_ref, purpose, arg_type, stride in (
        (0x5753222, 0x5753205, 0x57531C4, 0x57531B4, 0x575324C, "DispatchIndirectCompute", 105, 12),
        (0x57532B7, 0x575329A, 0x575325A, 0x5753261, 0x57532E1, "DispatchIndirectMesh", 107, 16),
        (0x575339A, 0x575337D, 0x575331B, 0x575330B, 0x57533C4, "DispatchRaysIndirect", 9, 104),
    ):
        check_rows(rows, {call: "call r10", slot_load: "mov r10, qword ptr [rax + 0x148]",
                          type_store: f"mov dword ptr [rbp - 0x30], {arg_type:#x}",
                          stride_store: f"mov dword ptr [rbp - 0x20], {stride:#x}"})
        diagnostic = pe.get_string_at_rva(rip(pe, log_ref, b"\x48\x8d\x15"))
        require((purpose + "CommandSignature").encode() in diagnostic, "Unexpected command signature diagnostic")
        signatures.append({"purpose": purpose, "call_rva": hex(call), "device_slot": 41,
                           "argument_type": arg_type, "byte_stride": stride})
    return signatures


def inspect(mount, llvm):
    root = ElementTree.parse(mount / "MicrosoftGame.config")
    decl, = root.findall("./{*}ExecutableList/{*}Executable")
    require(decl.get("TargetDeviceFamily") == "Scarlett", "Expected a Scarlett title")
    relative = PureWindowsPath(decl.attrib["Name"])
    require(relative.as_posix() == "Frosty/Binaries/XSX/Frosty-XSX-Shipping.exe", "Unexpected executable declaration")
    path = mount / relative.as_posix()
    with path.open("rb") as stream:
        require(hashlib.file_digest(stream, "sha256").hexdigest() == BUILD, "Unsupported Frosty executable hash")
    sites = [0x5752BC5, 0x5766CAB, 0x575291D, 0x575284A, 0x5786667, 0x5751C9B]
    symbol_sites = [*sites, *DXC_FACTORIES, *(row[3] for row in DXC_BLOB_CALLS), *(row[0] for row in XG_CALLS)]
    count, context = symbols(mount / "Symbols/Frosty-XSX-Shipping-Symbols.bin", path.name, symbol_sites)
    with closing(pefile.PE(str(path), fast_load=True)) as pe:
        require(pe.FILE_HEADER.Machine == 0x8664, "Expected AMD64 PE")
        base = pe.OPTIONAL_HEADER.ImageBase
        pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]])
        graphics = {mod.dll.decode().lower(): {imp.name.decode(): imp.address - base for imp in mod.imports}
                    for mod in pe.DIRECTORY_ENTRY_IMPORT if mod.dll.lower() in (b"d3d12_xs.dll", b"xg_xs.dll", b"dxcompiler_xs.dll")}
        for site, export in ((sites[0], "D3D12XboxCreateDevice"), (sites[1], "D3D12CreateDevice"),
                             (0x5752AC9, "D3D12XboxSetProcessDebugFlags")):
            require(graphics["d3d12_xs.dll"][export] == rip(pe, site, b"\xff\x15"), f"Wrong import for {site:#x}")
        factory = {**context[sites[0]], "call_rva": hex(sites[0]), "adapter": "null",
                   "requested_interface": "ID3D12Device", "iid_rva": interface(pe, 0x5752BB8, b"\x4c\x8d\x05", DEVICE),
                   "record_dwords": factory_record(decoded(llvm, path, base, 0x5752A70, 0x5752BCB))}
        # Resolve the read of dword 15 back to its producer. The writes have a
        # trailing 1-byte immediate after disp32, so next RIP is one byte later.
        flag = rip(pe, 0x5752B8A, b"\x40\x38\x35")
        for store, value in ((0x5751C9B, 1), (0x5751CC3, 0)):
            require(rip(pe, store, b"\xc6\x05") + 1 == flag and pe.get_data(store + 6, 1) == bytes([value]),
                    "Global factory flag does not match the RHI initialization writes")
        require(rip(pe, 0x5751CB9, b"\xe8") == 0x5771AF0, "Unexpected log target on the flag-set branch")
        message = rip(pe, 0x5771AF4, b"\x48\x8d\x05")
        expected = "AutoHDR output is enabled.\0".encode("utf-16le")
        require(pe.get_data(message, len(expected)) == expected, "Unexpected diagnostic for the flag-set branch")
        factory["dword_15_source"] = {**context[sites[5]], "flag_rva": hex(flag),
                                      "true_branch_log": "AutoHDR enabled", "log_call_rva": hex(0x5751CB9)}
        calls = []
        for site, load, slot, ref, prefix, iid, name in (
            (sites[2], 0x5752919, 8, 0x5752909, b"\x4c\x8d\x05", QUEUE, "ID3D12CommandQueue"),
            (sites[3], 0x5752843, 48, 0x5752831, b"\x4c\x8d\x05", QUEUE, "ID3D12CommandQueue"),
            (sites[4], 0x5786639, 73, 0x5786645, b"\x48\x8d\x05", LIST, "ID3D12GraphicsCommandList"),
        ):
            rows = decoded(llvm, path, base, load, site + 3)
            check_rows(rows, {load: f"mov r10, qword ptr [rax + {slot*8:#x}]", site: "call r10"})
            calls.append({**context[site], "call_rva": hex(site), "device_slot": slot, "requested_interface": name,
                          "iid_rva": interface(pe, ref, prefix, iid)})
        return {"kind": "static call sites", "package_version": root.find("./{*}Identity").attrib["Version"],
                "executable_sha256": BUILD, "shipping_symbols_sha256": SYMBOLS, "shipping_function_records": count,
                "graphics_imports": {mod: sorted(table) for mod, table in graphics.items()}, "xbox_factory": factory,
                "other_factory": {**context[sites[1]], "call_rva": hex(sites[1]), "import": "d3d12_xs.dll!D3D12CreateDevice"},
                "device_calls": calls,
                "command_signatures": command_signatures(pe, llvm, path),
                "dxc_utils": dxc_usage(pe, llvm, path, context, graphics["dxcompiler_xs.dll"]["DxcCreateInstance"]),
                "xg_texture": xg_usage(pe, llvm, path, context, graphics["xg_xs.dll"]["XGCreateTextureComputer"])}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("mount", type=Path)
    parser.add_argument("--llvm-objdump", default="llvm-objdump")
    parser.add_argument("--output", type=Path, help="write JSON after all checks pass (default: stdout)")
    args = parser.parse_args()
    try:
        result = json.dumps(inspect(args.mount, args.llvm_objdump), indent=2) + "\n"
        if args.output:
            args.output.write_text(result, encoding="utf-8")
        else:
            sys.stdout.write(result)
    except (OSError, ValueError, KeyError, ElementTree.ParseError, pefile.PEFormatError, subprocess.SubprocessError) as exc:
        parser.exit(1, f"Frosty profile check failed: {exc}\n")


if __name__ == "__main__":
    main()
