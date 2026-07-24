#!/usr/bin/env python3

import io
import json
import os
import tarfile
import tempfile
import unittest
from pathlib import Path

from docs_preview_bundle import BundleError, Limits, extract_bundle, seal_bundle


def file_entry(name, content=b"x", mode=0o644):
    return ("file", name, content, mode)


def directory_entry(name):
    return ("directory", name, b"", 0o755)


def valid_entries(prefix=""):
    output = f"{prefix}.vercel/output"
    function = f"{output}/functions/app.func"
    materialized = f"{output}/.docs-preview-files/library"
    config = {
        "runtime": "nodejs20.x",
        "handler": "index.js",
        "filePathMap": {
            "index.js": ".vercel/output/.docs-preview-files/library",
        },
    }
    return [
        directory_entry(output),
        directory_entry(function),
        directory_entry(f"{output}/.docs-preview-files"),
        file_entry(f"{function}/.vc-config.json", json.dumps(config).encode()),
        file_entry(materialized, b"module.exports = {}"),
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
        self.assertFalse(
            self.destination.exists(), "failed extraction must not be published"
        )
        self.assertEqual(
            list(self.root.glob(".deploy.*")),
            [],
            "failed extraction must remove its staging directory",
        )

    @staticmethod
    def replace_config(entries, config):
        index = next(
            i for i, entry in enumerate(entries) if entry[1].endswith(".vc-config.json")
        )
        entries[index] = file_entry(
            entries[index][1],
            json.dumps(config).encode(),
        )

    def test_extracts_valid_self_contained_bundle(self):
        self.extract(valid_entries())
        self.assertTrue(
            (
                self.destination / ".vercel/output/functions/app.func/.vc-config.json"
            ).is_file()
        )
        self.assertTrue(
            (self.destination / ".vercel/output/.docs-preview-files/library").is_file()
        )

    def test_accepts_explicit_dot_slash_prefix(self):
        self.extract(valid_entries(prefix="./"))
        self.assertTrue((self.destination / ".vercel/output").is_dir())

    def test_accepts_bounded_long_name_metadata(self):
        long_path = ".vercel/output/" + "/".join(["nested"] * 20) + "/index.js"
        self.extract(valid_entries() + [file_entry(long_path)])
        self.assertTrue((self.destination / long_path).is_file())

    def test_rejects_traversal_absolute_backslash_and_empty_components(self):
        unsafe_names = [
            "../../.vercel/output/escape",
            "/.vercel/output/escape",
            ".vercel\\output\\escape",
            ".vercel/output//escape",
            ".vercel/output/./escape",
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
                ("symlink", ".vercel/output/link", b"/etc/passwd", 0o777),
                (
                    "hardlink",
                    ".vercel/output/link",
                    b".vercel/output/.docs-preview-files/library",
                    0o644,
                ),
                ("fifo", ".vercel/output/pipe", b"", 0o644),
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
            + [
                file_entry(
                    ".vercel/output/.docs-preview-files/library",
                    b"replacement",
                )
            ]
        )

    def test_rejects_pax_overrides_before_tarfile_parses_them(self):
        for kind in ("pax-size", "pax-unknown"):
            with self.subTest(kind=kind):
                self.assert_rejected(
                    valid_entries()
                    + [(kind, ".vercel/output/override.js", b"x", 0o644)]
                )

    def test_enforces_tar_metadata_member_limit(self):
        long_path = ".vercel/output/" + "/".join(["nested"] * 20) + "/index.js"
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

    def test_rejects_file_path_map_escape_and_missing_reference(self):
        for index, reference in enumerate(
            ["../../proc/self/cmdline", ".vercel/output/missing.js"]
        ):
            entries = valid_entries()
            self.replace_config(
                entries,
                {
                    "runtime": "nodejs20.x",
                    "handler": "index.js",
                    "filePathMap": {"index.js": reference},
                },
            )
            archive = self.root / f"reference-{index}.tgz"
            destination = self.root / f"reference-{index}"
            write_archive(archive, entries)
            with self.assertRaises(BundleError):
                extract_bundle(archive, destination)
            self.assertFalse(destination.exists())

    def test_rejects_unsafe_file_path_map_bundle_paths(self):
        for index, bundle_path in enumerate(
            ["../index.js", "/index.js", "dir\\index.js", "dir//index.js"]
        ):
            entries = valid_entries()
            self.replace_config(
                entries,
                {
                    "runtime": "nodejs20.x",
                    "handler": "index.js",
                    "filePathMap": {
                        bundle_path: ".vercel/output/.docs-preview-files/library"
                    },
                },
            )
            archive = self.root / f"bundle-path-{index}.tgz"
            destination = self.root / f"bundle-path-{index}"
            write_archive(archive, entries)
            with self.assertRaises(BundleError):
                extract_bundle(archive, destination)
            self.assertFalse(destination.exists())

    def test_rejects_handler_escape_and_missing_handler(self):
        for index, handler in enumerate(["../index.js", "missing.js"]):
            entries = valid_entries()
            self.replace_config(
                entries,
                {
                    "runtime": "nodejs20.x",
                    "handler": handler,
                    "filePathMap": {
                        "index.js": ".vercel/output/.docs-preview-files/library"
                    },
                },
            )
            archive = self.root / f"handler-{index}.tgz"
            destination = self.root / f"handler-{index}"
            write_archive(archive, entries)
            with self.assertRaises(BundleError):
                extract_bundle(archive, destination)
            self.assertFalse(destination.exists())

    def test_rejects_nul_in_handler_and_cleans_staging(self):
        entries = valid_entries()
        self.replace_config(
            entries,
            {"runtime": "nodejs20.x", "handler": "index.js\u0000"},
        )
        self.assert_rejected(entries)

    def test_rejects_non_standard_json_constants(self):
        entries = valid_entries()
        index = next(
            i for i, entry in enumerate(entries) if entry[1].endswith(".vc-config.json")
        )
        entries[index] = file_entry(
            entries[index][1],
            b'{"runtime":"nodejs20.x","handler":NaN}',
        )
        self.assert_rejected(entries)

    def test_malformed_gzip_is_reported_as_bundle_error(self):
        self.archive.write_bytes(b"not gzip")
        with self.assertRaises(BundleError):
            extract_bundle(self.archive, self.destination)
        self.assertFalse(self.destination.exists())

    def test_rejects_excessive_file_path_map_references(self):
        self.assert_rejected(
            valid_entries(),
            limits=Limits(max_file_path_references=0),
        )

    def test_rejects_excessive_file_path_map_entries_per_config(self):
        self.assert_rejected(
            valid_entries(),
            limits=Limits(max_file_path_references_per_config=0),
        )

    def test_seal_materializes_external_references_and_rewrites_config(self):
        source = self.root / "source"
        function = source / ".vercel/output/functions/app.func"
        dependency = source / "website/node_modules/pkg/index.js"
        function.mkdir(parents=True)
        dependency.parent.mkdir(parents=True)
        dependency.write_text("module.exports = 1", encoding="utf-8")
        config = {
            "runtime": "nodejs20.x",
            "handler": "index.js",
            "filePathMap": {"index.js": "website/node_modules/pkg/index.js"},
        }
        config_path = function / ".vc-config.json"
        config_path.write_text(json.dumps(config), encoding="utf-8")

        seal_bundle(source, self.archive)
        rewritten = json.loads(config_path.read_text(encoding="utf-8"))
        materialized = rewritten["filePathMap"]["index.js"]
        self.assertTrue(materialized.startswith(".vercel/output/.docs-preview-files/"))

        extract_bundle(self.archive, self.destination)
        self.assertTrue((self.destination / materialized).is_file())
        self.assertFalse((self.destination / "website").exists())

    def test_seal_rejects_reference_that_resolves_outside_source_root(self):
        source = self.root / "source"
        function = source / ".vercel/output/functions/app.func"
        function.mkdir(parents=True)
        (function / ".vc-config.json").write_text(
            json.dumps(
                {
                    "runtime": "nodejs20.x",
                    "handler": "index.js",
                    "filePathMap": {"index.js": "../outside.js"},
                }
            ),
            encoding="utf-8",
        )
        with self.assertRaises(BundleError):
            seal_bundle(source, self.archive)
        self.assertFalse(self.archive.exists())

    def test_preserves_only_executable_or_non_executable_modes(self):
        entries = valid_entries()
        script = ".vercel/output/tool"
        entries.append(file_entry(script, b"#!/bin/sh\n", mode=0o4777))
        self.extract(entries)
        extracted_mode = os.stat(self.destination / script).st_mode & 0o7777
        self.assertEqual(extracted_mode, 0o755)


if __name__ == "__main__":
    unittest.main()
