#!/usr/bin/env python3
"""Fail closed if a feature-dependent RustSec exclusion becomes reachable."""

import re
import subprocess


HICKORY_FEATURES = {
    ("hickory-proto", (0, 24, 4)): {"mdns", "socket2", "tokio", "tokio-runtime"},
    ("hickory-resolver", (0, 24, 4)): {
        "ipconfig",
        "resolv-conf",
        "system-config",
        "tokio",
        "tokio-runtime",
    },
    ("hickory-proto", (0, 25, 2)): {"futures-io", "std", "tokio"},
    ("hickory-resolver", (0, 25, 2)): {"default", "system-config", "tokio"},
}


def violations(tree: str) -> list[str]:
    failures = set()
    for line in tree.splitlines():
        package = re.match(r"^(\S+) v(\d+)\.(\d+)\.(\d+)\b(.*)$", line)
        if package is None:
            continue
        name, major, minor, patch, features = package.groups()
        version = (int(major), int(minor), int(patch))
        if name == "ruzstd" and (
            version < (0, 9, 1) or (version == (0, 9, 1) and features.startswith("-"))
        ):
            failures.add(
                "RUSTSEC-2026-0344: affected ruzstd is in the active build graph"
            )
        if name == "wasmtime" and "component-model" in re.split(r"[,\s]+", features):
            failures.add(
                "Wasmtime component-model is enabled; reassess RustSec exclusions"
            )
        if name == "wasmtime" and any(
            feature.startswith(("wasi", "component-model", "runtime"))
            for feature in re.split(r"[,\s]+", features)
        ):
            failures.add(
                "RUSTSEC-2026-0566/0569/0571: Wasmtime WASI or host-runtime "
                "features changed; reassess the reviewed exclusions"
            )
        if name in {"wasmtime-wasi", "wasmtime-wasi-http", "wasi-common"}:
            failures.add(
                "RUSTSEC-2026-0566/0569/0571: WASI filesystem or HTTP support "
                "entered the build graph; reassess the reviewed exclusions"
            )
        if name in {"libp2p-quic", "quinn", "quinn-proto", "quinn-udp"}:
            failures.add(
                "RUSTSEC-2026-0523/0524/0525/0538: QUIC entered the build graph; "
                "reassess the reviewed exclusions"
            )
        if name == "libp2p":
            active_features = set(re.split(r"[,\s]+", features.split(" (*")[0]))
            if not {"dns", "noise"} <= active_features or "quic" in active_features:
                failures.add(
                    "RUSTSEC-2026-0487..0551: libp2p DNS/Noise/QUIC features "
                    "changed; reassess the reviewed DNS and QUIC exclusions"
                )
        if name in {"hickory-proto", "hickory-resolver"}:
            active_features = set(
                filter(None, re.split(r"[,\s]+", features.split(" (*")[0]))
            )
            reviewed_features = HICKORY_FEATURES.get((name, version))
            if (
                reviewed_features is None
                or active_features != reviewed_features
                or any(feature.startswith("dnssec") for feature in active_features)
            ):
                failures.add(
                    "RUSTSEC-2026-0118/0119/0487..0551: Hickory version or "
                    "reviewed features (including DNSSEC backends) changed; "
                    "reassess the DNSSEC exclusions and residual DNS codec risk"
                )
    return sorted(failures)


def main() -> None:
    tree = subprocess.run(
        [
            "cargo",
            "tree",
            "--locked",
            "--workspace",
            "--all-features",
            "--target",
            "all",
            "--prefix",
            "none",
            "--format",
            "{p} {f}",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    if not tree.strip():
        raise SystemExit("Cargo returned an empty dependency graph")
    failures = violations(tree)
    if failures:
        raise SystemExit("\n".join(failures))
    print(
        "RustSec feature exclusions verified against all workspace features and targets"
    )


if __name__ == "__main__":
    main()
