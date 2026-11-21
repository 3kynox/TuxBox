import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from xml.etree import ElementTree as ET

from project_tuxbox.progress_map import render_map, write_map


class ProgressMapTests(unittest.TestCase):
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
            self.assertIn("example<&>.dll · Complete · Host Pad input", descriptions)

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
            receipt["title_sha256"] = "title1"
            file.write_text(json.dumps({"desktop_extended_trial": receipt}))
            graphic = render_map(root)
            self.assertIn("Near-black", graphic)
            self.assertIn("Unverified", graphic)
