#!/usr/bin/env python3

from __future__ import annotations

import unittest

from python_rc_plan import (
    ApiError,
    InconsistentPackageState,
    base_versions,
    publication_outputs,
    version_is_published,
)


class PythonRcPlanTests(unittest.TestCase):
    def test_reads_stable_bases_from_manifests(self) -> None:
        self.assertEqual(
            base_versions(
                '[project]\nversion = "11.0.2.dev0"\n',
                '[project]\nversion = "0.1.2"\n',
            ),
            ("11.0.2", "0.1.2"),
        )

    def test_rejects_non_release_manifest_versions(self) -> None:
        with self.assertRaisesRegex(ValueError, "X.Y.Z.dev0"):
            base_versions(
                '[project]\nversion = "11.0.2"\n',
                '[project]\nversion = "0.1.2"\n',
            )

    def test_publishes_when_both_stable_bases_are_available(self) -> None:
        self.assertEqual(
            publication_outputs(
                "11.0.2",
                "0.1.2",
                "123",
                is_published=lambda _package, _version: False,
            ),
            ["publish=true", "sdk=11.0.2rc123", "core=0.1.2rc123"],
        )

    def test_skips_when_both_stable_bases_are_published(self) -> None:
        self.assertEqual(
            publication_outputs(
                "11.0.1",
                "0.1.1",
                "123",
                is_published=lambda _package, _version: True,
            ),
            ["publish=false"],
        )

    def test_rejects_either_mixed_package_state(self) -> None:
        for published_package in ("bittensor", "bittensor-core"):
            with self.subTest(published_package=published_package):
                with self.assertRaises(InconsistentPackageState):
                    publication_outputs(
                        "11.0.2",
                        "0.1.2",
                        "123",
                        is_published=lambda package, _version: (
                            package == published_package
                        ),
                    )

    def test_maps_only_200_and_404_to_package_state(self) -> None:
        self.assertTrue(
            version_is_published(
                "bittensor",
                "11.0.2",
                fetch_status=lambda _url: 200,
            )
        )
        self.assertFalse(
            version_is_published(
                "bittensor",
                "11.0.2",
                fetch_status=lambda _url: 404,
            )
        )
        with self.assertRaises(ApiError):
            version_is_published(
                "bittensor",
                "11.0.2",
                fetch_status=lambda _url: 503,
            )


if __name__ == "__main__":
    unittest.main()
