import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import Mock, patch

from project_tuxbox.mangohud import performance_capture
from project_tuxbox.performance import summarize_capture


class PerformanceTests(unittest.TestCase):
    def test_managed_capture_uses_its_own_config_and_finalizes_after_game_error(self):
        with TemporaryDirectory(prefix="hud test ") as tmp, patch("shutil.which", return_value="/usr/bin/mangohud"), \
                patch("project_tuxbox.mangohud.monotonic", side_effect=[0.0, 1.0]):
            log = Mock(path=Path(tmp) / "run.log")
            env = {"MANGOHUD_CONFIG": "no_display", "DISABLE_MANGOHUD": "1",
                   "TUXBOX_MANGOHUD_PCI": "0000:01:00.0"}
            with self.assertRaises(KeyboardInterrupt):
                with performance_capture("proton", env, Path("lanes.exe"), log):
                    config = Path(env["MANGOHUD_CONFIGFILE"])
                    self.assertEqual(env["MANGOHUD"], "1")
                    self.assertNotIn("MANGOHUD_CONFIG", env)
                    self.assertNotIn("DISABLE_MANGOHUD", env)
                    self.assertIn(f"output_folder={config.parent}\n", config.read_text())
                    self.assertIn("pci_dev=0000:01:00.0\n", config.read_text())
                    (config.parent / "lanes_2026-11-22_01-00-00.csv").write_text("fps,frametime\n100,10\n")
                    raise KeyboardInterrupt
            self.assertTrue((config.parent / "summary.json").is_file())

    def test_only_title_presentations_count_and_sessions_stay_separate(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            first = root / "Frosty-XSX_2026-11-22_01-00-00.csv"
            first.write_text("v1\n0.8.4\n---SYSTEM INFO---\nos,cpu,gpu\nLinux,CPU,RTX\n"
                             "---FRAME METRICS---\nfps,frametime,cpu_load,elapsed,\n"
                             + "".join(f"100,10,20,{i * 10_000_000},\n" for i in range(1, 98))
                             + "5,200,25,1170000000,\n2,500,30,1670000000,\n"
                             + "0.05,20000,29,21670000000,\n")
            second = root / "frosty-xsx.EXE_2026-11-22_02-00-00.csv"
            second.write_text("frametime,fps\n16,60\n")
            for other in ("steam.exe_2026-11-22_01-00-00.csv",
                          "Frosty-XSX_Helper_2026-11-22_01-00-00.csv",
                          "Frosty-XSX_2026-11-22_01-00-00_summary.csv"):
                (root / other).write_text("fps,frametime\n100,10\n")
            output = summarize_capture(root, root / "Frosty-XSX.exe")
            self.assertEqual(output, root / "summary.json")
            report = json.loads(output.read_text())
            by_name = {s["csv"]: s for s in report["sessions"]}
            self.assertEqual(set(by_name), {first.name, second.name})
            session = by_name[first.name]
            self.assertEqual(session["frames"], 100)
            self.assertEqual(session["sampled_seconds"], 21.67)
            self.assertAlmostEqual(session["mean_fps"], 100 / 21.67)
            self.assertEqual(session["frametime_ms"], {"p50": 10, "p95": 10, "p99": 500, "max": 20000})
            self.assertEqual(by_name[second.name]["mean_fps"], 62.5)

    def test_incomplete_last_row_and_unusable_frame_times(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            file = root / "lanes_2026-11-22_01-00-00.csv"
            file.write_text("\ufeffos,cpu,gpu\nLinux,CPU,GPU\n\nfps,frametime,elapsed\n"
                            "90,0,1\n90,-1,2\n90,nan,3\n90,inf,4\n90,broken,5\n"
                            "90,16.5,16500000\n\n90,20,")
            output = summarize_capture(root, "lanes.exe")
            session, = json.loads(output.read_text())["sessions"]
            self.assertEqual(session["frames"], 1)
            self.assertEqual(session["skipped_rows"], 6)
            self.assertEqual(session["frametime_ms"]["p50"], 16.5)

    def test_uptime_samples_repeated_between_real_frames(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            file = root / "lanes_2026-11-22_01-00-00.csv"
            file.write_text("fps,frametime,elapsed\n"
                            "1.79e-6,558286000,103423845\n100,10,113423845\n"
                            "1.79e-6,558286000,220712860\n0.0073,136667,136887712860\n")
            for run_seconds in (None, 142):
                output = summarize_capture(root, "lanes.exe", run_seconds=run_seconds)
                session, = json.loads(output.read_text())["sessions"]
                self.assertEqual((session["frames"], session["skipped_rows"]), (2, 2))
                self.assertAlmostEqual(session["sampled_seconds"], 136.677)
                self.assertEqual(session["frametime_ms"]["max"], 136667)

    def test_first_frame_can_span_the_start_of_logging(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            file = root / "lanes_2026-11-22_01-00-00.csv"
            file.write_text("fps,frametime,elapsed\n0.05,20000,2000000\n")
            output = summarize_capture(root, "lanes.exe", run_seconds=21)
            session, = json.loads(output.read_text())["sessions"]
            self.assertEqual(session["sampled_seconds"], 20)

    def test_no_title_samples(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            file = root / "lanes_2026-11-22_01-00-00.csv"
            for text in ("", "v1\n0.8.4\n", "fps,frametime\n0,0\n"):
                file.write_text(text)
                self.assertIsNone(summarize_capture(root, "lanes.exe"))
                self.assertFalse((root / "summary.json").exists())
