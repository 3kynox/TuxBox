"""Render the public runtime and Xbox library compatibility baseline."""
from collections import Counter
import json
from pathlib import Path
from textwrap import wrap
from xml.etree import ElementTree as ET

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
    replay = read_json(evidence / report["chase_display_receipt"]) if report.get("chase_display_receipt") else {}
    extended = replay.get("desktop_extended_trial", {})
    if not report.get("sha256") or not extended or extended.get("title_sha256") != report["sha256"]:
        return "", notes, []
    rows = []
    if replay.get("city_vista_trial", {}).get("later_clip_match"):
        rows.append(("Chase cinematics", "Rendered", GREEN))
    mixed = read_json(evidence / "matrix-awakens/retail-mixed-input.json")
    if mixed.get("title_sha256") == report["sha256"] and mixed.get("actions"):
        rows.append(("Menu input", "Keyboard, mouse, XInput", GREEN))
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
        run = prep["complete_preparation"]
        rows.append(("First-boot prep", f"{run['cold_dxil_compiled']} shaders · {run['cold_gpu_pipelines_created']} pipelines", TEXT))
    gpu = replay.get("gpu", "").removeprefix("NVIDIA GeForce ")
    provider = "Proton Experimental" if "experimental" in replay.get("proton", "").lower() else replay.get("proton", "")
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
            text(group, 18, 96, "Retail launch with reference providers", 15)
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


def draw_libraries(svg, y, data):
    libraries = data.get("libraries", [])
    if not libraries:
        return y
    counts = Counter(row["status"] for row in libraries)
    by_group = {}
    for library in libraries:
        by_group.setdefault(library["group"], []).append(library)
    groups = list(by_group.items())
    # Reserve a monospace cell for the longest DLL before deciding how many families fit.
    min_width = max(37 + 9.1*len(row["name"]) for row in libraries)
    columns = min(4, max(1, int((INNER-44+GAP)//min_width)))
    bands = [groups[i:i+columns] for i in range(0, len(groups), columns)]
    height = 130 + sum(50 + max(len(entries) for _, entries in band)*30 for band in bands)
    group = section(svg, PAD, y, "Xbox library coverage")
    window(group, INNER, height, "~/evidence/library-status.json")
    text(group, 18, 70, "Xbox library coverage", 25, AMBER, bold=True)
    text(group, INNER-18, 68, f"{len(libraries)} tracked DLLs · TuxBox and reference providers",
         15, MUTED, anchor="end")
    # Counts refer to the libraries listed below; status definitions and evidence are in LIBRARIES.md.
    x = 18
    for name in COLORS:
        symbol(group, x+5, 99, name)
        label = f"{counts[name]} {name}"
        text(group, x+20, 104, label, 15, COLORS[name])
        x += 20 + 10*len(label) + 24
    y0 = 123
    for band in bands:
        width = (INNER-36+GAP)/len(band)
        for col, (family, entries) in enumerate(band):
            x = 18+col*width
            rect(group, x, y0, width-GAP, 1, LINE)
            text(group, x, y0+28, family, 16, AQUA, bold=True)
            for index, library in enumerate(entries):
                state, name = library["status"], library["name"]
                cell = element(group, "g")
                element(cell, "title").text = f"{name} · {state.capitalize()} · {library['provider']} {library['scope']}"
                top = y0+54+30*index
                symbol(cell, x+6, top-5, state)
                text(cell, x+21, top, name, 15, COLORS[state])
        y0 += 50 + max(len(entries) for _, entries in band)*30
    return y+height+GAP


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
        "Current runtime capabilities, retail output and Xbox DLL coverage. "
        "A checked circle marks complete coverage, a half-filled circle partial coverage "
        "and an open ring incomplete coverage.")
    bg = rect(svg, 0, 0, WIDTH, 1, BG)
    rect(svg, 0, 0, WIDTH, 48, BAR)
    rect(svg, 0, 47, WIDTH, 1, LINE)
    rect(svg, PAD, 10, 210, 27, AMBER)
    text(svg, PAD+105, 29, "Runtime compatibility", 15, BG, bold=True, anchor="middle")
    brand = text(svg, WIDTH//2, 32, "TUXBOX", 22, AMBER, bold=True, anchor="middle")
    brand.set("letter-spacing", "4")
    text(svg, WIDTH-PAD, 30, "Xbox Series X|S", 16, AQUA, anchor="end")
    y = draw_runtime(svg, 48+GAP, evidence)
    y = draw_libraries(svg, y, read_json(evidence / "library-status.json"))
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
