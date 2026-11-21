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

    def test_reviewed_dns_noise_and_quic_graph_is_allowed(self):
        tree = "\n".join(
            (
                "libp2p v0.54.1 dns,identify,noise,tcp",
                "hickory-proto v0.24.4 mdns,tokio",
                "hickory-proto v0.25.2 futures-io,std,tokio (*)",
                "hickory-resolver v0.24.4 system-config,tokio",
                "hickory-resolver v0.25.2 default,system-config,tokio",
            )
        )
        self.assertEqual(module.violations(tree), [])

    def test_dns_noise_or_quic_graph_change_fails(self):
        for tree in (
            "libp2p v0.54.1 identify,noise,tcp",
            "libp2p v0.54.1 dns,identify,tcp",
            "libp2p v0.54.1 dns,noise,quic,tcp",
            "libp2p-quic v0.12.0",
            "quinn-proto v0.11.13",
            "quinn-udp v0.5.14",
            "hickory-proto v0.24.4 dnssec,tokio",
            "hickory-resolver v0.25.2 default,dnssec,tokio (*)",
            "hickory-proto v0.26.2 std,tokio",
            "hickory-resolver v0.24.3 system-config,tokio",
        ):
            with self.subTest(tree=tree):
                self.assertTrue(module.violations(tree))

    def test_wasi_filesystem_or_http_graph_change_fails(self):
        for tree in (
            "wasmtime v8.0.1 cache,wasi-common",
            "wasmtime v8.0.1 cache,wasi-http",
            "wasmtime v8.0.1 cache,component-model-async",
            "wasmtime v8.0.1 cache,runtime",
            "wasmtime-wasi v8.0.1",
            "wasmtime-wasi-http v8.0.1",
            "wasi-common v8.0.1",
        ):
            with self.subTest(tree=tree):
                self.assertTrue(module.violations(tree))


if __name__ == "__main__":
    unittest.main()
