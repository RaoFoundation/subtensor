"""Run countersign validation offline; fail if live verification or btcli starts."""

import builtins
import importlib.util
import json
import sys
from pathlib import Path

module_path = Path(sys.argv[1]).resolve()
spec = importlib.util.spec_from_file_location("checked_in_countersign", module_path)
countersign = importlib.util.module_from_spec(spec)
spec.loader.exec_module(countersign)


def unreachable(*_args):
    raise RuntimeError("offline test reached live verification or btcli")


builtins.input = lambda _: "y"
countersign.asyncio.run = unreachable
countersign.btcli_command = unreachable
try:
    countersign.main(sys.argv[2:])
except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
    raise SystemExit(f"Refusing approval: {error}") from error
