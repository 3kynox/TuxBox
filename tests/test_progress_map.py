import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from xml.etree import ElementTree as ET

from project_tuxbox.progress_map import export_progress, culling_readbacks, library_table, render_map, write_map


class ProgressMapTests(unittest.TestCase):
    def test_culling_readbacks_require_matching_source_and_current_gpu_bytecode(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            dest = root / "matrix-awakens"
            dest.mkdir()
            retail = dict(global_archive_sha256="archive", compiler_sha256="old", d3d12_xs_sha256="provider",
                          entry=5, source_sha256="source", dxil_sha256="bytecode", family={"archive_entries": [5, 6]})
            (dest / "instance-cull-retail.json").write_text(json.dumps(retail))
            program = dict(entry=5, source_sha256="source", dxil_sha256="bytecode",
                           cases={"sparse": {"checked_words": 48, "checked_texels": 16}})
            run = dict(name="InstanceCull", compiler_sha256="current", global_archive_sha256="archive",
                       provider_sha256="provider", programs=[program])
            for suffix, adapter in (("amd", "1002:13c0"), ("nvidia", "10de:2705")):
                (dest / f"instance-cull-{suffix}.json").write_text(json.dumps(dict(run, adapter=adapter)))
            self.assertIn("1/2", culling_readbacks(root, "archive", "current", "InstanceCull"))
            self.assertFalse(culling_readbacks(root, "archive", "old", "InstanceCull"))
            self.assertFalse(culling_readbacks(root, "other", "current", "InstanceCull"))
            for changed in (dict(dxil_sha256="different"), dict(cases={})):
                (dest / "instance-cull-amd.json").write_text(json.dumps(
                    dict(run, adapter="1002:13c0", programs=[dict(program, **changed)])))
                self.assertFalse(culling_readbacks(root, "archive", "current", "InstanceCull"))
            # A corrected build may emit new DXIL for the same Xbox program.
            updated = dict(program, dxil_sha256="corrected")
            for suffix, adapter in (("amd", "1002:13c0"), ("nvidia", "10de:2705")):
                (dest / f"instance-cull-{suffix}.json").write_text(json.dumps(
                    dict(run, adapter=adapter, provider_sha256="new provider", programs=[updated])))
            self.assertIn("1/2", culling_readbacks(root, "archive", "current", "InstanceCull"))
            (dest / "instance-cull-nvidia.json").write_text(json.dumps(
                dict(run, adapter="10de:2705", programs=[updated])))
            self.assertFalse(culling_readbacks(root, "archive", "current", "InstanceCull"))

    def test_workspace_map_matches_saved_receipts_and_milestones(self):
        root = Path(__file__).resolve().parents[1]
        self.assertEqual((root / "docs/progress.svg").read_text(encoding="utf-8"),
                         render_map(root / "evidence"),
                         "Regenerate the map with python tuxbox.py status --svg docs/progress.svg")

    def test_library_coverage_precedes_regression_summary(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            rows = [{"name": "example<&>.dll", "status": "complete", "group": "Input", "provider": "Host",
                     "scope": "Pad input"}, {"name": "graphics.dll", "status": "partial", "group": "Graphics",
                                           "provider": "Rust factory", "scope": "Device creation"}]
            (root / "library-status.json").write_text(json.dumps({"libraries": rows}))
            for name, passed in [("provider-a", True), ("provider-b", False)]:
                (root / (name + ".json")).write_text(json.dumps({"type": "homebrew", "host": "Linux",
                    "provider": name, "checks": [{"gate": "memory", "passed": passed}]}))
            output = root / "output/map.svg"
            write_map(root, output)
            self.assertEqual(render_map(root), output.read_text())
            svg = ET.parse(output)
            labels = [e.text for e in svg.findall(".//{*}text")]
            self.assertLess(labels.index("Xbox library coverage"), labels.index("Guest regression checks"))
            for label in ["example<&>.dll", "1 complete", "1 partial", "0 incomplete",
                          "1 of 2 passed across 2 host configurations"]:
                self.assertIn(label, labels)
            descriptions = [e.text for e in svg.findall(".//{*}title")]
            self.assertTrue(any("example<&>.dll · Complete" in d and "Host Pad input" in d for d in descriptions))

    def test_export_bars_use_reviewed_totals_and_verified_exports(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            rows = []
            for name, total, verified in [("factory.dll", 1, ["Create"]), ("memory.dll", 15, ["#10", "Free"]),
                                          ("external.dll", 12, []), ("pending.dll", None, ["Initialize"])]:
                rows.append({"name": name, "status": "partial", "group": "Test", "provider": "Test provider",
                             "scope": "One tested operation", "export_coverage": {"total": total, "verified": verified}})
            rows[1]["export_coverage"]["data_exports"] = ["#10", "IID_Device", "IID_Queue"]
            (root / "library-status.json").write_text(json.dumps({"libraries": rows}))
            svg = ET.fromstring(render_map(root))
            table = library_table(root)
            for name, tally, pct in [("factory.dll", "1/1", "100%"), ("memory.dll", "2/15", "13.3%"),
                                      ("external.dll", "0/12", "0%"), ("pending.dll", "1/?", "Unmeasured")]:
                cell = next(g for g in svg.findall(".//{*}g") if any(t.text == name for t in g.findall("{*}text")))
                labels = [t.text for t in cell.findall("{*}text")]
                self.assertEqual(labels[:3], [name, tally, pct])
                if name == "memory.dll":
                    self.assertIn("1/12 functions, 1/3 data", labels)
                    self.assertIn("1/12 functions, 1/3 data", table)
                row = next(line for line in table.splitlines() if line.startswith(name))
                self.assertEqual(row.rsplit(maxsplit=3)[-3:], [tally, pct, "Partial"])
                self.assertIn("[", row)
                widths = [float(r.get("width")) for r in cell.findall("{*}rect")]
                if pct == "Unmeasured":
                    self.assertIsNotNone(cell.find("{*}line"))
                elif pct == "0%":
                    self.assertEqual(len(widths), 1)
                else:
                    numerator, denominator = map(int, tally.split("/"))
                    self.assertAlmostEqual(widths[1] / widths[0], numerator/denominator, places=5)

    def test_inconsistent_export_counts_are_rejected_instead_of_plotted(self):
        for total, verified in [(1, ["One", "Two"]), (2, ["One", "One"]), (-1, []), (2, "fn")]:
            with self.subTest(total=total, verified=verified), self.assertRaisesRegex(ValueError, "sample.dll"):
                export_progress({"name": "sample.dll", "export_coverage": {"total": total, "verified": verified}})

    def test_frosty_imports_match_export_slots_and_reviewed_build(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            rows = [{"name": name, "status": "partial", "group": "Test", "scope": "", "provider": "Test",
                     "export_coverage": {"total": 3, "verified": ["First", "#2"], "binary_sha256": digest}}
                    for name, digest in [("Sample.dll", "reviewed"), ("Other.dll", "previous")]]
            entries = [{"ordinal": 1, "names": ["First", "AlsoFirst"], "rva": "0x1000"},
                       {"ordinal": 2, "names": ["Second"], "rva": "0x2000"},
                       {"ordinal": 3, "names": ["Third"], "rva": "0x2000"}]
            inventory = {
                "title": {"imports": [
                    {"module": "sample.DLL", "table": "imports", "symbols": ["AlsoFirst", "#1", "Second"]},
                    {"module": "SAMPLE.dll", "table": "delay_imports", "symbols": ["First", "#3"]},
                    {"module": "Other.dll", "table": "imports", "symbols": ["First"]}]},
                "libraries": [{"name": name, "sha256": digest, "exports": {"entries": entries}}
                              for name, digest in [("sample.dll", "reviewed"), ("Other.dll", "current")]]}
            (root / "library-status.json").write_text(json.dumps({"libraries": rows}))
            (root / "frosty-xbox-library-inventory.json").write_text(json.dumps(inventory))
            table = library_table(root)
            self.assertRegex(table, r"Sample\.dll[^\n]*direct\s+2/3")
            self.assertRegex(table, r"Other\.dll[^\n]*direct\s+Unmeasured")
            self.assertIn("verified 2/3 distinct Frosty imports", table)
            self.assertIn("covers 1 of 2 directly imported tracked DLLs", table)
            svg = ET.fromstring(render_map(root))
            tips = [node.text for node in svg.findall(".//{*}title")]
            self.assertTrue(any("Frosty direct imports verified 2/3" in tip and
                                "Verified (AlsoFirst, Second). Unverified (#3)." in tip for tip in tips))
            self.assertTrue(any("Other.dll" in tip and "Unmeasured" in tip for tip in tips))

    def test_current_trial_takes_precedence_over_later_observations(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            file = root / "matrix-awakens-launch.json"
            latest = {"missing_modules": ["Unknown.dll"], "unresolved_direct_symbols": []}
            earlier = {"missing_modules": ["ONE.dll"], "unresolved_direct_symbols": ["x!a"]}
            data = {"trials": {"earlier": earlier, "latest": latest}, "current_trial": "earlier",
                    "imports_from_missing_modules": {"One.DLL": ["a", "b"]}}
            file.write_text(json.dumps(data))
            current = render_map(root)
            self.assertIn("Trial earlier", current)
            self.assertIn("ONE.dll", current)
            self.assertNotIn("Unknown.dll", current)
            self.assertIn("Direct imports in missing DLLs: 2", current)
            earlier.update(stage="title-code", frame_verified=False,
                           observed_failure={"kind": "unimplemented-import", "symbol": "x!a",
                                             "title_frame_rva": "0x24", "exception_code": "0x80000100"})
            file.write_text(json.dumps(data))
            advanced = render_map(root)
            for proof in ["Title code reached", "Frame unverified", "Unimplemented import x!a", "Title stack RVA 0x24"]:
                self.assertIn(proof, advanced)
            earlier["observed_failure"].update(kind="access-violation", exception_code="0xc0000005",
                                              title_diagnostic="XGameRuntimeInitialize failed: 0x80070001")
            file.write_text(json.dumps(data))
            report = render_map(root)
            self.assertLess(report.index("XGameRuntimeInitialize failed"), report.index("Access violation"))
            del data["current_trial"]
            file.write_text(json.dumps(data))
            self.assertIn("Unknown.dll", render_map(root))
            self.assertIn("Direct imports in missing DLLs: unknown", render_map(root))
            data["current_trial"] = "typo"
            file.write_text(json.dumps(data))
            with self.assertRaisesRegex(ValueError, "Unknown current retail trial"):
                render_map(root)

    def test_retail_summary_requires_matching_title(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "matrix-awakens-launch.json").write_text(json.dumps({
                "sha256": "title1", "trials": {"earlier": {}}, "chase_display_receipt": "replay.json"}))
            receipt = {"title_sha256": "title2", "fraction_pixels_with_all_rgb_channels_at_most_12": 0.995,
                       "driving_and_shooting_verified": False}
            file = root / "replay.json"
            file.write_text(json.dumps({"desktop_extended_trial": receipt}))
            self.assertNotIn("Near-black", render_map(root))
            (root / "matrix-awakens").mkdir()
            menu = root / "matrix-awakens/menu-background.json"
            menu.write_text(json.dumps({"title_sha256": "other", "verified": False, "observation": "Black"}))
            receipt["title_sha256"] = "title1"
            file.write_text(json.dumps({"desktop_extended_trial": receipt}))
            graphic = render_map(root)
            self.assertIn("Near-black", graphic)
            self.assertIn("Unverified", graphic)
            self.assertNotIn("Menu code background", graphic)
            menu.write_text(json.dumps({"title_sha256": "title1", "verified": False, "observation": "Black"}))
            self.assertIn("Menu code background", render_map(root))
            file.unlink()
            self.assertIn("Menu code background", render_map(root))
            self.assertNotIn("Near-black", render_map(root))

    def test_independent_menu_uses_the_matching_background_measurement(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            evidence = root / "matrix-awakens"
            evidence.mkdir()
            (root / "matrix-awakens-launch.json").write_text(json.dumps({"sha256": "game", "trials": {"trial": {}}}))
            (evidence / "independent-menu.json").write_text(json.dumps({
                "executable_sha256": "game", "captures": {"independent-menu.webp": "capture"}}))
            background = evidence / "menu-background.json"
            background.write_text(json.dumps({"title_sha256": "different", "verified": True}))
            self.assertNotIn(">Animated<", render_map(root))
            background.write_text(json.dumps({"title_sha256": "game", "verified": True}))
            graphic = render_map(root)
            self.assertIn(">Animated<", graphic)
            self.assertIn("Retail launch with TuxBox providers", graphic)
