#!/usr/bin/env python3

import io
import json
import os
import tarfile
import tempfile
import unittest
from pathlib import Path

from docs_preview_bundle import BundleError, Limits, extract_bundle


def file_entry(name, content=b"x", mode=0o644):
    return ("file", name, content, mode)


def directory_entry(name):
    return ("directory", name, b"", 0o755)


def valid_entries(prefix=""):
    function = f"{prefix}.vercel/output/functions/app.func"
    return [
        directory_entry(f"{prefix}.vercel/output"),
        directory_entry(function),
        file_entry(
            f"{function}/.vc-config.json",
            json.dumps({"runtime": "nodejs20.x", "handler": "index.js"}).encode(),
        ),
        file_entry(f"{function}/index.js", b"module.exports = {}"),
        file_entry(
            f"{function}/index.nft.json",
            json.dumps(
                {
                    "version": 1,
                    "files": [
                        "index.js",
                        "../../../../website/node_modules/pkg/index.js",
                    ],
                }
            ).encode(),
        ),
        directory_entry(f"{prefix}website/node_modules/pkg"),
        file_entry(f"{prefix}website/node_modules/pkg/index.js", b"module.exports = 1"),
    ]


def write_archive(path, entries):
    with tarfile.open(path, "w:gz") as archive:
        for kind, name, content, mode in entries:
            info = tarfile.TarInfo(name)
            info.mode = mode
            if kind == "file":
                info.size = len(content)
                archive.addfile(info, io.BytesIO(content))
            elif kind == "directory":
                info.type = tarfile.DIRTYPE
                archive.addfile(info)
            elif kind == "symlink":
                info.type = tarfile.SYMTYPE
                info.linkname = content.decode()
                archive.addfile(info)
            elif kind == "hardlink":
                info.type = tarfile.LNKTYPE
                info.linkname = content.decode()
                archive.addfile(info)
            elif kind == "fifo":
                info.type = tarfile.FIFOTYPE
                archive.addfile(info)
            elif kind == "pax-size":
                info.size = len(content)
                info.pax_headers = {"size": "999999999"}
                archive.addfile(info, io.BytesIO(content))
            elif kind == "pax-unknown":
                info.size = len(content)
                info.pax_headers = {"uid": "999"}
                archive.addfile(info, io.BytesIO(content))
            else:
                raise AssertionError(kind)


class DocsPreviewBundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.archive = self.root / "bundle.tgz"
        self.destination = self.root / "deploy"

    def tearDown(self):
        self.temp.cleanup()

    def extract(self, entries, limits=None):
        write_archive(self.archive, entries)
        extract_bundle(self.archive, self.destination, limits=limits)

    def assert_rejected(self, entries, limits=None):
        write_archive(self.archive, entries)
        with self.assertRaises(BundleError):
            extract_bundle(self.archive, self.destination, limits=limits)
        self.assertFalse(self.destination.exists(), "failed extraction must not be published")
        self.assertEqual(
            list(self.root.glob(".deploy.*")),
            [],
            "failed extraction must remove its staging directory",
        )

    def test_extracts_valid_bundle(self):
        self.extract(valid_entries())
        self.assertTrue(
            (
                self.destination
                / ".vercel/output/functions/app.func/.vc-config.json"
            ).is_file()
        )
        self.assertTrue(
            (self.destination / "website/node_modules/pkg/index.js").is_file()
        )

    def test_accepts_explicit_dot_slash_prefix(self):
        self.extract(valid_entries(prefix="./"))
        self.assertTrue((self.destination / ".vercel/output").is_dir())

    def test_accepts_bounded_long_name_metadata(self):
        long_path = "website/node_modules/" + "/".join(["nested"] * 20) + "/index.js"
        entries = valid_entries() + [file_entry(long_path)]
        self.extract(entries)
        self.assertTrue((self.destination / long_path).is_file())

    def test_rejects_traversal_absolute_backslash_and_empty_components(self):
        unsafe_names = [
            "../../website/node_modules/escape",
            "/website/node_modules/escape",
            "website\\node_modules\\escape",
            "website//node_modules/escape",
            "website/./node_modules/escape",
        ]
        for index, name in enumerate(unsafe_names):
            with self.subTest(name=name):
                archive = self.root / f"unsafe-{index}.tgz"
                destination = self.root / f"unsafe-{index}"
                write_archive(archive, valid_entries() + [file_entry(name)])
                with self.assertRaises(BundleError):
                    extract_bundle(archive, destination)
                self.assertFalse(destination.exists())

    def test_rejects_unexpected_top_level_path(self):
        self.assert_rejected(valid_entries() + [file_entry("package.json")])

    def test_rejects_links_and_special_files(self):
        for index, entry in enumerate(
            [
                ("symlink", "website/node_modules/link", b"/etc/passwd", 0o777),
                (
                    "hardlink",
                    "website/node_modules/link",
                    b"website/node_modules/pkg/index.js",
                    0o644,
                ),
                ("fifo", "website/node_modules/pipe", b"", 0o644),
            ]
        ):
            with self.subTest(kind=entry[0]):
                archive = self.root / f"special-{index}.tgz"
                destination = self.root / f"special-{index}"
                write_archive(archive, valid_entries() + [entry])
                with self.assertRaises(BundleError):
                    extract_bundle(archive, destination)
                self.assertFalse(destination.exists())

    def test_rejects_duplicate_member(self):
        self.assert_rejected(
            valid_entries()
            + [file_entry("website/node_modules/pkg/index.js", b"replacement")]
        )

    def test_rejects_pax_size_override_before_tarfile_parses_it(self):
        self.assert_rejected(
            valid_entries()
            + [
                (
                    "pax-size",
                    "website/node_modules/pkg/override.js",
                    b"x",
                    0o644,
                )
            ]
        )

    def test_rejects_unsupported_pax_parser_inputs(self):
        self.assert_rejected(
            valid_entries()
            + [
                (
                    "pax-unknown",
                    "website/node_modules/pkg/override.js",
                    b"x",
                    0o644,
                )
            ]
        )

    def test_enforces_tar_metadata_member_limit(self):
        long_path = "website/node_modules/" + "/".join(["nested"] * 20) + "/index.js"
        self.assert_rejected(
            valid_entries() + [file_entry(long_path)],
            limits=Limits(max_metadata_member_bytes=8),
        )

    def test_enforces_member_file_total_and_compressed_limits(self):
        base = valid_entries()
        write_archive(self.archive, base)
        archive_size = self.archive.stat().st_size
        cases = [
            Limits(max_members=len(base) - 1),
            Limits(max_file_bytes=4),
            Limits(max_total_bytes=8),
            Limits(max_archive_bytes=archive_size - 1),
        ]
        for index, limits in enumerate(cases):
            with self.subTest(limit=index):
                destination = self.root / f"limit-{index}"
                with self.assertRaises(BundleError):
                    extract_bundle(self.archive, destination, limits=limits)
                self.assertFalse(destination.exists())

    def test_rejects_nft_path_escape_and_missing_reference(self):
        for index, reference in enumerate(
            ["../../../../../outside", "../../../../website/node_modules/missing.js"]
        ):
            entries = valid_entries()
            trace_index = next(
                i for i, entry in enumerate(entries) if entry[1].endswith(".nft.json")
            )
            entries[trace_index] = file_entry(
                entries[trace_index][1],
                json.dumps({"version": 1, "files": [reference]}).encode(),
            )
            archive = self.root / f"trace-{index}.tgz"
            destination = self.root / f"trace-{index}"
            write_archive(archive, entries)
            with self.assertRaises(BundleError):
                extract_bundle(archive, destination)
            self.assertFalse(destination.exists())

    def test_rejects_handler_escape_and_missing_handler(self):
        for index, handler in enumerate(
            ["../../../../website/node_modules/pkg/index.js", "missing.js"]
        ):
            entries = valid_entries()
            config_index = next(
                i for i, entry in enumerate(entries) if entry[1].endswith(".vc-config.json")
            )
            entries[config_index] = file_entry(
                entries[config_index][1],
                json.dumps({"runtime": "nodejs20.x", "handler": handler}).encode(),
            )
            archive = self.root / f"handler-{index}.tgz"
            destination = self.root / f"handler-{index}"
            write_archive(archive, entries)
            with self.assertRaises(BundleError):
                extract_bundle(archive, destination)
            self.assertFalse(destination.exists())

    def test_rejects_nul_in_handler_and_cleans_staging(self):
        entries = valid_entries()
        config_index = next(
            i for i, entry in enumerate(entries) if entry[1].endswith(".vc-config.json")
        )
        entries[config_index] = file_entry(
            entries[config_index][1],
            json.dumps({"runtime": "nodejs20.x", "handler": "index.js\u0000"}).encode(),
        )
        self.assert_rejected(entries)

    def test_malformed_gzip_is_reported_as_bundle_error(self):
        self.archive.write_bytes(b"not gzip")
        with self.assertRaises(BundleError):
            extract_bundle(self.archive, self.destination)
        self.assertFalse(self.destination.exists())

    def test_rejects_excessive_trace_references(self):
        entries = valid_entries()
        trace_index = next(
            i for i, entry in enumerate(entries) if entry[1].endswith(".nft.json")
        )
        entries[trace_index] = file_entry(
            entries[trace_index][1],
            json.dumps({"version": 1, "files": ["index.js", "index.js"]}).encode(),
        )
        self.assert_rejected(entries, limits=Limits(max_trace_references=1))

    def test_preserves_only_executable_or_non_executable_modes(self):
        entries = valid_entries()
        script = "website/node_modules/pkg/tool"
        entries.append(file_entry(script, b"#!/bin/sh\n", mode=0o4777))
        self.extract(entries)
        extracted_mode = os.stat(self.destination / script).st_mode & 0o7777
        self.assertEqual(extracted_mode, 0o755)


if __name__ == "__main__":
    unittest.main()
