#!/usr/bin/env python3

import tempfile
import unittest
import warnings
import zipfile
from pathlib import Path

from docs_preview_artifact import ArtifactError, Limits, extract_artifact


class DocsPreviewArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.archive = self.root / "artifact.zip"
        self.destination = self.root / "artifact"

    def tearDown(self):
        self.temp.cleanup()

    def write_zip(self, entries):
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            with zipfile.ZipFile(
                self.archive, "w", compression=zipfile.ZIP_DEFLATED
            ) as archive:
                for name, content in entries:
                    archive.writestr(name, content)

    def assert_rejected(self, entries, limits=None):
        self.write_zip(entries)
        with self.assertRaises(ArtifactError):
            extract_artifact(self.archive, self.destination, limits=limits)
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.root.glob(".artifact.*")), [])

    def test_extracts_deploy_and_cleanup_artifacts(self):
        for index, entries in enumerate(
            [
                [
                    ("docs-preview-action.txt", b"deploy\n"),
                    ("docs-preview-pr-number.txt", b"42\n"),
                    ("docs-preview-sealed.tgz", b"bundle"),
                ],
                [
                    ("docs-preview-action.txt", b"cleanup\n"),
                    ("docs-preview-pr-number.txt", b"42\n"),
                ],
            ]
        ):
            with self.subTest(index=index):
                self.archive = self.root / f"artifact-{index}.zip"
                self.destination = self.root / f"artifact-{index}"
                self.write_zip(entries)
                extract_artifact(self.archive, self.destination)
                self.assertEqual(
                    (self.destination / "docs-preview-action.txt").read_bytes(),
                    entries[0][1],
                )

    def test_rejects_paths_duplicates_and_extra_entries(self):
        cases = [
            [
                ("docs-preview-action.txt", b"deploy\n"),
                ("docs-preview-pr-number.txt", b"42\n"),
                ("../docs-preview-sealed.tgz", b"bundle"),
            ],
            [
                ("docs-preview-action.txt", b"deploy\n"),
                ("docs-preview-action.txt", b"deploy\n"),
                ("docs-preview-pr-number.txt", b"42\n"),
            ],
            [
                ("docs-preview-action.txt", b"deploy\n"),
                ("docs-preview-pr-number.txt", b"42\n"),
                ("docs-preview-sealed.tgz", b"bundle"),
                ("extra", b"x"),
            ],
        ]
        for index, entries in enumerate(cases):
            with self.subTest(index=index):
                self.archive = self.root / f"unsafe-{index}.zip"
                self.destination = self.root / f"unsafe-{index}"
                self.assert_rejected(entries)

    def test_rejects_symlink_entry(self):
        with zipfile.ZipFile(self.archive, "w") as archive:
            archive.writestr("docs-preview-action.txt", b"deploy\n")
            archive.writestr("docs-preview-pr-number.txt", b"42\n")
            info = zipfile.ZipInfo("docs-preview-sealed.tgz")
            info.create_system = 3
            info.external_attr = 0o120777 << 16
            archive.writestr(info, b"/etc/passwd")
        with self.assertRaises(ArtifactError):
            extract_artifact(self.archive, self.destination)
        self.assertFalse(self.destination.exists())

    def test_enforces_file_total_zip_and_central_directory_limits(self):
        self.write_zip(
            [
                ("docs-preview-action.txt", b"deploy\n"),
                ("docs-preview-pr-number.txt", b"42\n"),
                ("docs-preview-sealed.tgz", b"bundle"),
            ]
        )
        size = self.archive.stat().st_size
        cases = [
            Limits(max_zip_bytes=size - 1),
            Limits(max_bundle_bytes=5),
            Limits(max_total_bytes=10),
            Limits(max_central_directory_bytes=10),
        ]
        for index, limits in enumerate(cases):
            with self.subTest(index=index):
                destination = self.root / f"limit-{index}"
                with self.assertRaises(ArtifactError):
                    extract_artifact(self.archive, destination, limits=limits)
                self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
