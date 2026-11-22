"""Render the public runtime and Xbox library compatibility baseline."""
from collections import Counter
import json
from pathlib import Path
from textwrap import wrap
from xml.etree import ElementTree as ET

from .d3d12_status import interface_inventory
from .verify import homebrew_matrix


WIDTH, PAD, GAP = 1000, 14, 12
INNER = WIDTH - 2 * PAD
BG, BAR, CHROME = "#0b0b0f", "#090910", "#0d0d15"
PANEL, LINE = "#14141d", "#272733"
TEXT, MUTED, AQUA = "#d7d7de", "#a0a0b1", "#6fc7d4"
GREEN, AMBER, RED = "#2ecc71", "#e2bc8c", "#e74c3c"
COLORS = {"complete": GREEN, "partial": AMBER, "incomplete": RED}
MONO = "'Terminess Nerd Font Mono', 'DejaVu Sans Mono', Consolas, monospace"


def element(parent, tag, **attrs):
    return ET.SubElement(parent, tag, {k.replace("_", "-"): str(v) for k, v in attrs.items()})


def text(parent, x, y, value, size=16, color=TEXT, *, bold=False, anchor="start"):
    node = element(parent, "text", x=x, y=y, fill=color, font_size=size, text_anchor=anchor,
                   font_weight="bold" if bold else "normal")
    node.text = str(value)
    return node


def rect(node, x, y, width, height, color, **attrs):
    return element(node, "rect", x=x, y=y, width=width, height=height, fill=color, **attrs)


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else {}


def section(parent, x, y, heading):
    group = element(parent, "g", transform=f"translate({x} {y})")
    element(group, "title").text = heading
    return group


def window(group, width, height, path, *, focused=False):
    rect(group, 0, 0, width, height, PANEL, stroke=AMBER if focused else LINE,
         stroke_width=2 if focused else 1)
    rect(group, 1, 1, width-2, 32, CHROME)
    rect(group, 1, 33, width-2, 1, LINE)
    for i, color in enumerate((RED, "#f1c40f", GREEN)):
        element(group, "circle", cx=16+16*i, cy=17, r=4.5, fill=color)
    text(group, 68, 22, path, 14, AQUA)


def symbol(parent, x, y, level):
    """A tick, half-disc and ring keep the library levels distinguishable without colour."""
    group = element(parent, "g", transform=f"translate({x} {y})")
    color = COLORS[level]
    element(group, "circle", cx=0, cy=0, r=5.5, fill="none", stroke=color, stroke_width=1.7)
    if level == "complete":
        element(group, "path", d="M-3,0 -1,2 3,-2", fill="none", stroke=color, stroke_width=1.5)
    elif level == "partial":
        element(group, "path", d="M0,-5.5 A5.5,5.5 0 0 0 0,5.5 Z", fill=color)


def milestones(path):
    rows = []
    if path.is_file():
        for line in path.read_text(encoding="utf-8").splitlines():
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            if len(cells) == 4:
                number, sep, name = cells[0].partition(". ")
                if number.isdigit() and sep:
                    summary = cells[2].partition(". ")[0].rstrip(".")
                    rows.append((name, cells[1], summary))
    return rows


def loader_notes(report, key):
    trial = report["trials"][key]
    missing = trial.get("missing_modules", [])
    known = {name.casefold(): symbols for name, symbols in report.get("imports_from_missing_modules", {}).items()}
    notes = [f"Trial {key}"]
    if trial.get("stage") == "title-code":
        notes.append("Title code reached")
    notes.append("Frame verified" if trial.get("frame_verified") else "Frame unverified")
    if missing:
        notes.append("Missing DLLs: " + ", ".join(missing))
        count = (sum(len(known[name.casefold()]) for name in missing)
                 if all(name.casefold() in known for name in missing) else "unknown")
        notes.append(f"Direct imports in missing DLLs: {count}")
    unresolved = trial.get("unresolved_direct_symbols", [])
    if unresolved:
        notes.append(f"Unresolved imports in loaded DLLs: {len(unresolved)}")
    failure = trial.get("observed_failure", {})
    if failure:
        if failure.get("title_diagnostic"):
            notes.append(failure["title_diagnostic"])
        details = {
            "unimplemented-import": ("Unimplemented import", "symbol"),
            "access-violation": ("Access violation", "exception_code"),
            "title-fatal-error": ("Title fatal error", "exception_code"),
            "graphics-device-lost": ("Graphics device lost", "hresult"),
        }
        label, field = details.get(failure["kind"], (failure["kind"], "exception_code"))
        notes.append(f"{label} {failure.get(field, '')}".strip())
        if "title_frame_rva" in failure:
            notes.append(f"Title stack RVA {failure['title_frame_rva']}")
    return notes


def retail_summary(evidence, report):
    trials = report.get("trials", {})
    key = report.get("current_trial") or next(reversed(trials), None)
    if key is None:
        return "", ["No retail launch recorded."], []
    if key not in trials:
        raise ValueError(f"Unknown current retail trial {key!r}")
    notes = loader_notes(report, key)
    independent = read_json(evidence / "matrix-awakens/independent-menu.json")
    if (independent.get("executable_sha256") == report.get("sha256") and report.get("sha256")
            and "independent-menu.webp" in independent.get("captures", {})):
        rows = [("Main menu", "Rendered", GREEN)]
        if "independent-settings.webp" in independent["captures"]:
            rows.append(("Settings", "Opened from keyboard", GREEN))
        menu = read_json(evidence / "matrix-awakens/menu-background.json")
        if menu.get("title_sha256") == report["sha256"] and "verified" in menu:
            if menu["verified"]:
                rows.append(("Green code background", "Animated", GREEN))
            elif menu.get("observation") == "Black":
                rows.append(("Green code background", "Missing", RED))
        if "independent-wakeup.webp" in independent["captures"]:
            rows.append(("Start Chase", "Opening cinematic", GREEN))
        chase = read_json(evidence / "matrix-awakens/chase-gameplay.json")
        if (chase.get("title_sha256") == report["sha256"]
                and chase.get("run", {}).get("first_gameplay_profile", {}).get("name") == "XSX_ChaseGameplay"):
            rows.append(("Chase tutorial", "Reached in game", GREEN))
            controls = chase.get("current_build", {}).get("controls") or chase["controls"]
            if controls.get("target_lock_action", {}).get("verified"):
                rows.append(("Controller target lock", "Registered in game", GREEN))
            fired = controls.get("manual_fire_verified")
            rows.append(("Manual shooting", "Verified" if fired else "Unverified", GREEN if fired else MUTED))
            runs = [chase.get(key, {}) for key in ("warm_replay", "sandbox_replay", "normal_final")]
            sandboxes = [run["sandbox"] for run in runs if run.get("title_sha256") == report["sha256"]
                         and run.get("sandbox", {}).get("state") == "EFrostyGameStateType::Sandbox"]
            if sandboxes:
                rows.append(("City sandbox", "Reached after the chase", GREEN))
            movement = any(sandbox.get("free_roam_movement_verified") for sandbox in sandboxes)
            rows.append(("Free roam", "Verified" if movement else "Unverified", GREEN if movement else MUTED))
        else:
            rows.append(("Driving and shooting", "Unverified", MUTED))
        host = independent.get("proton", "")
        if "experimental" in host.casefold():
            host = "Proton Experimental"
        return host, notes, rows
    replay = read_json(evidence / report["chase_display_receipt"]) if report.get("chase_display_receipt") else {}
    extended = replay.get("desktop_extended_trial", {})
    if not report.get("sha256"):
        return "", notes, []
    if extended.get("title_sha256") != report["sha256"]:
        replay, extended = {}, {}
    host = replay
    rows = []
    if replay.get("city_vista_trial", {}).get("later_clip_match"):
        rows.append(("Chase cinematics", "Rendered", GREEN))
    mixed = read_json(evidence / "matrix-awakens/retail-mixed-input.json")
    if mixed.get("title_sha256") == report["sha256"] and mixed.get("actions"):
        rows.append(("Menu input", "Keyboard, mouse, XInput", GREEN))
        host = host or mixed
    menu = read_json(evidence / "matrix-awakens/menu-background.json")
    if (menu.get("title_sha256") == report["sha256"] and menu.get("verified") is False
            and menu.get("observation") == "Black"):
        rows.append(("Menu code background", "Missing", RED))
        host = host or menu
    if extended.get("fraction_pixels_with_all_rgb_channels_at_most_12", 0) > 0.99:
        rows.append(("Real-time dialogue", "Near-black", RED))
    if "driving_and_shooting_verified" in extended:
        verified = extended["driving_and_shooting_verified"]
        rows.append(("Driving and shooting", "Verified" if verified else "Unverified", GREEN if verified else MUTED))
    seconds = extended.get("seconds_from_chase_global_to_last_render_warning")
    if seconds and "Cine_10_ChaseIntro_1_Dialogue" in extended.get("activated_layers_utc", {}):
        minutes, seconds = divmod(int(seconds), 60)
        rows.append(("Recorded chase run", f"{minutes}m {seconds:02d}s", TEXT))
    prep = read_json(evidence / "matrix-awakens/first-boot-pipelines.json")
    if prep.get("title_sha256") == report["sha256"] and "complete_preparation" in prep:
        host = host or prep
        run = prep["complete_preparation"]
        rows.append(("First-boot prep", f"{run['cold_dxil_compiled']} shaders · {run['cold_gpu_pipelines_created']} pipelines", TEXT))
    gpu = host.get("gpu", "").removeprefix("NVIDIA GeForce ")
    provider = host.get("proton") or host.get("provider", "")
    if "experimental" in provider.lower():
        provider = "Proton Experimental"
    return " · ".join(filter(None, (provider, gpu))), notes, rows


def draw_runtime(svg, y, evidence):
    rows = milestones(evidence.parent / "docs/PROGRESS.md")
    report = read_json(evidence / "matrix-awakens-launch.json")
    if not rows and not report:
        return y
    host, notes, retail = retail_summary(evidence, report) if report else ("", [], [])
    if retail:
        # The retail title has its own panel with the measured result.
        rows = [row for row in rows if row[0] != "First retail title"]
    left = 509 if rows and report else INNER
    right = INNER - left - GAP if rows and report else INNER
    runtime_rows = []
    for name, status, summary in rows:
        # Leave room for the right-aligned status when a fallback monospace font is used.
        names = wrap(name, max(16, int((left-48-9*len(status))/9.7)))
        detail = wrap(summary, (left-36)//9) or [""]
        runtime_rows.append((names, status, detail))
    runtime_height = 91 + sum(15 + 20*len(names) + 18*len(detail) for names, _, detail in runtime_rows)

    fallback = [part for note in notes for part in wrap(note, (right-36)//9)]
    retail_rows = []
    for label, value, color in retail:
        labels = wrap(label, max(10, int((right-48)/9.2) - len(value)))
        retail_rows.append((labels, value, color))
    retail_height = (149 + sum(21 + 18*len(labels) for labels, _, _ in retail_rows)
                     if retail else 99 + 23*len(fallback))
    height = max(runtime_height if rows else 0, retail_height if report else 0)
    if rows:
        group = section(svg, PAD, y, "Runtime capabilities")
        window(group, left, height, "~/docs/PROGRESS.md", focused=True)
        text(group, 18, 70, "Runtime capabilities", 25, AMBER, bold=True)
        top = 83
        for names, status, detail in runtime_rows:
            rect(group, 18, top, left-36, 1, LINE)
            for i, line in enumerate(names):
                text(group, 18, top+23+i*20, line, 16, bold=True)
            color = {"Verified": GREEN, "Partial": AMBER, "Pending": MUTED}.get(status, AQUA)
            text(group, left-18, top+23, status, 14, color, anchor="end")
            top += 20*len(names)
            for i, line in enumerate(detail):
                text(group, 18, top+22+i*18, line, 15, MUTED)
            top += 15 + 18*len(detail)
    if report:
        group = section(svg, WIDTH-PAD-right, y, "The Matrix Awakens")
        window(group, right, height, "~/evidence/matrix-awakens-launch.json", focused=not rows)
        text(group, 18, 70, "The Matrix Awakens", 25, AMBER, bold=True)
        if retail:
            element(group, "desc").text = ". ".join(notes)
            label = "Retail launch with TuxBox providers" if any(row[0] == "Main menu" for row in retail) else "Retail comparison run"
            text(group, 18, 96, label, 15)
            text(group, 18, 118, host, 15, MUTED)
            top = 136
            for labels, value, color in retail_rows:
                rect(group, 18, top, right-36, 1, LINE)
                for i, line in enumerate(labels):
                    text(group, 18, top+25+18*i, line, 15)
                text(group, right-18, top+25, value, 15, color, anchor="end")
                top += 21 + 18*len(labels)
        else:
            for i, line in enumerate(fallback):
                text(group, 18, 96+i*23, line, 15, MUTED)
    return y+height+GAP


def export_progress(library):
    record = library.get("export_coverage")
    if record is None:
        return "—", "Unmeasured", None
    verified, total = record["verified"], record["total"]
    if (not isinstance(verified, list) or not all(isinstance(name, str) and name for name in verified)
            or len(verified) != len(set(verified))):
        raise ValueError(f"Invalid verified exports for {library['name']}")
    count = len(verified)
    if total is None:
        return f"{count}/?", "Unmeasured", None
    if type(total) is not int or total < count:
        raise ValueError(f"Invalid export total for {library['name']}")
    if total == 0:
        return "0/0", "No exports", None
    fraction = count / total
    number = f"{fraction * 100:.1f}".removesuffix(".0")
    if number == "0" and count:
        number = "<0.1"
    elif number == "100" and count < total:
        number = ">99.9"
    return f"{count}/{total}", number + "%", fraction


def export_breakdown(library):
    coverage = library.get("export_coverage") or {}
    data = coverage.get("data_exports")
    if not data or coverage.get("total") is None:
        return ""
    verified = set(coverage["verified"])
    tested_data = len(verified.intersection(data))
    functions = coverage["total"] - len(data)
    return f"{len(verified)-tested_data}/{functions} functions, {tested_data}/{len(data)} data"


def frosty_import_coverage(evidence, libraries):
    inventory = read_json(evidence / "frosty-xbox-library-inventory.json")
    if not inventory:
        return None
    direct = {}
    for entry in inventory["title"]["imports"]:
        direct.setdefault(entry["module"].casefold(), []).extend(entry["symbols"])
    binaries = {(pe["name"].casefold(), pe["sha256"]): pe for pe in inventory["libraries"]}
    results = {}
    for library in libraries:
        name = library["name"]
        if name.casefold() not in direct:
            continue
        coverage = library.get("export_coverage") or {}
        pe = binaries.get((name.casefold(), coverage.get("binary_sha256")))
        if pe is None:
            results[name] = None
            continue
        slots = {}
        for entry in pe["exports"]["entries"]:
            ordinal = entry["ordinal"]
            slots[f"#{ordinal}"] = ordinal
            slots.update(dict.fromkeys(entry["names"], ordinal))
        verified = {slots[symbol] for symbol in coverage["verified"] if symbol in slots}
        # Multiple import spellings can resolve to one export slot. Preserve the first
        # title spelling for display and count unresolved imports as outstanding.
        imported = {}
        for symbol in direct[name.casefold()]:
            imported.setdefault(slots.get(symbol, symbol), symbol)
        results[name] = {symbol: slot in verified for slot, symbol in imported.items()}
    return results


def import_tally(symbols):
    return f"{sum(symbols.values())}/{len(symbols)}" if symbols is not None else "Unmeasured"


def library_table(evidence):
    libraries = read_json(evidence / "library-status.json").get("libraries", [])
    direct = frosty_import_coverage(evidence, libraries)
    name_width = max([len("Library"), *(len(lib["name"]) for lib in libraries)])
    heading = f"{'Library':<{name_width}}  {'Verified TuxBox exports':<26} {'Count':>7}  {'Share':>10}  Status"
    if direct is not None:
        heading += "      Static PE path  Frosty imports verified"
    lines = [heading]
    for library in libraries:
        tally, percent, fraction = export_progress(library)
        if fraction is None:
            bar = "[      unmeasured      ]"
        else:
            # '+' marks a partly filled cell so small positive values remain visible.
            cells = fraction * 22
            filled = "#" * int(cells) + ("+" if cells % 1 else "")
            bar = f"[{filled:.<22}]"
        name, state = library["name"], library["status"].capitalize()
        row = f"{name:<{name_width}}  {bar:<26} {tally:>7}  {percent:>10}  "
        if direct is None:
            lines.append(row + state)
            continue
        relation = (library.get("inventory") or {}).get("title_reachability")
        path = {"transitive": "transitive", "not statically reachable": "no static path"}.get(relation, "unscanned")
        path = "direct" if name in direct else path
        count = import_tally(direct[name]) if name in direct else "—"
        lines.append(f"{row}{state:<10} {path:<14}  {count}")
    if direct:
        measured = [symbols for symbols in direct.values() if symbols is not None]
        if measured:
            checked = sum(sum(symbols.values()) for symbols in measured)
            total = sum(len(symbols) for symbols in measured)
            lines.extend(["", f"TuxBox has verified {checked}/{total} distinct Frosty imports from the measured DLLs. "
                          f"Matching build evidence covers {len(measured)} of {len(direct)} directly imported tracked DLLs."])
        else:
            lines.extend(["", "Frosty direct import coverage is unmeasured for this build."])
    details = [f"{library['name']} covers {parts}." for library in libraries if (parts := export_breakdown(library))]
    return "\n".join(lines + (["", *details] if details else []))


def draw_libraries(svg, y, data, direct):
    libraries = data.get("libraries", [])
    if not libraries:
        return y
    counts = Counter(row["status"] for row in libraries)
    by_group = {}
    for library in libraries:
        by_group.setdefault(library["group"], []).append(library)
    groups = list(by_group.items())
    # Allow room for the widest filename, the bar and its two right-aligned labels.
    name_width = 32 + max(len(row["name"]) for row in libraries) * 9.2
    columns = 2 if len(groups) > 1 and (INNER-36+GAP)/2 >= name_width+220 else 1
    width = (INNER-36+GAP)/columns
    bands = [groups[i:i+columns] for i in range(0, len(groups), columns)]
    row_height = 29
    # Mixed function/data tables get a second line so large GUID tables remain legible.
    band_heights = [49 + max(sum(row_height + (16 if export_breakdown(lib) else 0) for lib in entries)
                             for _, entries in band) for band in bands]
    height = 151 + sum(band_heights)
    group = section(svg, PAD, y, "Xbox library coverage")
    window(group, INNER, height, "~/evidence/library-status.json")
    text(group, 18, 70, "Xbox library coverage", 25, AMBER, bold=True)
    summary = data.get("inventory_summary", {})
    if summary:
        heading = (f"{len(libraries)} tracked · {summary['system_image_dlls_scanned']} system DLLs scanned · "
                   f"{summary['direct_static_imports']} imports · {summary['transitive_static_dependencies']} transitive")
    else:
        heading = f"{len(libraries)} tracked DLLs · TuxBox and reference providers"
    text(group, INNER-18, 68, heading, 14, MUTED, anchor="end")
    x = 18
    for name in COLORS:
        symbol(group, x+5, 99, name)
        label = f"{counts[name]} {name}"
        text(group, x+20, 104, label, 15, COLORS[name])
        x += 20 + 10*len(label) + 24
    text(group, 18, 130,
         "Bars show verified TuxBox exports. * title import · ~ static dependency · blank = no static path; dynamic loads are unmeasured.",
         13, MUTED)
    y0 = 148
    for band, band_height in zip(bands, band_heights):
        for col, (family, entries) in enumerate(band):
            x = 18+col*width
            right = x+width-GAP
            bar_x = x+name_width
            bar_width = right-bar_x-143
            rect(group, x, y0, width-GAP, 1, LINE)
            text(group, x, y0+25, family, 16, AQUA, bold=True)
            top = y0+52
            for library in entries:
                state, name = library["status"], library["name"]
                tally, percent, fraction = export_progress(library)
                cell = element(group, "g")
                parts = export_breakdown(library)
                inventory = library.get("inventory") or {}
                relation = inventory.get("title_reachability")
                marker = {"direct": " *", "transitive": " ~"}.get(relation, "")
                title_symbols = inventory.get("title_import_symbols", [])
                if direct is not None and name in direct:
                    symbols = direct[name]
                    reach_note = f"Frosty direct imports verified {import_tally(symbols)}."
                    if symbols is None:
                        reach_note += " The reviewed DLL hash does not match the PE inventory."
                    else:
                        for checked, label in [(True, "Verified"), (False, "Unverified")]:
                            names = [symbol for symbol, covered in symbols.items() if covered == checked]
                            if names:
                                reach_note += f" {label} ({', '.join(names)})."
                elif title_symbols:
                    reach_note = "Title PE imports: " + ", ".join(title_symbols)
                elif inventory.get("static_import_chain"):
                    reach_note = "Static title path: " + " -> ".join(inventory["static_import_chain"])
                else:
                    reach_note = "No static PE import path found; dynamic loads are not measured."
                element(cell, "title").text = (f"{name} · {state.capitalize()} · Verified TuxBox exports {tally} "
                                               f"({percent}). {library['provider']} {library['scope']} "
                                               f"{reach_note}")
                symbol(cell, x+6, top-5, state)
                text(cell, x+20, top, name + marker, 15)
                if fraction is None:
                    element(cell, "line", x1=bar_x, x2=bar_x+bar_width, y1=top-5, y2=top-5,
                            stroke=MUTED, stroke_dasharray="2 4")
                else:
                    rect(cell, bar_x, top-9, bar_width, 7, LINE)
                    if fraction:
                        rect(cell, bar_x, top-9, round(bar_width*fraction, 4), 7, AQUA)
                text(cell, right-83, top, tally, 13, MUTED, anchor="end")
                text(cell, right, top, percent, 13, TEXT, bold=True, anchor="end")
                if parts:
                    text(cell, x+20, top+16, parts, 13, MUTED)
                    top += 16
                top += row_height
        y0 += band_height
    return y+height+GAP


def shader_validation(data):
    checks = [row.get("standalone_dxil_validated") for row in data["requested"]]
    measured = sum(check is not None for check in checks)
    if not measured:
        return ""
    return (f"Standalone DXIL passed for {checks.count(True)}/{measured} tested programs "
            f"among {len(checks)} distinct title requests.")


def culling_readbacks(evidence, archive, compiler, name):
    census, probe, fields = {
        "InstanceCull": ("instance-cull-retail", "instance-cull", ("checked_words", "checked_texels")),
        "PersistentClusterCull": ("persistent-cluster-retail", "persistent-cull", ("verified_uav_words",)),
    }[name]
    retail = read_json(evidence / f"matrix-awakens/{census}.json")
    if not retail or not archive or not compiler or retail["global_archive_sha256"] != archive:
        return ""
    family = set(retail["family"]["archive_entries"])
    paired = []
    provider = None
    for suffix, adapter in (("amd", "1002:13c0"), ("nvidia", "10de:2705")):
        run = read_json(evidence / f"matrix-awakens/{probe}-{suffix}.json")
        if (run.get("adapter") != adapter or run.get("name") != name
                or run.get("compiler_sha256") != compiler or run.get("global_archive_sha256") != archive
                or not run.get("provider_sha256") or (provider and provider != run["provider_sha256"])):
            return ""
        provider = run["provider_sha256"]
        paired.append({(p["entry"], p["source_sha256"], p["dxil_sha256"]) for p in run["programs"]
                       if p["entry"] in family and p["cases"]
                       and all(c[field] > 0 for c in p["cases"].values() for field in fields)})
    verified = paired[0] & paired[1]
    # The trace identifies the original Xbox program. Both current GPUs must
    # agree on the tested DXIL, which can change after a compiler correction.
    if not any(entry == retail["entry"] and source == retail["source_sha256"]
               and (compiler != retail["compiler_sha256"] or dxil == retail["dxil_sha256"])
               for entry, source, dxil in verified):
        return ""
    count = len({entry for entry, _, _ in verified})
    return f"Nanite {name} · {count}/{len(family)} packaged variants passed GPU readback on both adapters"


def shader_checks(evidence, data):
    notes = [validation] if (validation := shader_validation(data)) else []
    archive = data.get("archives", {}).get("global", {}).get("member_sha256")
    warmup = read_json(evidence / "matrix-awakens/chase-warmup.json")
    if (warmup and warmup["title_sha256"] == data.get("executable_sha256")
            and warmup["global_archive_sha256"] == archive
            and warmup["compiler_sha256"] == data.get("compiler_sha256")
            and warmup["warm"]["total_compiled"] == 0):
        chase = warmup["warm"]["chase"]
        notes.append(f"Optional chase warmup · {chase['programs']['checked']:,} programs and "
                     f"{chase['links']['checked']} linked pairs ready before playing")
    total = next((row["global"] for row in data["stages"] if row["stage"] == "Mesh"), 0)
    results = []
    for suffix, gpu in (("nvidia", "NVIDIA"), ("amd", "Radeon")):
        run = read_json(evidence / f"matrix-awakens/nanite-mesh-{suffix}.json")
        pipelines = run.get("mesh_pipelines", {})
        if (total and pipelines and run.get("global_archive_sha256") == archive
                and pipelines["compiler_sha256"] == data["compiler_sha256"]):
            results.append(f"{gpu} {len(set(pipelines['archive_entries']))}/{total}")
    if results:
        notes.append("Packaged mesh shaders with a GPU pipeline · " + " · ".join(results))
    for name in ("InstanceCull", "PersistentClusterCull"):
        if cull := culling_readbacks(evidence, archive, data.get("compiler_sha256"), name):
            notes.append(cull)
    micro = read_json(evidence / "matrix-awakens/micropoly-retail.json")
    if micro and micro["global_archive_sha256"] == archive:
        pipelines = []
        for suffix, adapter in (("amd", "1002:13c0"), ("nvidia", "10de:2705")):
            run = read_json(evidence / f"matrix-awakens/micropoly-{suffix}.json")
            suite = run.get("pipelines", {})
            if (run.get("adapter") == adapter and run.get("global_archive_sha256") == archive
                    and run.get("source_sha256") == micro["source_sha256"]
                    and run.get("compiler_sha256") == suite.get("compiler_sha256") == data["compiler_sha256"]
                    and run.get("cases") and all(c["checked_texels"] > 0 for c in run["cases"].values())):
                pipelines.append((suite["program_manifest_sha256"], run["provider_sha256"], run["dxil_sha256"],
                                  frozenset(suite["archive_entries"])))
        if len(pipelines) == 2 and pipelines[0] == pipelines[1]:
            family = set(micro["family"]["archive_entries"])
            count = len(family & pipelines[0][3])
            notes.append(f"Nanite MicropolyRasterize · {count}/{len(family)} pipelines on both adapters "
                         f"· retail shader {micro['entry']} passed pixel readback")
    for suite in ("classify-materials", "stencil-export", "tsr-filter", "tsr-history"):
        amd, nvidia = (read_json(evidence / f"matrix-awakens/{suite}-{vendor}.json")
                       for vendor in ("amd", "nvidia"))
        if not (amd.get("adapter") == "1002:13c0" and nvidia.get("adapter") == "10de:2705"
                and amd.get("provider_sha256") == nvidia.get("provider_sha256")
                and amd.get("compiler_sha256") == nvidia.get("compiler_sha256") == data["compiler_sha256"]
                and amd.get("global_archive_sha256") == nvidia.get("global_archive_sha256") == archive):
            continue
        if suite == "classify-materials":
            if (amd["source_sha256"] == nvidia["source_sha256"] and amd["dxil_sha256"] == nvidia["dxil_sha256"]
                    and amd["cases"] == nvidia["cases"] and amd["cases"]):
                notes.append(f"Nanite ClassifyMaterials · {len(amd['cases'])} material tile tests passed on both adapters")
        elif suite in ("tsr-filter", "tsr-history"):
            if (amd["source_sha256"] == nvidia["source_sha256"] and amd["dxil_sha256"] == nvidia["dxil_sha256"]
                    and amd["cases"] == nvidia["cases"] and amd["cases"]):
                texels = sum(output["verified_texels"] for case in amd["cases"].values() for output in case["outputs"])
                label = "temporal filter" if suite == "tsr-filter" else "temporal history"
                notes.append(f"City flyover {label} · {texels:,} output texels matched on both adapters")
        else:
            readbacks = [{(s["source_sha256"], s["readback_sha256"], s["matching_texels"])
                          for s in run["shaders"] if s["matching_texels"] > 0} for run in (amd, nvidia)]
            count = len(readbacks[0] & readbacks[1])
            if count:
                notes.append(f"Pixel shader stencil export · {count} retail programs matched depth and stencil on both adapters")
    return notes


def independent_counts(data):
    measurement = data.get("independent_emitter")
    if measurement is None:
        return {}
    counts = Counter(p["stage"] for p in measurement["programs"])
    return {row["stage"]: counts[row["stage"]] for row in data["stages"] if row["unique_dxbc"]}


def shader_table(evidence):
    data = read_json(evidence / "matrix-awakens/shader-inventory.json")
    if not data:
        return "No retail shader inventory recorded."
    independent = independent_counts(data)
    lines = ["Frosty shader coverage",
             f"{'Shader stage':<16} {'Packaged':>9} {'HLSL / unique DXBC':>21} {'TuxBox only':>12} "
             f"{'In profile':>11} {'Profiled / requested':>23}"]
    for row in data["stages"]:
        emitted = row["hlsl_emitted"]
        hlsl = f"{emitted:,}/{row['unique_dxbc']:,}" if emitted is not None else "—"
        observed = f"{row['requested_in_profile']}/{row['requested_dxbc']}" if row["requested_dxbc"] else "—"
        lines.append(f"{row['stage']:<16} {row['global'] + row['material']:>9,} {hlsl:>21} "
                     f"{independent.get(row['stage'], '—'):>12} {row['profiled_dxbc']:>11} {observed:>23}")
    lines.append(f"The preparation profile includes {data['linked_variants']} additional linked variants.")
    lines.extend(shader_checks(evidence, data))
    for material in data.get("menu_materials", ()):
        requested = [f"{row['stage']} #{row['entry']}" for row in material["members"] if row["observed_in"]]
        matches = ", ".join(requested) or "none in these traces"
        lines.append(f"{Path(material['asset']).stem} (map {material['map']}, {len(material['members'])} entries) "
                     f"· request trace matches {matches}.")
    for shader in data["requested"]:
        if not shader["in_preparation_profile"]:
            where = ", ".join(shader["archive_entries"]) or "outside the indexed archives"
            frontend = {True: "HLSL emitted", False: "frontend failed", None: "frontend unmeasured"}[shader.get("hlsl_emitted")]
            if (validated := shader.get("standalone_dxil_validated")) is not None:
                frontend = "standalone DXIL passed" if validated else "standalone DXIL failed"
            lines.append(f"Unprofiled {shader['stage'].lower()} {shader['source_sha256']} ({where}, {frontend})")
    return "\n".join(lines)


def draw_shaders(svg, y, evidence):
    data = read_json(evidence / "matrix-awakens/shader-inventory.json")
    if not data:
        return y
    rows = data["stages"]
    independent = independent_counts(data)
    checks = shader_checks(evidence, data)
    height = 195 + 27 * len(rows) + 21 * len(checks)
    group = section(svg, PAD, y, "Frosty shader coverage")
    window(group, INNER, height, "~/evidence/matrix-awakens/shader-inventory.json")
    text(group, 18, 70, "Frosty shader coverage", 25, AMBER, bold=True)
    packages = sum(row["global"] + row["material"] for row in rows)
    sources = sum(row["profiled_dxbc"] for row in rows)
    text(group, INNER-18, 68, f"{packages:,} shipped entries · {sources} in profile · "
         f"{data['linked_variants']} linked variants", 14, MUTED, anchor="end")
    text(group, 18, 97, "HLSL counts all translated programs. TuxBox only counts those translated with the legacy emitter disabled.", 14, MUTED)
    text(group, 18, 118, "Requests are distinct programs seen in captured title runs. A pipeline can request a shader before drawing.", 14, MUTED)
    columns = [(18, "Stage"), (211, "Packaged"), (463, "HLSL / unique DXBC"), (584, "TuxBox only"),
               (694, "In profile"), (INNER-18, "In profile / requested")]
    for x, label in columns:
        text(group, x, 151, label, 14, AQUA, anchor="start" if x == 18 else "end")
    for index, row in enumerate(rows):
        top = 164 + 27*index
        rect(group, 18, top-3, INNER-36, 1, LINE)
        text(group, 18, top+17, row["stage"], 15)
        text(group, 211, top+17, f"{row['global']+row['material']:,}", 15, TEXT, anchor="end")
        text(group, 584, top+17, independent.get(row["stage"], "—"), 15, TEXT, anchor="end")
        for x, width, right, value, total in (
                (229, 84, 463, row["hlsl_emitted"], row["unique_dxbc"]),
                (722, 94, INNER-18, row["requested_in_profile"], row["requested_dxbc"])):
            if value is None or not total:
                text(group, right, top+17, "—", 15, MUTED, anchor="end")
                continue
            rect(group, x, top+8, width, 7, LINE)
            if value:
                rect(group, x, top+8, width*value/total, 7, GREEN if value == total else AMBER)
            text(group, right, top+17, f"{value:,} / {total:,}", 14, TEXT if value else RED, anchor="end")
        text(group, 694, top+17, f"{row['profiled_dxbc']:,}", 15, TEXT, anchor="end")
    for line, check in enumerate(checks):
        text(group, 18, height-14-21*(len(checks)-line), check, 14, TEXT)
    text(group, 18, height-14, "Ray-tracing entries contain serialized state objects. Their translation has a separate path.", 14, MUTED)
    return y+height+GAP


def draw_d3d12(svg, y, data):
    if not data:
        return y
    rows = data["interfaces"]
    height = 138 + 23 * ((len(rows) + 2) // 3)
    group = section(svg, PAD, y, "d3d12_xs Xbox interface methods")
    window(group, INNER, height, "d3d12_xs.dll / interface methods")
    text(group, 18, 63, "Checked slots in Frosty’s catalogued interfaces", 18, TEXT, bold=True)
    text(group, 18, 86, f"Build {data['package_version']} · Counts show methods with TuxBox checks / identified methods in the sample.", 13, MUTED)
    for i, entry in enumerate(rows):
        col, row = i % 3, i // 3
        x, baseline = 18 + col * 318, 113 + 23 * row
        slots = entry["slots"]
        checked = sum(bool(slot.get("checks")) for slot in slots)
        text(group, x, baseline, entry["name"], 14, TEXT)
        text(group, x + 279, baseline, f"{checked}/{len(slots)}", 14, AQUA if checked else MUTED, anchor="end")
    bindings = data["registered_slots"]
    text(group, 18, height - 15, f"Compiled TuxBox handlers: {len(bindings['Device'])} device · "
         f"{len(bindings['Graphics command list'])} graphics list · {len(bindings['DMA command list'])} DMA list · "
         f"{len(bindings['Command queue'])} queue", 13, MUTED)
    return y + height + GAP


def draw_verification(svg, y, evidence):
    rows, _ = homebrew_matrix(evidence)
    if not rows:
        return y
    checks = [check for _, data in rows for check in data["checks"]]
    passed = sum(bool(check["passed"]) for check in checks)
    group = section(svg, PAD, y, "Guest regression checks")
    text(group, 0, 21, "Guest regression checks", 15, MUTED)
    text(group, INNER, 21, f"{passed} of {len(checks)} passed across {len(rows)} host configurations",
         15, MUTED, anchor="end")
    return y+28


def render_map(evidence):
    svg = ET.Element("svg", {"xmlns": "http://www.w3.org/2000/svg", "role": "img", "font-family": MONO,
                             "aria-labelledby": "map-title map-description"})
    element(svg, "title", id="map-title").text = "TuxBox runtime compatibility"
    element(svg, "desc", id="map-description").text = (
        "Current runtime capabilities, retail output, Xbox DLL coverage and shader preparation. "
        "A checked circle marks complete coverage, a half-filled circle partial coverage "
        "and an open ring incomplete coverage. Shader bars show frontend and preparation coverage. DLL bars show verified exports.")
    bg = rect(svg, 0, 0, WIDTH, 1, BG)
    rect(svg, 0, 0, WIDTH, 48, BAR)
    rect(svg, 0, 47, WIDTH, 1, LINE)
    rect(svg, PAD, 10, 210, 27, AMBER)
    text(svg, PAD+105, 29, "Runtime compatibility", 15, BG, bold=True, anchor="middle")
    brand = text(svg, WIDTH//2, 32, "TUXBOX", 22, AMBER, bold=True, anchor="middle")
    brand.set("letter-spacing", "4")
    text(svg, WIDTH-PAD, 30, "Xbox Series X|S", 16, AQUA, anchor="end")
    y = draw_runtime(svg, 48+GAP, evidence)
    y = draw_d3d12(svg, y, interface_inventory(evidence))
    y = draw_shaders(svg, y, evidence)
    data = read_json(evidence / "library-status.json")
    y = draw_libraries(svg, y, data, frosty_import_coverage(evidence, data.get("libraries", [])))
    height = draw_verification(svg, y, evidence) + 6
    bg.set("height", str(height))
    svg.set("viewBox", f"0 0 {WIDTH} {height}")
    svg.set("width", str(WIDTH))
    svg.set("height", str(height))
    ET.indent(svg)
    return ET.tostring(svg, encoding="unicode") + "\n"


def write_map(evidence, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_map(evidence), encoding="utf-8")
