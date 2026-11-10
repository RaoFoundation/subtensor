#!/usr/bin/env python3
"""Offline checks for the version guard shared by PR CI and the release train."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch
from urllib.error import HTTPError, URLError


spec = importlib.util.spec_from_file_location(
    "release_python_versions", Path(__file__).with_name("release-python-versions.py")
)
versions = importlib.util.module_from_spec(spec)
spec.loader.exec_module(versions)


def http_error(status: int) -> HTTPError:
    return HTTPError("https://pypi.org", status, "test response", {}, None)


class ClientVersionTests(unittest.TestCase):
    def test_unpublished_stable_versions_pass(self) -> None:
        with patch.object(
            versions, "urlopen", side_effect=[http_error(404), http_error(404)]
        ) as request:
            versions.check_unpublished("11.3.3", "0.1.10")
        self.assertEqual(
            [call.args[0] for call in request.call_args_list],
            [
                "https://pypi.org/pypi/bittensor/11.3.3/json",
                "https://pypi.org/pypi/bittensor-core/0.1.10/json",
            ],
        )

    def test_either_published_package_blocks_release(self) -> None:
        published = MagicMock()
        published.__enter__.return_value.status = 200
        for package, replies in (
            ("bittensor 11.3.2", [published]),
            ("bittensor-core 0.1.10", [http_error(404), published]),
        ):
            with self.subTest(package=package):
                with patch.object(versions, "urlopen", side_effect=replies):
                    with self.assertRaisesRegex(
                        ValueError,
                        f"{package} is already published; bump the base version",
                    ):
                        versions.check_unpublished("11.3.2", "0.1.10")

    def test_registry_errors_never_pass(self) -> None:
        for failure in (
            http_error(403),
            http_error(429),
            http_error(500),
            URLError("offline"),
            TimeoutError(),
        ):
            for prefix in ([], [http_error(404)]):
                with self.subTest(failure=failure, core=bool(prefix)):
                    with patch.object(
                        versions, "urlopen", side_effect=[*prefix, failure]
                    ):
                        with self.assertRaisesRegex(ValueError, "PyPI availability"):
                            versions.check_unpublished("11.3.3", "0.1.10")

    def test_unexpected_success_status_does_not_prove_availability(self) -> None:
        response = MagicMock()
        response.__enter__.return_value.status = 204
        with patch.object(versions, "urlopen", return_value=response):
            with self.assertRaisesRegex(ValueError, "HTTP 204"):
                versions.check_unpublished("11.3.3", "0.1.10")

    def test_cli_checks_stable_base_and_preserves_offline_mode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            sdk = Path(directory) / "sdk.toml"
            core = Path(directory) / "core.toml"
            sdk.write_text('[project]\nversion = "11.3.3.dev0"\n')
            core.write_text('[project]\nversion = "0.1.10"\n')
            for check in (False, True):
                arguments = ["release-python-versions.py", str(sdk), str(core)]
                if check:
                    arguments.append("--check-unpublished")
                with self.subTest(check=check):
                    with (
                        patch("sys.argv", arguments),
                        patch.object(
                            versions,
                            "urlopen",
                            side_effect=[http_error(404), http_error(404)],
                        ) as request,
                        contextlib.redirect_stdout(io.StringIO()) as output,
                    ):
                        self.assertEqual(versions.main(), 0)
                    self.assertEqual(request.call_count, 2 if check else 0)
                    self.assertEqual(
                        json.loads(output.getvalue()),
                        {"sdk": "11.3.3", "core": "0.1.10"},
                    )

    def test_cli_rejects_published_version_without_emitting_versions(self) -> None:
        with (
            patch(
                "sys.argv",
                [
                    "release-python-versions.py",
                    "sdk.toml",
                    "core.toml",
                    "--check-unpublished",
                ],
            ),
            patch.object(
                Path,
                "read_text",
                side_effect=['version = "11.3.2.dev0"', 'version = "0.1.10"'],
            ),
            patch.object(versions, "urlopen", side_effect=http_error(200)),
            contextlib.redirect_stdout(io.StringIO()) as output,
            contextlib.redirect_stderr(io.StringIO()) as error,
        ):
            self.assertEqual(versions.main(), 1)
        self.assertEqual(output.getvalue(), "")
        self.assertIn("bittensor 11.3.2 is already published", error.getvalue())

    def test_invalid_versions_fail_before_querying_pypi(self) -> None:
        for sdk, core in (("11.3.3", "0.1.10"), ("11.3.3.dev0", "0.1.10rc1")):
            with self.subTest(sdk=sdk, core=core):
                with self.assertRaises(ValueError):
                    versions.release_versions(
                        f'version = "{sdk}"', f'version = "{core}"'
                    )


if __name__ == "__main__":
    unittest.main()
