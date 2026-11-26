"""Regression checks for the release PR's published-source comparison."""

import io
import runpy
import subprocess
import tarfile
import tempfile
import unittest
from contextlib import chdir
from pathlib import Path
from unittest.mock import patch

checks = runpy.run_path(Path(__file__).with_name("check-python-release.py"))


class ReleaseSourceTests(unittest.TestCase):
    def test_source_changed_before_the_spec_bump(self):
        filename = "bittensor-1.2.3.tar.gz"
        manifest = b'[project]\nversion = "1.2.3"\n[tool.hatch]\n'
        source = b"VALUE = 1\n"
        data = io.BytesIO()
        with tarfile.open(fileobj=data, mode="w:gz") as archive:
            for path, content in {
                "pyproject.toml": manifest,
                "bittensor/__init__.py": source,
            }.items():
                entry = tarfile.TarInfo(f"bittensor-1.2.3/{path}")
                entry.size = len(content)
                archive.addfile(entry, io.BytesIO(content))

        with tempfile.TemporaryDirectory() as root, chdir(root):
            sdk = Path("sdk/python")
            module = sdk / "bittensor/__init__.py"
            module.parent.mkdir(parents=True)
            (sdk / "pyproject.toml").write_bytes(
                manifest.replace(b'1.2.3"', b'1.2.3.dev0"')
            )
            module.write_bytes(source)
            subprocess.run(["git", "init", "-q"], check=True)
            subprocess.run(["git", "add", "sdk"], check=True)
            self.assertTrue(
                checks["source_matches"](data.getvalue(), filename, sdk=True)
            )

            module.write_text("VALUE = 2\n")
            subprocess.run(["git", "add", "sdk"], check=True)
            subprocess.run(
                [
                    "git",
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "commit",
                    "-qm",
                    "SDK change merged before the release",
                ],
                check=True,
            )
            self.assertEqual(
                subprocess.check_output(["git", "diff", "HEAD", "--", "sdk"]), b""
            )
            self.assertFalse(
                checks["source_matches"](data.getvalue(), filename, sdk=True)
            )

    def test_fresh_pair_or_both_published_not_mixed(self):
        check = checks["check_source"]
        args = dict(
            sdk_version="1.2.3",
            core_version="1.2.3",
            repository="org/repo",
            workflows=(),
            environment="mainnet",
            base_url="https://pypi.org",
        )
        with patch.dict(check.__globals__, fetch_json=lambda _: None):
            self.assertIn("available", check(**args))
        with patch.dict(
            check.__globals__,
            fetch_json=lambda url: {} if "bittensor-core" in url else None,
        ):
            with self.assertRaisesRegex(
                checks["IncompleteRelease"], "one is already on PyPI"
            ):
                check(**args)


if __name__ == "__main__":
    unittest.main()
