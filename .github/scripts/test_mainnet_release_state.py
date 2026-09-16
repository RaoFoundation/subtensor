#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("mainnet-release-state.py")
spec = importlib.util.spec_from_file_location("mainnet_release_state", SCRIPT)
assert spec and spec.loader
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class DetectorTests(unittest.TestCase):
    def run_cli(self, value, *, error=None):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "state.json"
            with patch.object(module, "compute", side_effect=error or (lambda mode: value)):
                code = module.main(["--mode", "metadata", "--output", str(output)])
            return code, output.read_text() if output.exists() else None

    def test_historical_state_contains_only_eligible(self):
        code, raw = self.run_cli({"eligible": False})
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(raw), {"eligible": False})

    def test_success_writes_complete_identity(self):
        value = {
            "eligible": True,
            "spec_version": 433,
            "release_tag": "v433",
            "sha": "a" * 40,
            "code_hash": "0x" + "b" * 64,
            "finalized_head": "0x" + "c" * 64,
            "release_needed": True,
            "artifact_id": 42,
        }
        code, raw = self.run_cli(value)
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(raw), value)

    def test_failure_does_not_leave_usable_output(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "state.json"
            output.write_text("old\n")
            with patch.object(module, "compute", side_effect=module.StateError("bad RPC")):
                code = module.main(["--mode", "published", "--output", str(output)])
            self.assertEqual(code, 1)
            self.assertFalse(output.exists())

    def test_rpc_rejects_malformed_identity(self):
        responses = iter([
            "0x" + "a" * 64,
            {"specVersion": "433"},
        ])
        with patch.object(module, "rpc", side_effect=lambda *args: next(responses)):
            with self.assertRaises(module.StateError):
                module.finalized_identity("https://fake")


if __name__ == "__main__":
    unittest.main()
