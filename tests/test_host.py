from pathlib import Path
import os
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

from project_tuxbox.console_cbv import configure_console_cbv, console_cbv_layer
from project_tuxbox.host import Host, proton_installation


class SelectProton(unittest.TestCase):
    def test_explicit_runner_in_path_with_spaces(self):
        with tempfile.TemporaryDirectory(prefix="provider with spaces ") as tmp:
            install = Path(tmp).resolve()
            runner = install / "proton"
            runner.touch()
            self.assertEqual(proton_installation(str(runner)), install)
            self.assertEqual(proton_installation(str(install)), install)

    def test_registered_steam_library(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            (root / "steamapps").mkdir()
            library = root / "Steam Library"
            install = library / "steamapps/common/Proton - Experimental"
            install.mkdir(parents=True)
            (install / "proton").touch()
            (root / "steamapps/libraryfolders.vdf").write_text(f'"libraryfolders" {{ "0" {{ "path" "{library}" }} }}')
            with patch.dict(os.environ, {"STEAM_COMPAT_CLIENT_INSTALL_PATH": str(root)}), patch.object(Path, "home", return_value=root):
                self.assertEqual(proton_installation(None), install)

    @unittest.skipUnless(sys.platform == "linux", "Wine prefix drive links and POSIX modes require Linux")
    def test_linux_title_drive_uses_prefix_and_preserves_existing_assignment(self):
        with tempfile.TemporaryDirectory() as tmp:
            prefix = Path(tmp)
            drive = prefix / "dosdevices/t:"
            drive.parent.mkdir()
            host = Host.__new__(Host)
            host.backend, host.command = "proton", ["umu-run"]
            host.env = {"WINEPREFIX": tmp, "TUXBOX_RUNTIME_SHADERS": "0"}
            log = Mock()

            def configured_target():
                host.run_game(prefix / "game.exe", ["-nullrhi"], log)
                return log.run.call_args.kwargs["env"].get("TUXBOX_WINE_TITLE_TEMP")

            scratch = prefix / "drive_t"
            self.assertEqual(configured_target(), str(scratch))
            self.assertEqual(scratch.stat().st_mode & 0o777, 0o700)
            drive.symlink_to("../custom")
            self.assertIsNone(configured_target())
            (prefix / "custom").mkdir()
            self.assertEqual(configured_target(), str(prefix / "custom"))
            host.backend = "native"
            self.assertIsNone(configured_target())

    def test_linux_package_drive_points_at_the_manifest_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            prefix, package = root / "prefix", root / "game"
            (package / "bin").mkdir(parents=True)
            (prefix / "dosdevices").mkdir(parents=True)
            host = Host.__new__(Host)
            host.backend, host.command = "proton", ["umu-run"]
            host.env = {"WINEPREFIX": str(prefix), "TUXBOX_RUNTIME_SHADERS": "0"}
            log = Mock()

            def package_drive(executable):
                host.run_game(executable, ["-nullrhi"], log)
                return log.run.call_args.kwargs["env"].get("TUXBOX_WINE_PACKAGE_DRIVE")

            self.assertIsNone(package_drive(package / "bin/game.exe"))
            (package / "MicrosoftGame.Config").write_text("<Game/>")
            self.assertEqual(package_drive(package / "game.exe"), str(package.resolve()))
            self.assertEqual(package_drive(package / "bin/game.exe"), str(package.resolve()))
            host.backend = "native"
            self.assertIsNone(package_drive(package / "game.exe"))


class ConsoleConstantBuffers(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.stage = self.root / "Sample"
        self.stage.mkdir()
        self.exe = self.stage / "game.exe"
        self.built = self.root / "build/d3d12-smoke/d3d12_xs.dll"
        self.built.parent.mkdir(parents=True)
        self.built.write_bytes(b"a built TuxBox provider")
        self.provider = self.stage / "D3D12_XS.DLL"
        self.provider.write_bytes(self.built.read_bytes())
        self.layer = str(self.root / "build/frosty-cbv-layer")
        self.log = Mock()
        for target, value in (("project_tuxbox.shader_prep.TUXBOX_PROVIDER", self.built),
                              ("project_tuxbox.shader_prep.ROOT", self.root)):
            override = patch(target, value)
            override.start()
            self.addCleanup(override.stop)

    def test_direct_stock_launch_preserves_user_settings_and_host_environment(self):
        host = Host.__new__(Host)
        host.backend, host.command = "proton", ["umu-run"]
        host.env = {"PROTONPATH": str(self.root), "VK_LAYER_PATH": "/user/layers",
                    "VK_ADD_LAYER_PATH": "/user/extra", "VK_INSTANCE_LAYERS": "overlay",
                    "VKD3D_CONFIG": "force_static_cbv", "VKD3D_DISABLE_EXTENSIONS": "first;second",
                    "PRESSURE_VESSEL_FILESYSTEMS_RO": "/user/mount", "TUXBOX_RUNTIME_SHADERS": "0"}
        layer = "/tuxbox/build/frosty-cbv-layer"
        with patch("project_tuxbox.console_cbv.console_cbv_layer", return_value=layer):
            host.run_game(self.exe, [], self.log)
            env = self.log.run.call_args.kwargs["env"]
            self.assertEqual(env["VK_LAYER_PATH"], layer + ":/user/layers")
            self.assertEqual(env["VK_ADD_LAYER_PATH"], "/user/extra")
            self.assertEqual(env["VK_INSTANCE_LAYERS"], "VK_LAYER_TUXBOX_console_cbv:overlay")
            self.assertEqual(env["VKD3D_DISABLE_EXTENSIONS"], "VK_EXT_descriptor_buffer,first;second")
            self.assertEqual(env["VKD3D_CONFIG"], "force_static_cbv")
            self.assertEqual(env["PRESSURE_VESSEL_FILESYSTEMS_RO"], layer + ":/user/mount")
            before = env.copy()
            configure_console_cbv("proton", env, self.exe, self.log.status)
            self.assertEqual(env, before)
            self.assertEqual(host.env["VK_LAYER_PATH"], "/user/layers")
            host.run_game(self.exe, ["-NullRHI"], self.log)
            self.assertEqual(self.log.run.call_args.kwargs["env"]["VK_INSTANCE_LAYERS"], "overlay")

    def test_explicit_fork_feature_and_wine_with_an_older_staged_copy(self):
        sample = self.root / "build/lanes/d3d12_xs.dll"
        sample.parent.mkdir(parents=True)
        sample.write_bytes(b"older sample")
        self.provider.write_bytes(sample.read_bytes())
        (self.root / "tuxbox-features").write_text("cbv_16_byte_alignment\n")
        env = {"PROTONPATH": str(self.root), "VKD3D_DISABLE_EXTENSIONS": "first;VK_EXT_descriptor_buffer",
               "VKD3D_CONFIG": "other"}
        with patch("project_tuxbox.console_cbv.console_cbv_layer", return_value=self.layer) as layer:
            configure_console_cbv("proton", env, self.exe, self.log.status)
            self.assertEqual(env["VKD3D_CONFIG"], "cbv_16_byte_alignment,other")
            self.assertEqual(env["VKD3D_DISABLE_EXTENSIONS"], "first;VK_EXT_descriptor_buffer")
            layer.assert_not_called()
            wine = {"PROTONPATH": str(self.root)}
            configure_console_cbv("wine", wine, self.exe, self.log.status)
            self.assertEqual(wine["VK_ADD_LAYER_PATH"], self.layer)
            self.assertNotIn("VKD3D_CONFIG", wine)
            self.assertNotIn("PRESSURE_VESSEL_FILESYSTEMS_RO", wine)

    def test_native_foreign_missing_and_explicit_comparison(self):
        with patch("project_tuxbox.console_cbv.console_cbv_layer", return_value=self.layer) as layer:
            env = {}
            configure_console_cbv("native", env, self.exe, self.log.status)
            self.provider.write_bytes(b"unknown")
            configure_console_cbv("proton", env, self.exe, self.log.status)
            self.provider.unlink()
            configure_console_cbv("proton", env, self.exe, self.log.status)
            self.assertEqual(env, {})
            layer.assert_not_called()
            env["TUXBOX_REFERENCE_CBV"] = "1"
            configure_console_cbv("proton", env, self.exe, self.log.status)
            self.assertEqual(env["VK_INSTANCE_LAYERS"], "VK_LAYER_TUXBOX_console_cbv")

    def test_rebuilds_missing_or_outdated_layer(self):
        sources = self.root / "tools/reference/cbv-layer"
        sources.mkdir(parents=True)
        script = self.root / "tools/build_frosty_cbv_layer.sh"
        layer = Path(self.layer)
        layer.mkdir(parents=True)
        for path in (script, sources / "layer.c", sources / "frosty.json"):
            path.touch()
            os.utime(path, (10, 10))
        env = {"CC": "clang"}
        with patch("project_tuxbox.console_cbv.ROOT", self.root), \
             patch("project_tuxbox.console_cbv.subprocess.run") as build:
            self.assertEqual(console_cbv_layer(env), self.layer)
            build.assert_called_once_with([script, layer], env=env, check=True)
            build.reset_mock()
            for path in (layer / "frosty.json", layer / "frosty_cbv.so"):
                path.touch()
                os.utime(path, (20, 20))
            console_cbv_layer(env)
            build.assert_not_called()
            os.utime(sources / "layer.c", (30, 30))
            console_cbv_layer(env)
            build.assert_called_once_with([script, layer], env=env, check=True)
