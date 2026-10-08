#!/usr/bin/env python3

import io
import json
import os
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from docs_preview_bundle import (
    FUNCTION_ALIASES_MANIFEST,
    BundleError,
    Limits,
    _parse_args,
    extract_bundle,
    seal_bundle,
)


def file_entry(name, content=b"x", mode=0o644):
    return ("file", name, content, mode)


def directory_entry(name):
    return ("directory", name, b"", 0o755)


def alias_manifest(aliases, version=1):
    return file_entry(
        FUNCTION_ALIASES_MANIFEST,
        json.dumps(
            {"version": version, "aliases": aliases},
            sort_keys=True,
            separators=(",", ":"),
        ).encode(),
    )


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

    def source_function(self, name="app.func"):
        source = self.root / "source"
        function = source / ".vercel/output/functions" / name
        function.mkdir(parents=True)
        (function / "index.js").write_text("module.exports = 1", encoding="utf-8")
        (function / ".vc-config.json").write_text(
            json.dumps({"runtime": "nodejs20.x", "handler": "index.js"}),
            encoding="utf-8",
        )
        return source, function

    def test_alias_transport_is_explicitly_opt_in_and_default_is_legacy(self):
        self.assertFalse(
            _parse_args(["seal", "source", "archive"]).preserve_function_aliases
        )
        source, function = self.source_function()
        alias = function.with_name("page.func")
        alias.symlink_to("app.func", target_is_directory=True)
        seal_bundle(source, self.archive)
        with tarfile.open(self.archive) as bundle:
            names = bundle.getnames()
            self.assertNotIn(FUNCTION_ALIASES_MANIFEST, names)
            self.assertIn(".vercel/output/functions/page.func/index.js", names)
            self.assertTrue(all(item.isdir() or item.isreg() for item in bundle))
        extract_bundle(self.archive, self.destination)
        restored = self.destination / ".vercel/output/functions/page.func"
        self.assertTrue(restored.is_dir())
        self.assertFalse(restored.is_symlink())

    def test_transports_only_original_function_aliases_as_regular_metadata(self):
        source, function = self.source_function()
        nested = function.parent / "docs"
        nested.mkdir()
        (nested / "page.func").symlink_to("../app.func", target_is_directory=True)
        (function.parent / "page.rsc.func").symlink_to(
            "app.func", target_is_directory=True
        )
        # Identical real functions are not inferred to be aliases.
        _, independent = self.source_function("independent.func")
        self.assertEqual(
            (independent / "index.js").read_bytes(),
            (function / "index.js").read_bytes(),
        )
        seal_bundle(source, self.archive, preserve_function_aliases=True)
        with tarfile.open(self.archive) as bundle:
            names = bundle.getnames()
            self.assertIn(FUNCTION_ALIASES_MANIFEST, names)
            self.assertNotIn(".vercel/output/functions/docs/page.func", names)
            self.assertNotIn(".vercel/output/functions/page.rsc.func", names)
            self.assertIn(".vercel/output/functions/independent.func/index.js", names)
            self.assertTrue(all(item.isdir() or item.isreg() for item in bundle))
        extract_bundle(self.archive, self.destination)
        functions = self.destination / ".vercel/output/functions"
        self.assertEqual(os.readlink(functions / "docs/page.func"), "../app.func")
        self.assertEqual(os.readlink(functions / "page.rsc.func"), "app.func")
        self.assertEqual(
            (functions / "docs/page.func/index.js").read_text(), "module.exports = 1"
        )
        self.assertFalse((functions / "independent.func").is_symlink())
        self.assertFalse((self.destination / FUNCTION_ALIASES_MANIFEST).exists())

    def test_opt_in_preserves_existing_static_and_function_file_link_handling(self):
        source, function = self.source_function()
        static = source / ".vercel/output/static"
        static.mkdir()
        external = source / "shared.txt"
        external.write_text("shared content", encoding="utf-8")
        (static / "page.html").symlink_to(external)
        (function / "shared.txt").symlink_to(external)
        seal_bundle(source, self.archive, preserve_function_aliases=True)
        extract_bundle(self.archive, self.destination)
        for relative in ("static/page.html", "functions/app.func/shared.txt"):
            output = self.destination / ".vercel/output" / relative
            self.assertEqual(output.read_text(), "shared content")
            self.assertFalse(output.is_symlink())

    def test_consumer_accepts_valid_aliases_only_after_regular_validation(self):
        entries = valid_entries() + [alias_manifest({"page.func": "app.func"})]
        self.extract(entries)
        alias = self.destination / ".vercel/output/functions/page.func"
        self.assertTrue(alias.is_symlink())
        self.assertEqual(os.readlink(alias), "app.func")

    def test_alias_archive_still_rejects_raw_symlinks_and_hardlinks(self):
        for kind in ("symlink", "hardlink"):
            with self.subTest(kind=kind):
                self.assert_rejected(
                    valid_entries()
                    + [alias_manifest({"page.func": "app.func"})]
                    + [
                        (
                            kind,
                            ".vercel/output/functions/other.func",
                            b"app.func",
                            0o777,
                        )
                    ]
                )

    def test_rejects_invalid_alias_schema_and_noncanonical_metadata(self):
        invalid = [
            b"not json",
            b"\xff",
            b"[]",
            b"{}",
            b'{"aliases":{"page.func":"app.func"},"version":true}',
            b'{"aliases":{"page.func":"app.func"},"version":2}',
            b'{"aliases":{},"version":1}',
            b'{"aliases":[],"version":1}',
            b'{"aliases":{"page.func":"app.func"},"extra":0,"version":1}',
            b'{"aliases":{"page.func":"app.func"},"version":1,"version":1}',
            b'{"aliases":{"page.func":"missing.func","page.func":"app.func"},"version":1}',
            b'{ "aliases":{"page.func":"app.func"},"version":1}',
            b'{"aliases":{"page.func":"app.func"},"version":1}\n',
        ]
        for content in invalid:
            with self.subTest(content=content):
                self.assert_rejected(
                    valid_entries() + [file_entry(FUNCTION_ALIASES_MANIFEST, content)]
                )

    def test_rejects_alias_and_target_path_escapes_or_non_functions(self):
        invalid = [
            "../page.func",
            "/page.func",
            "dir\\page.func",
            "dir//page.func",
            "dir/./page.func",
            "page.func/",
            "page",
            ".func",
            "app.func/page.func",
            "x\0.func",
            "x" * 256 + ".func",
        ]
        for name in invalid:
            for aliases in ({name: "app.func"}, {"page.func": name}):
                with self.subTest(aliases=aliases):
                    self.assert_rejected(valid_entries() + [alias_manifest(aliases)])

    def test_rejects_alias_chains_cycles_self_and_missing_targets(self):
        invalid = [
            {"page.func": "page.func"},
            {"a.func": "b.func", "b.func": "app.func"},
            {"a.func": "b.func", "b.func": "a.func"},
            {"page.func": "missing.func"},
        ]
        for aliases in invalid:
            with self.subTest(aliases=aliases):
                self.assert_rejected(valid_entries() + [alias_manifest(aliases)])

    def test_rejects_alias_collisions_and_unconfigured_targets(self):
        functions = ".vercel/output/functions"
        cases = [
            ([file_entry(f"{functions}/page.func")], {"page.func": "app.func"}),
            ([directory_entry(f"{functions}/page.func")], {"page.func": "app.func"}),
            ([file_entry(f"{functions}/page.func/child")], {"page.func": "app.func"}),
            ([file_entry(f"{functions}/parent")], {"parent/page.func": "app.func"}),
            ([], {"missing-parent/page.func": "app.func"}),
            ([directory_entry(f"{functions}/empty.func")], {"page.func": "empty.func"}),
            ([file_entry(f"{functions}/file.func")], {"page.func": "file.func"}),
        ]
        for extra, aliases in cases:
            with self.subTest(aliases=aliases, extra=extra):
                self.assert_rejected(
                    valid_entries() + extra + [alias_manifest(aliases)]
                )

    def test_aliases_cannot_make_invalid_regular_maps_valid(self):
        entries = valid_entries()
        self.replace_config(
            entries,
            {
                "runtime": "nodejs20.x",
                "handler": "index.js",
                "filePathMap": {
                    "index.js": ".vercel/output/functions/page.func/index.js"
                },
            },
        )
        self.assert_rejected(entries + [alias_manifest({"page.func": "app.func"})])

    def test_enforces_alias_metadata_count_and_reconstructed_member_limits(self):
        entries = valid_entries() + [alias_manifest({"page.func": "app.func"})]
        cases = [
            Limits(max_function_aliases=0),
            Limits(max_function_alias_manifest_bytes=8),
            Limits(max_members=len(entries)),
            Limits(max_file_path_references=1),
        ]
        for limits in cases:
            with self.subTest(limits=limits):
                self.assert_rejected(entries, limits=limits)

    def test_failed_link_creation_removes_staging_and_never_publishes(self):
        entries = valid_entries() + [
            alias_manifest({"a.func": "app.func", "b.func": "app.func"})
        ]
        write_archive(self.archive, entries)
        original = os.symlink
        calls = []

        def fail_second(source, destination, **kwargs):
            calls.append(destination)
            if len(calls) == 2:
                raise OSError("simulated link creation failure")
            original(source, destination, **kwargs)

        with patch("docs_preview_bundle.os.symlink", side_effect=fail_second):
            with self.assertRaises(BundleError):
                extract_bundle(self.archive, self.destination)
        self.assertEqual(len(calls), 2)
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.root.glob(".deploy.*")), [])

    def test_shared_alias_targets_count_together_with_real_map_references(self):
        entries = valid_entries() + [
            alias_manifest({"a.func": "app.func", "b.func": "app.func"})
        ]
        # One real map plus two aliases of that map needs three references.
        self.assert_rejected(entries, limits=Limits(max_file_path_references=2))

    def test_manifest_must_be_a_regular_file(self):
        self.assert_rejected(
            valid_entries() + [directory_entry(FUNCTION_ALIASES_MANIFEST)]
        )

    def test_producer_rejects_outside_dangling_chained_and_file_targets(self):
        source, function = self.source_function()
        functions = function.parent
        outside = self.root / "outside.func"
        outside.mkdir()
        (functions / "file.func").write_text("not a directory")
        (functions / "chain.func").symlink_to("app.func", target_is_directory=True)
        alias = functions / "page.func"
        for target in (
            str(outside),
            "missing.func",
            "chain.func",
            "file.func",
            "page.func",
        ):
            with self.subTest(target=target):
                alias.symlink_to(target, target_is_directory=True)
                with self.assertRaises(BundleError):
                    seal_bundle(source, self.archive, preserve_function_aliases=True)
                self.assertFalse(self.archive.exists())
                alias.unlink()

    def test_producer_rejects_target_through_symlink_parent(self):
        source, function = self.source_function("real/app.func")
        functions = source / ".vercel/output/functions"
        (functions / "redirect").symlink_to("real", target_is_directory=True)
        (functions / "page.func").symlink_to(
            "redirect/app.func", target_is_directory=True
        )
        with self.assertRaises(BundleError):
            seal_bundle(source, self.archive, preserve_function_aliases=True)
        self.assertFalse(self.archive.exists())

    def test_producer_rejects_reserved_manifest_even_without_opt_in(self):
        source, _ = self.source_function()
        (source / FUNCTION_ALIASES_MANIFEST).write_bytes(b"{}")
        with self.assertRaises(BundleError):
            seal_bundle(source, self.archive)
        self.assertFalse(self.archive.exists())


if __name__ == "__main__":
    unittest.main()
