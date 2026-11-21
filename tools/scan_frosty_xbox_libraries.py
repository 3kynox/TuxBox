#!/usr/bin/env python3
"""Inventory Xbox-facing PE libraries shipped beside The Matrix Awakens."""
import argparse
from collections import deque
from contextlib import closing
import json
from pathlib import Path
import re
import sys

import pefile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from project_tuxbox.inspect import digest, image


TITLE_RELATIVE = Path("Mount/Frosty/Binaries/XSX/Frosty-XSX-Shipping.exe")
SYSTEM_RELATIVE = Path("EmbeddedXvd/Windows/System32")
MOUNT_RELATIVE = Path("Mount/Frosty/Binaries/XSX")
KNOWN_XBOX_MODULES = {
    "acphal.dll", "audiohalwrapper.dll", "connectedstorageservice.proxystub.dll",
    "d3d12_x.dll", "d3d12_xs.dll", "d3dcompiler_47.dll", "dxbc2dxil_xs.dll",
    "dxcompiler_x.dll", "dxcompiler_xs.dll", "gameconfighelper.dll",
    "gamecoreclient.dll", "gamecorecomproxy.dll", "gameinput.dll", "gcxaudioext.dll",
    "hix_xs.dll", "minappmodelx.dll", "minuserx.dll", "msave64xgs.dll",
    "newbe_xs.dll", "pixrt.dll", "pixrt10.dll", "scdxil.dll", "xapu.dll",
    "xbsc_xs.dll", "xbthunk.dll", "xbtp.dll", "xcrdapi.dll", "xfp_gc.dll",
    "xfrontpaneldisplay.dll", "xg_x.dll", "xg_xs.dll", "xgameruntime.dll",
    "xgs12.dll", "xgs12_10.dll", "xgs12_10d.dll", "xgs12_10i.dll",
    "xgs12_d.dll", "xgs12_i.dll", "xhit.dll", "xhit_s.dll", "xmem.dll",
    "werxboxgc.dll", "xvmaudioservice.dll",
    "pixevt.dll", "xtfbaseservices.dll", "xtfconsolecontrol.dll",
    "xtfdebugmonitor.dll", "xtfdiaginfo.dll", "xtfeventserver.dll",
    "xtffileio.dll", "xtfpix.dll", "xtfremoterun.dll",
}
XBOX_PLATFORM_METADATA = re.compile(r"\b(?:xbox|gamecore|durango|game os|pix|console)\b", re.IGNORECASE)

GROUPS = {
    "acphal.dll": "Input and audio",
    "audiohalwrapper.dll": "Input and audio",
    "gcxaudioext.dll": "Input and audio",
    "gameinput.dll": "Input and audio",
    "xapu.dll": "Input and audio",
    "xvmaudioservice.dll": "Input and audio",
    "d3d12_x.dll": "Graphics and memory",
    "d3d12_xs.dll": "Graphics and memory",
    "d3dcompiler_47.dll": "Graphics and memory",
    "dxbc2dxil_xs.dll": "Graphics and memory",
    "dxcompiler_x.dll": "Graphics and memory",
    "dxcompiler_xs.dll": "Graphics and memory",
    "newbe_xs.dll": "Graphics and memory",
    "scdxil.dll": "Graphics and memory",
    "xbsc_xs.dll": "Graphics and memory",
    "xg_x.dll": "Graphics and memory",
    "xg_xs.dll": "Graphics and memory",
    "xgs12.dll": "Graphics and memory",
    "xgs12_10.dll": "Graphics and memory",
    "xgs12_10d.dll": "Graphics and memory",
    "xgs12_10i.dll": "Graphics and memory",
    "xgs12_d.dll": "Graphics and memory",
    "xgs12_i.dll": "Graphics and memory",
    "xmem.dll": "Graphics and memory",
    "connectedstorageservice.proxystub.dll": "Platform and storage",
    "gameconfighelper.dll": "Platform and storage",
    "gamecoreclient.dll": "Platform and storage",
    "gamecorecomproxy.dll": "Platform and storage",
    "minappmodelx.dll": "Platform and storage",
    "minuserx.dll": "Platform and storage",
    "msave64xgs.dll": "Platform and storage",
    "xbthunk.dll": "Platform and storage",
    "xbtp.dll": "Platform and storage",
    "xcrdapi.dll": "Platform and storage",
    "xgameruntime.dll": "Platform and storage",
    "werxboxgc.dll": "Diagnostics and tooling",
    "hix_xs.dll": "Diagnostics and tooling",
    "pixrt.dll": "Diagnostics and tooling",
    "pixrt10.dll": "Diagnostics and tooling",
    "pixevt.dll": "Diagnostics and tooling",
    "pixlivemerge.dll": "Diagnostics and tooling",
    "xtfbaseservices.dll": "Diagnostics and tooling",
    "xtfconsolecontrol.dll": "Diagnostics and tooling",
    "xtfdebugmonitor.dll": "Diagnostics and tooling",
    "xtfdiaginfo.dll": "Diagnostics and tooling",
    "xtfeventserver.dll": "Diagnostics and tooling",
    "xtffileio.dll": "Diagnostics and tooling",
    "xtfpix.dll": "Diagnostics and tooling",
    "xtfremoterun.dll": "Diagnostics and tooling",
    "xhit.dll": "Diagnostics and tooling",
    "xhit_s.dll": "Diagnostics and tooling",
    "xfp_gc.dll": "Other Xbox support",
    "xfrontpaneldisplay.dll": "Other Xbox support",
    "coniosrv.dll": "Other Xbox support",
}


def _key(value):
    if isinstance(value, bytes):
        return value.decode("ascii", "backslashreplace")
    return str(value)


def version_metadata(path):
    result = {}
    with closing(pefile.PE(str(path), fast_load=True)) as pe:
        pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_RESOURCE"]])
        for file_info in getattr(pe, "FileInfo", []):
            for entry in file_info:
                if _key(getattr(entry, "Key", "")) != "StringFileInfo":
                    continue
                for table in getattr(entry, "StringTable", []):
                    for key, value in table.entries.items():
                        key = _key(key)
                        if key in {"CompanyName", "FileDescription", "FileVersion", "InternalName",
                                   "OriginalFilename", "ProductName", "ProductVersion"}:
                            result[key] = _key(value)
        fixed = getattr(pe, "VS_FIXEDFILEINFO", [])
        if fixed and "FileVersion" not in result:
            info = fixed[0]
            ms, ls = info.FileVersionMS, info.FileVersionLS
            result["FileVersion"] = f"{ms >> 16}.{ms & 0xffff}.{ls >> 16}.{ls & 0xffff}"
    return result


def export_metadata(path):
    with closing(pefile.PE(str(path), fast_load=True)) as pe:
        pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_EXPORT"]])
        directory = getattr(pe, "DIRECTORY_ENTRY_EXPORT", None)
        if directory is None:
            return {"total": 0, "named_entries": 0, "data_exports": [], "entries": []}
        table = directory.struct
        start = pe.OPTIONAL_HEADER.DATA_DIRECTORY[0].VirtualAddress
        end = start + pe.OPTIONAL_HEADER.DATA_DIRECTORY[0].Size
        aliases = {}
        for entry in directory.symbols:
            aliases.setdefault(entry.ordinal, []).append(_key(entry.name) if entry.name else None)
        data_ordinals = set()
        exports = []
        for offset in range(table.NumberOfFunctions):
            address = pe.get_dword_at_rva(table.AddressOfFunctions + 4 * offset)
            if not address:
                continue
            ordinal = table.Base + offset
            section = pe.get_section_by_rva(address)
            forwarder = start <= address < end
            if forwarder:
                kind = "forwarder"
                target = _key(pe.get_string_at_rva(address))
            elif section is not None and not section.Characteristics & 0x20000000:
                kind = "data"
                target = None
                data_ordinals.add(ordinal)
            else:
                kind = "code"
                target = None
            names = sorted(name for name in aliases.get(ordinal, []) if name)
            exports.append({"ordinal": ordinal, "names": names, "kind": kind,
                            "rva": hex(address), **({"forwarder": target} if target else {})})
        by_ordinal = {row["ordinal"]: row for row in exports}
        data_exports = []
        for ordinal in sorted(data_ordinals):
            row = by_ordinal[ordinal]
            data_exports.append(row["names"][0] if row["names"] else f"#{ordinal}")
        return {
            "total": len(exports),
            "named_entries": sum(len(row["names"]) for row in exports),
            "code_exports": sum(row["kind"] == "code" for row in exports),
            "forwarder_exports": sum(row["kind"] == "forwarder" for row in exports),
            "data_exports": data_exports,
            "entries": exports,
        }


def candidate(path, metadata, reviewed_names):
    name = path.name.casefold()
    description = " ".join(metadata.values())
    reasons = []
    if name in reviewed_names:
        reasons.append("already tracked as an Xbox-facing Frosty library")
    if name in KNOWN_XBOX_MODULES:
        reasons.append("known Xbox, Game OS, Xtf, or PIX support module name")
    if XBOX_PLATFORM_METADATA.search(description):
        reasons.append("Xbox, GameCore, Durango, Game OS, PIX, or console-support file metadata")
    return reasons


def record_module(path, package, root_label):
    info = image(path)
    exports = export_metadata(path)
    metadata = version_metadata(path)
    return {
        "name": path.name,
        "path": path.relative_to(package).as_posix(),
        "source_root": root_label,
        "sha256": info["sha256"],
        "machine": info["machine"],
        "x64_pe": info["x64_pe"],
        "kind": info["kind"],
        "metadata": metadata,
        "exports": exports,
        "imports": info.get("imports", []),
        "delay_imports": info.get("delay_imports", []),
        "selection_reasons": [],
    }


def _module_map(rows):
    result = {}
    for row in rows:
        result.setdefault(row["name"].casefold(), row)
    return result


def import_edges(rows, title):
    by_name = _module_map(rows)
    edges = []
    reverse = {}
    for row in rows:
        for table_name in ("imports", "delay_imports"):
            for item in row[table_name]:
                target = by_name.get(item["module"].casefold())
                if target is None:
                    continue
                edge = {"from": row["name"], "to": target["name"], "table": table_name,
                        "symbols": item["symbols"]}
                edges.append(edge)
                reverse.setdefault(target["name"].casefold(), []).append(edge)
    for table_name in ("imports", "delay_imports"):
        for item in title[table_name]:
            target = by_name.get(item["module"].casefold())
            if target is None:
                continue
            edge = {"from": "Frosty-XSX-Shipping.exe", "to": target["name"], "table": table_name,
                    "symbols": item["symbols"]}
            edges.append(edge)
            reverse.setdefault(target["name"].casefold(), []).append(edge)
    return edges, reverse


def title_reachability(rows, title):
    by_name = _module_map(rows)
    paths = {}
    queue = deque()
    for table_name in ("imports", "delay_imports"):
        for item in title[table_name]:
            row = by_name.get(item["module"].casefold())
            if row is None or row["name"].casefold() in paths:
                continue
            paths[row["name"].casefold()] = ["Frosty-XSX-Shipping.exe", row["name"]]
            queue.append(row)
    while queue:
        row = queue.popleft()
        for table_name in ("imports", "delay_imports"):
            for item in row[table_name]:
                dependency = by_name.get(item["module"].casefold())
                if dependency is None or dependency["name"].casefold() in paths:
                    continue
                paths[dependency["name"].casefold()] = paths[row["name"].casefold()] + [dependency["name"]]
                queue.append(dependency)
    return paths


def read_reviewed_map(path):
    return json.loads(path.read_text(encoding="utf-8"))


def sync_library_map(inventory, map_path):
    data = read_reviewed_map(map_path)
    reachability_counts = {}
    for row in inventory["title_reachability"]:
        reachability_counts[row["relation"]] = reachability_counts.get(row["relation"], 0) + 1
    data["inventory_summary"] = {
        "evidence": "evidence/frosty-xbox-library-inventory.json",
        "system_image_dlls_scanned": inventory["scan_summary"]["system_image_dlls_found"],
        "title_mount_dlls_scanned": inventory["scan_summary"]["title_mount_dlls_found"],
        "catalogued_xbox_facing_dlls": inventory["scan_summary"]["xbox_facing_libraries_catalogued"],
        "direct_static_imports": reachability_counts.get("direct", 0),
        "transitive_static_dependencies": reachability_counts.get("transitive", 0),
        "not_statically_reachable": reachability_counts.get("not statically reachable", 0),
        "title_regular_import_dlls": inventory["title"]["regular_import_dlls"],
        "title_regular_import_symbols": inventory["title"]["regular_import_symbols"],
        "title_delay_import_dlls": inventory["title"]["delay_import_dlls"],
        "title_delay_import_symbols": inventory["title"]["delay_import_symbols"],
    }
    by_name = {row["name"].casefold(): row for row in data.get("libraries", [])}
    for row in inventory["libraries"]:
        name = row["name"]
        exports = row["exports"]
        relative = row["path"]
        reference = relative
        reachability = row["title_reachability"]
        map_info = {
            "source_path": relative,
            "source_sha256": row["sha256"],
            "file_description": row["metadata"].get("FileDescription", ""),
            "file_version": row["metadata"].get("FileVersion", row["metadata"].get("ProductVersion", "")),
            "title_reachability": reachability,
            "title_import_symbols": row.get("title_import_symbols", []),
            "static_import_chain": row.get("title_import_chain", []),
        }
        existing = by_name.get(name.casefold())
        if existing is not None:
            existing["inventory"] = map_info
            continue
        description = row["metadata"].get("FileDescription", "")
        group = GROUPS.get(name.casefold(), "Other Xbox support")
        relationship = {
            "direct": "directly imported by the title",
            "transitive": "reached through the title's static DLL dependency graph",
            "not statically reachable": (
                "not present in the title's static DLL dependency graph; dynamic loads are not measured here"
            ),
        }[reachability]
        role = f" {description}." if description else ""
        note = (f"On-disk Xbox support module.{role} {exports['total']} exports catalogued; "
                f"{relationship}. No TuxBox implementation has been verified.")
        new = {
            "name": name,
            "group": group,
            "status": "incomplete",
            "provider": "No verified TuxBox provider for this library.",
            "scope": note.strip(),
            "remaining": "Confirm the required title behavior, then implement and verify its used exports and interfaces.",
            "evidence": ["evidence/frosty-xbox-library-inventory.json"],
            "export_coverage": {
                "reference": reference,
                "binary_sha256": row["sha256"],
                "total": exports["total"],
                "data_exports": exports["data_exports"],
                "verified": [],
                "notes": "Exports are catalogued in the local image. Zero exports currently have verified TuxBox coverage.",
            },
            "inventory": map_info,
        }
        data.setdefault("libraries", []).append(new)
        by_name[name.casefold()] = new
    map_path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def scan(package):
    package = package.resolve()
    system = package / SYSTEM_RELATIVE
    mount = package / MOUNT_RELATIVE
    title_path = package / TITLE_RELATIVE
    for path in (system, mount, title_path):
        if not path.exists():
            raise ValueError(f"Expected Frosty package member is missing: {path.relative_to(package)}")
    reviewed = read_reviewed_map(ROOT / "evidence/library-status.json")
    reviewed_names = {row["name"].casefold() for row in reviewed.get("libraries", [])}
    all_dlls = [(path, "system image") for path in sorted(system.rglob("*.dll"), key=lambda p: p.as_posix().casefold())]
    all_dlls += [(path, "title mount") for path in sorted(mount.rglob("*.dll"), key=lambda p: p.as_posix().casefold())]
    all_modules = []
    errors = []
    for path, source in all_dlls:
        try:
            row = record_module(path, package, source)
            row["selection_reasons"] = candidate(path, row["metadata"], reviewed_names)
            all_modules.append(row)
        except (OSError, ValueError, pefile.PEFormatError, AttributeError, KeyError) as exc:
            errors.append({"path": path.relative_to(package).as_posix(), "error": str(exc)})
    candidates = [row for row in all_modules if row["selection_reasons"]]
    target = image(title_path)
    paths = title_reachability(candidates, target)
    edges, reverse = import_edges(all_modules, target)
    for row in candidates:
        chain = paths.get(row["name"].casefold(), [])
        row["title_reachability"] = ("direct" if len(chain) == 2 else
                                     "transitive" if chain else "not statically reachable")
        row["title_import_chain"] = chain
        row["title_import_symbols"] = []
        for table_name in ("imports", "delay_imports"):
            for item in target[table_name]:
                if item["module"].casefold() == row["name"].casefold():
                    row["title_import_symbols"].extend(item["symbols"])
        row["imported_by"] = reverse.get(row["name"].casefold(), [])
        row["catalog_dependencies"] = [
            edge for edge in edges if edge["from"].casefold() == row["name"].casefold()
        ]
    target_imports = [
        {"table": table_name, **item,
         "xbox_catalogued": item["module"].casefold() in {row["name"].casefold() for row in candidates}}
        for table_name in ("imports", "delay_imports") for item in target[table_name]
    ]
    return {
        "schema_version": 1,
        "purpose": "Static inventory of Xbox-facing binaries, their PE export/import surfaces, and Frosty's static DLL reachability. Presence and imports do not prove runtime use.",
        "source_layout": {
            "system_image": SYSTEM_RELATIVE.as_posix(),
            "title_mount": MOUNT_RELATIVE.as_posix(),
            "title_executable": TITLE_RELATIVE.as_posix(),
        },
        "scan_summary": {
            "system_image_dlls_found": sum(source == "system image" for _, source in all_dlls),
            "title_mount_dlls_found": sum(source == "title mount" for _, source in all_dlls),
            "successfully_parsed_dlls": len(all_modules),
            "xbox_facing_libraries_catalogued": len(candidates),
            "scan_errors": len(errors),
            "selection_rule": "Existing reviewed Xbox-facing map entries, named Xbox/Xtf/PIX families, or PE version metadata identifying Xbox, GameCore, Durango, Game OS, PIX, or console support.",
        },
        "title": {
            "path": TITLE_RELATIVE.as_posix(),
            "sha256": target["sha256"],
            "regular_import_dlls": len(target["imports"]),
            "regular_import_symbols": sum(len(row["symbols"]) for row in target["imports"]),
            "delay_import_dlls": len(target["delay_imports"]),
            "delay_import_symbols": sum(len(row["symbols"]) for row in target["delay_imports"]),
            "imports": target_imports,
        },
        "title_reachability": [
            {"library": row["name"], "relation": row["title_reachability"],
             "chain": row["title_import_chain"], "direct_symbols": row["title_import_symbols"]}
            for row in sorted(candidates, key=lambda row: (row["title_reachability"] != "direct",
                                                            row["title_reachability"] != "transitive",
                                                            row["name"].casefold()))
        ],
        "dependency_edges": edges,
        "libraries": candidates,
        "scan_errors": errors,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path,
                        help="root containing EmbeddedXvd and Mount for the local Frosty installation")
    parser.add_argument("--output", type=Path, default=ROOT / "evidence/frosty-xbox-library-inventory.json",
                        help="inventory JSON output path")
    parser.add_argument("--update-map", action="store_true",
                        help="add newly discovered libraries to evidence/library-status.json as incomplete")
    args = parser.parse_args()
    try:
        inventory = scan(args.package)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(inventory, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        if args.update_map:
            sync_library_map(inventory, ROOT / "evidence/library-status.json")
        counts = inventory["scan_summary"]
    except (OSError, ValueError, KeyError, pefile.PEFormatError) as exc:
        parser.exit(1, f"scan failed: {exc}\n")
    print(f"Scanned {counts['successfully_parsed_dlls']} DLLs; catalogued "
          f"{counts['xbox_facing_libraries_catalogued']} Xbox-facing libraries; "
          f"{counts['scan_errors']} scan errors.")
    print(f"Wrote {args.output}")
    if args.update_map:
        print("Updated evidence/library-status.json; new entries start at 0 verified exports.")


if __name__ == "__main__":
    main()
