#!/usr/bin/env python3

from __future__ import annotations

import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class PrepareSdkDistributionTests(unittest.TestCase):
    def test_stamps_stable_version_and_removes_monorepo_inputs(self) -> None:
        repository = Path(__file__).resolve().parents[2]
        script = Path(__file__).with_name("prepare-sdk-dist.py")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "sdk/python"
            package.mkdir(parents=True)
            source_manifest = repository / "sdk/python/pyproject.toml"
            source_text = source_manifest.read_text()
            version = re.search(
                r'^version = "([0-9]+\.[0-9]+\.[0-9]+)\.dev0"$', source_text, flags=re.M
            )
            self.assertIsNotNone(version)
            shutil.copy(source_manifest, package / "pyproject.toml")
            shutil.copy(repository / "sdk/python/uv.lock", package / "uv.lock")
            subprocess.run(
                [sys.executable, str(script)],
                cwd=root,
                check=True,
            )

            manifest = (package / "pyproject.toml").read_text()
            self.assertIn(f'version = "{version.group(1)}"', manifest)
            self.assertNotIn("[tool.uv.sources]", manifest)
            self.assertFalse((package / "uv.lock").exists())


if __name__ == "__main__":
    unittest.main()
