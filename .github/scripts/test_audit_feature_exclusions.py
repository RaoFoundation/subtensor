"""Regression tests for the enforced unreachable-code advisory exclusions."""

import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location(
    "audit_exclusions", Path(__file__).with_name("check-audit-feature-exclusions.py")
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class AuditExclusions(unittest.TestCase):
    def test_core_wasm_and_fixed_decoder_are_allowed(self):
        self.assertEqual(
            module.violations(
                "wasmtime v8.0.1 cache,cranelift,jitdump\nruzstd v0.9.1 std\n"
            ),
            [],
        )

    def test_each_affected_decoder_fails(self):
        for version in ("0.4.0", "0.6.0", "0.9.0", "0.9.1-alpha.1"):
            with self.subTest(version=version):
                self.assertTrue(module.violations(f"ruzstd v{version} std"))

    def test_component_model_fails_in_any_feature_position(self):
        for features in (
            "component-model",
            "cache,component-model",
            "component-model,cache",
            "(/vendor/wasmtime) component-model (*)",
        ):
            with self.subTest(features=features):
                self.assertTrue(module.violations(f"wasmtime v8.0.1 {features}"))

    def test_quic_wasi_and_dnssec_exclusions_fail_if_reachable(self):
        for package in (
            "libp2p v0.54.1 dns,quic,tokio",
            "wasmtime-wasi v8.0.1 default",
            "wasmtime-wasi-http v8.0.1 default",
            "hickory-resolver v0.24.4 dnssec-ring,tokio",
            "hickory-resolver v0.25.2 dnssec-aws-lc-rs,tokio",
        ):
            with self.subTest(package=package):
                self.assertTrue(module.violations(package))


if __name__ == "__main__":
    unittest.main()
