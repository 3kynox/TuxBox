from pathlib import Path
import os
import tempfile
import unittest
from unittest.mock import patch

from project_tuxbox.host import proton_installation


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
