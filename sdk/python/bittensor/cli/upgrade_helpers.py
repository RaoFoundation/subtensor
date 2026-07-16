"""Runtime-upgrade proposal helpers: discovery, verification, signing support.

A mainnet runtime upgrade is proposed by CI as the first half of a 2-of-2
deployment multisig (CI key + sudo multisig) holding a ``SudoUncheckedSetCode``
proxy on the chain's sudo key (see ``.github/scripts/deploy`` in the subtensor
repo). Everything about a pending proposal is discoverable from chain state:

    sudo.key() -> proxy.proxies(sudo_key)      # the deployment multisig
               -> Multisig.Multisigs(deploy)   # the pending proposal (hash only)
               -> depositor == CI key          # multi([CI, sudo], 2) == delegate

The call *data* is not on-chain (CI approves by hash); it ships as the
``proxy_proxy_blob.hex`` asset of the proposal pre-release that the release
train publishes, alongside ``upgrade-manifest.json`` (spec version, commit,
signer set, asset URLs). These helpers fetch that bundle, verify the call data
is exactly ``proxy.proxy(sudo_key, None, sudoUncheckedWeight(setCode(wasm),
W))`` and matches the on-chain hash, and build the finalizing call the sudo
multisig signs.
"""

from __future__ import annotations

import json
import os
import re
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from hashlib import blake2b, sha256
from typing import Any, Optional

from .._generated import calls
from .._generated import storage as st
from ..sp_core import ss58_decode, ss58_encode
from . import multisig_helpers as ms_helpers

DEFAULT_UPGRADE_REPO = "RaoFoundation/subtensor"

SUDO_PROXY_TYPE = "SudoUncheckedSetCode"

# Weight literals of the CI deployment pipeline (.github/scripts/deploy).
# PROPOSAL_WEIGHT is an argument of the proposed call itself
# (sudoUncheckedWeight) — changing it changes the call hash CI registered.
# FINALIZE_WEIGHT is an argument of the finalizing as_multi call, whose hash
# the sudo multisig operates on, so every triumvirate approval must repeat it
# exactly. Both must stay in lockstep with propose-upgrade-multisig.js /
# approve-upgrade-multisig.js.
PROPOSAL_WEIGHT = {"ref_time": 50_000_000_000, "proof_size": 0}
# Must cover the proxy+setCode proposal's declared weight (v432 measured
# ~50.5B ref_time / ~13.4k proof_size). Too-low proof_size fails the
# deployment as_multi with MaxWeightTooLow *after* the outer sudo approval.
FINALIZE_WEIGHT = {"ref_time": 80_000_000_000, "proof_size": 50_000}

_MAX_FETCH_BYTES = 64 * 1024 * 1024
_FETCH_TIMEOUT = 60.0

MANIFEST_ASSET = "upgrade-manifest.json"
CALL_DATA_ASSET = "proxy_proxy_blob.hex"
WASM_ASSET = "subtensor.wasm"
DIGEST_ASSET = "subtensor-digest.json"


# --- releases and assets ---------------------------------------------------------------


@dataclass(frozen=True)
class ReleaseRef:
    """A GitHub release, addressed as ``owner/repo`` + tag."""

    repo: str
    tag: str

    @property
    def url(self) -> str:
        return f"https://github.com/{self.repo}/releases/tag/{self.tag}"

    def asset_url(self, name: str) -> str:
        return f"https://github.com/{self.repo}/releases/download/{self.tag}/{name}"


def parse_release_url(url: str) -> ReleaseRef:
    """Parse a GitHub release page or asset URL into a :class:`ReleaseRef`.

    Accepts ``https://github.com/O/R/releases/tag/TAG``,
    ``https://github.com/O/R/releases/download/TAG/asset``, and the short
    ``O/R@TAG`` form.
    """
    short = re.fullmatch(r"([\w.-]+/[\w.-]+)@([\w.-]+)", url.strip())
    if short:
        return ReleaseRef(repo=short.group(1), tag=short.group(2))
    match = re.match(
        r"https?://github\.com/([\w.-]+/[\w.-]+)/releases/(?:tag|download)/([^/?#]+)",
        url.strip(),
    )
    if not match:
        raise ValueError(
            f"cannot parse {url!r} as a GitHub release URL "
            "(expected https://github.com/OWNER/REPO/releases/tag/TAG)"
        )
    return ReleaseRef(repo=match.group(1), tag=match.group(2))


def _github_headers(api: bool = False) -> dict[str, str]:
    headers = {"User-Agent": "btcli-upgrade"}
    if api:
        headers["Accept"] = "application/vnd.github+json"
        token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
        if token:
            headers["Authorization"] = f"Bearer {token}"
    return headers


def fetch_bytes(url: str, *, api: bool = False) -> bytes:
    """GET a URL (following redirects), capped at ``_MAX_FETCH_BYTES``."""
    request = urllib.request.Request(url, headers=_github_headers(api=api))
    try:
        with urllib.request.urlopen(request, timeout=_FETCH_TIMEOUT) as response:
            data = response.read(_MAX_FETCH_BYTES + 1)
    except urllib.error.HTTPError as error:
        raise ValueError(f"fetching {url} failed: HTTP {error.code}") from error
    except (urllib.error.URLError, OSError, TimeoutError) as error:
        raise ValueError(f"fetching {url} failed: {error}") from error
    if len(data) > _MAX_FETCH_BYTES:
        raise ValueError(f"{url} exceeds the {_MAX_FETCH_BYTES // (1 << 20)} MiB fetch cap")
    return data


def fetch_json(url: str, *, api: bool = False) -> Any:
    return json.loads(fetch_bytes(url, api=api).decode("utf-8"))


def parse_hex_blob(raw: bytes | str) -> bytes:
    """Decode a call-data hex payload (``proxy_proxy_blob.hex`` contents)."""
    text = raw.decode("ascii") if isinstance(raw, bytes) else raw
    text = text.strip().removeprefix("0x")
    if not text or not re.fullmatch(r"[0-9a-fA-F]+", text) or len(text) % 2:
        raise ValueError("call data is not a valid hex string")
    return bytes.fromhex(text)


def call_hash_hex(blob: bytes) -> str:
    """0x-hex blake2_256 of raw call bytes — the multisig pallet's call hash."""
    return "0x" + blake2b(blob, digest_size=32).hexdigest()


@dataclass
class ProposalBundle:
    """The verification inputs for one proposal, however they were sourced."""

    blob: bytes
    release: Optional[ReleaseRef] = None
    manifest: Optional[dict] = None
    wasm: Optional[bytes] = None
    digest: Optional[dict] = None
    # "local" when --wasm supplied the runtime (the trust anchor), "release"
    # when it was downloaded alongside the call data it is checked against.
    wasm_source: Optional[str] = None
    notes: list[str] = field(default_factory=list)

    @property
    def call_hash(self) -> str:
        return call_hash_hex(self.blob)


def fetch_proposal_bundle(
    *,
    release_url: Optional[str] = None,
    hex_url: Optional[str] = None,
    hex_file: Optional[str] = None,
    wasm_path: Optional[str] = None,
) -> ProposalBundle:
    """Assemble the call data + runtime + manifest from a release URL or raw parts."""
    sources = [s for s in (release_url, hex_url, hex_file) if s]
    if len(sources) != 1:
        raise ValueError("pass exactly one of --url, --hex-url, or --hex-file")

    release: Optional[ReleaseRef] = None
    manifest: Optional[dict] = None
    digest: Optional[dict] = None
    notes: list[str] = []

    if release_url:
        release = parse_release_url(release_url)
        blob = parse_hex_blob(fetch_bytes(release.asset_url(CALL_DATA_ASSET)))
        try:
            manifest = fetch_json(release.asset_url(MANIFEST_ASSET))
        except ValueError:
            notes.append("release has no upgrade-manifest.json asset (older release train)")
        try:
            digest = fetch_json(release.asset_url(DIGEST_ASSET))
        except ValueError:
            notes.append("release has no subtensor-digest.json asset")
    elif hex_url:
        blob = parse_hex_blob(fetch_bytes(hex_url))
    else:
        assert hex_file is not None
        with open(hex_file, "rb") as handle:
            blob = parse_hex_blob(handle.read())

    wasm: Optional[bytes] = None
    wasm_source: Optional[str] = None
    if wasm_path:
        with open(wasm_path, "rb") as handle:
            wasm = handle.read()
        wasm_source = "local"
    elif release:
        try:
            wasm = fetch_bytes(release.asset_url(WASM_ASSET))
            wasm_source = "release"
        except ValueError:
            notes.append("release has no subtensor.wasm asset")

    return ProposalBundle(
        blob=blob,
        release=release,
        manifest=manifest,
        wasm=wasm,
        digest=digest,
        wasm_source=wasm_source,
        notes=notes,
    )


def fetch_release_manifests(repo: str, *, limit: int = 15) -> list[dict]:
    """Upgrade manifests attached to the repo's recent releases.

    Each manifest gains ``release_url`` and ``prerelease``. Network failures
    yield an empty list — manifest enrichment is always advisory.
    """
    try:
        releases = fetch_json(
            f"https://api.github.com/repos/{repo}/releases?per_page={limit}", api=True
        )
    except ValueError:
        return []
    manifests: list[dict] = []
    for release in releases if isinstance(releases, list) else []:
        assets = {a.get("name"): a for a in release.get("assets") or []}
        if MANIFEST_ASSET not in assets:
            continue
        try:
            manifest = fetch_json(assets[MANIFEST_ASSET]["browser_download_url"])
        except ValueError:
            continue
        manifest["release_url"] = release.get("html_url")
        manifest["prerelease"] = bool(release.get("prerelease"))
        manifests.append(manifest)
    return manifests


def find_manifest_for_call_hash(manifests: list[dict], call_hash: str) -> Optional[dict]:
    """The manifest whose ``call_hash`` matches, if any."""
    wanted = call_hash.lower()
    for manifest in manifests:
        if str(manifest.get("call_hash", "")).lower() == wanted:
            return manifest
    return None


# --- structural verification (offline port of test-wasm.py) ----------------------------

# The blob is the bare SCALE encoding of
#   proxy.proxy(real=sudo_key, force_proxy_type=None,
#               sudo.sudoUncheckedWeight(system.setCode(wasm), PROPOSAL_WEIGHT))
# as built by propose-upgrade-multisig.js. Pallet/call indices come from the
# runtime (System=0, Sudo=12, Proxy=16); if they change, this parse fails
# loudly and must be updated in lockstep with test-wasm.py. The parse is only
# used to *extract* the embedded runtime and proxied account — byte-exactness
# is then proven by re-encoding against live chain metadata (see
# ``reconstruct_proxy_call``), which does not depend on these constants.
_PROXY_PROXY = bytes([16, 0])
_MULTIADDRESS_ID = bytes([0])
_OPTION_NONE = bytes([0])
_SUDO_SUDO_UNCHECKED_WEIGHT = bytes([12, 1])
_SYSTEM_SET_CODE = bytes([0, 2])


def _compact_encode(n: int) -> bytes:
    """SCALE compact encoding of a non-negative integer."""
    if n < 1 << 6:
        return bytes([n << 2])
    if n < 1 << 14:
        return ((n << 2) | 0b01).to_bytes(2, "little")
    if n < 1 << 30:
        return ((n << 2) | 0b10).to_bytes(4, "little")
    data = n.to_bytes((n.bit_length() + 7) // 8, "little")
    return bytes([((len(data) - 4) << 2) | 0b11]) + data


def _compact_decode(data: bytes) -> tuple[int, int]:
    """Decode a SCALE compact integer; returns (value, bytes consumed)."""
    if not data:
        raise ValueError("empty compact integer")
    mode = data[0] & 0b11
    if mode == 0b00:
        return data[0] >> 2, 1
    if mode == 0b01:
        return int.from_bytes(data[:2], "little") >> 2, 2
    if mode == 0b10:
        return int.from_bytes(data[:4], "little") >> 2, 4
    length = (data[0] >> 2) + 4
    return int.from_bytes(data[1 : 1 + length], "little"), 1 + length


@dataclass
class ParsedBlob:
    """The two variable parts of a well-formed proposal blob."""

    real_ss58: str
    code: bytes


def parse_proposal_blob(blob: bytes, *, ss58_format: int = 42) -> ParsedBlob:
    """Parse a proposal blob, requiring the exact CI call shape around the wasm.

    Raises ``ValueError`` when the blob is anything other than
    ``proxy.proxy(MultiAddress::Id(real), None,
    sudoUncheckedWeight(setCode(code), PROPOSAL_WEIGHT))`` — a batch or any
    extra call wrapped around honest wasm bytes fails here.
    """
    head = _PROXY_PROXY + _MULTIADDRESS_ID
    if blob[: len(head)] != head:
        raise ValueError("call data is not proxy.proxy(MultiAddress::Id(...), ...)")
    real = blob[len(head) : len(head) + 32]
    if len(real) != 32:
        raise ValueError("call data is truncated inside the proxied account")
    rest = blob[len(head) + 32 :]
    inner_head = _OPTION_NONE + _SUDO_SUDO_UNCHECKED_WEIGHT + _SYSTEM_SET_CODE
    if rest[: len(inner_head)] != inner_head:
        raise ValueError(
            "call data after the proxied account is not sudoUncheckedWeight(setCode(...), ...)"
        )
    rest = rest[len(inner_head) :]
    code_len, consumed = _compact_decode(rest)
    code = rest[consumed : consumed + code_len]
    if len(code) != code_len:
        raise ValueError("call data is truncated inside the runtime bytes")
    weight = _compact_encode(PROPOSAL_WEIGHT["ref_time"]) + _compact_encode(
        PROPOSAL_WEIGHT["proof_size"]
    )
    tail = rest[consumed + code_len :]
    if tail != weight:
        raise ValueError(
            "call data does not end with exactly the pinned sudoUncheckedWeight "
            f"weight {PROPOSAL_WEIGHT} (or carries trailing bytes)"
        )
    return ParsedBlob(real_ss58=ss58_encode(real, ss58_format), code=code)


async def reconstruct_proxy_call(client, *, wasm: bytes, sudo_key: str):
    """Compose the proposal call from first principles against live metadata.

    Returns the composed call (``.data`` bytes, ``.call_hash``). Byte-equality
    of ``.data`` with a proposal blob proves the blob dispatches exactly
    ``setCode(wasm)`` on ``sudo_key`` and nothing else.
    """
    set_code = await client.compose(calls.System.set_code(code="0x" + wasm.hex()))
    unchecked = await client.compose(
        calls.Sudo.sudo_unchecked_weight(call=set_code, weight=PROPOSAL_WEIGHT)
    )
    return await client.compose(
        calls.Proxy.proxy(real=sudo_key, force_proxy_type=None, call=unchecked)
    )


# --- on-chain discovery -----------------------------------------------------------------


async def discover_pending_upgrades(client) -> list[dict[str, Any]]:
    """Pending runtime-upgrade proposals, from chain state alone.

    Walks sudo.key() -> its ``SudoUncheckedSetCode`` proxies -> pending
    multisig ops on each delegate, keeping ops whose depositor + sudo key
    re-derive the delegate as a 2-of-2 multisig (the CI deployment pattern).
    """
    sudo_key = str(await client.query(st.Sudo.Key))
    value = await client.query(st.Proxy.Proxies, [sudo_key])
    delegations = (value or ([], 0))[0]
    upgrades: list[dict[str, Any]] = []
    for delegation in delegations:
        if str(delegation.get("proxy_type")) != SUDO_PROXY_TYPE:
            continue
        if int(delegation.get("delay") or 0) != 0:
            continue
        delegate = str(delegation.get("delegate"))
        for op in await ms_helpers.list_pending_multisig_ops(client, delegate):
            depositor = str(op.get("depositor"))
            try:
                derived = await client.multisig([depositor, sudo_key], 2)
            except Exception:
                continue
            if derived.address != delegate:
                continue  # not the CI + sudo deployment pattern
            upgrades.append(
                {
                    "kind": "runtime-upgrade-proposal",
                    "call_hash": op["call_hash"],
                    "timepoint": op["timepoint"],
                    "timepoint_display": op["timepoint_display"],
                    "sudo_key": sudo_key,
                    "deployment_multisig": delegate,
                    "ci_address": depositor,
                    "deployment_approvals": list(op.get("approvals") or []),
                }
            )
    return upgrades


async def find_pending_upgrade(client, call_hash: str) -> Optional[dict[str, Any]]:
    """The discovered pending upgrade matching ``call_hash``, if any."""
    wanted = call_hash.lower()
    for upgrade in await discover_pending_upgrades(client):
        if str(upgrade["call_hash"]).lower() == wanted:
            return upgrade
    return None


# --- the finalizing call and the sudo-multisig layer -------------------------------------


async def compose_finalizing_call(
    client,
    *,
    blob: bytes,
    ci_address: str,
    deploy_timepoint: dict[str, int],
):
    """The deployment multisig's finalizing call — what the sudo multisig signs.

    ``Multisig.as_multi(2, [ci], deploy_timepoint, <proposal blob>,
    FINALIZE_WEIGHT)``: submitted by the sudo multisig account, it lands the
    second (executing) approval of CI's proposal. Its encoding must be
    byte-identical across all triumvirate signers, which is why the weight is
    a pinned constant and the call bytes are spliced straight from the shared
    blob (the call encoder accepts pre-composed calls as raw SCALE bytes).
    """
    return await client.compose(
        calls.Multisig.as_multi(
            threshold=2,
            other_signatories=[ci_address],
            maybe_timepoint={
                "height": int(deploy_timepoint["height"]),
                "index": int(deploy_timepoint["index"]),
            },
            call=bytes(blob),
            max_weight=FINALIZE_WEIGHT,
        )
    )


async def finalizing_max_weight(client, finalizing, signer_address: str) -> dict:
    """max_weight for the sudo-layer approval that dispatches ``finalizing``.

    ``pallet_multisig`` rejects the executing ``as_multi`` with
    ``MaxWeightTooLow`` unless its max_weight covers the wrapped call's
    declared dispatch weight — which for the finalizing call is the pinned
    ``FINALIZE_WEIGHT`` it embeds *plus* the multisig pallet's own overhead.
    Estimate the declared weight from the live runtime and pad it 10% so the
    approval never lands just under the line.
    """

    weight = await client.estimate_weight(finalizing, address=signer_address)
    return {
        "ref_time": int(weight["ref_time"] * 11 // 10),
        "proof_size": int(weight["proof_size"] * 11 // 10),
    }


def sorted_other_signatories(signatories: list[str], self_address: str) -> list[str]:
    """Everyone but ``self_address``, sorted by raw account id (chain order)."""
    others = [s for s in signatories if s != self_address]
    return sorted(set(others), key=lambda s: bytes(ss58_decode(s)))


async def sudo_layer_status(
    client,
    *,
    sudo_key: str,
    finalizing_call_hash: str,
) -> Optional[dict[str, Any]]:
    """The sudo multisig's pending op for the finalizing call, if opened."""
    pending = await client.query(st.Multisig.Multisigs, [sudo_key, finalizing_call_hash])
    if not pending:
        return None
    when = pending.get("when") or {}
    return {
        "call_hash": finalizing_call_hash,
        "timepoint": {"height": int(when.get("height", 0)), "index": int(when.get("index", 0))},
        "approvals": [str(a) for a in pending.get("approvals") or []],
        "depositor": str(pending.get("depositor")),
    }


def sign_command(network: str, release_url: str, wallet: str = "<your-wallet>") -> str:
    """The copy-paste command for a co-signer."""
    prefix = "btcli" if network == "finney" else f"btcli -n {network}"
    return f"{prefix} upgrade sign --url {release_url} -w {wallet}"


def check_command(network: str, release_url: str) -> str:
    prefix = "btcli" if network == "finney" else f"btcli -n {network}"
    return f"{prefix} upgrade check --url {release_url} --wasm <path/to/your-build.wasm>"


def srtool_recipe(manifest: Optional[dict]) -> list[str]:
    """The reproduce-from-source recipe, tailored by the manifest when present."""
    tag = (manifest or {}).get("tag") or "<tag>"
    repo = (manifest or {}).get("repo") or DEFAULT_UPGRADE_REPO
    rustc = (manifest or {}).get("srtool_rustc")
    image = f"paritytech/srtool:{rustc}" if rustc else "paritytech/srtool:<rustc-tag>"
    return [
        f"git clone https://github.com/{repo} && cd {repo.split('/', 1)[1]}",
        f"git checkout {tag}",
        "ln -s . runtime/node-subtensor",
        "docker run --rm --user root --platform=linux/amd64 \\",
        "  -e PACKAGE=node-subtensor-runtime \\",
        '  -e BUILD_OPTS="--features=metadata-hash" \\',
        "  -e PROFILE=production \\",
        '  -v "$(pwd)":/build \\',
        f"  {image} /srtool/build --app",
    ]


# --- verification runner ------------------------------------------------------------------


@dataclass
class Check:
    name: str
    ok: Optional[bool]  # None = skipped / not applicable
    detail: str


@dataclass
class CheckOutcome:
    checks: list[Check]
    data: dict[str, Any]

    @property
    def ok(self) -> bool:
        return all(c.ok is not False for c in self.checks)

    def failed(self) -> list[Check]:
        return [c for c in self.checks if c.ok is False]


async def run_proposal_checks(client, bundle: ProposalBundle) -> CheckOutcome:
    """Run every offline + on-chain check for a proposal bundle.

    The checks, in order:

    1. structure — the blob parses as exactly the CI proposal call shape;
    2. reconstruction — re-encoding ``proxy.proxy(real, None,
       sudoUncheckedWeight(setCode(code), W))`` against live chain metadata
       reproduces the blob byte-for-byte;
    3. runtime match — the embedded runtime equals the provided wasm
       (local srtool build or release asset);
    4. digest — sha256 of the embedded runtime matches subtensor-digest.json;
    5. sudo key — the proxied account is the chain's live sudo.key();
    6. on-chain proposal — a pending deployment-multisig op exists whose call
       hash is blake2_256(blob), depositor rederives the delegate;
    7. manifest cross-checks, when a manifest is present.
    """
    checks: list[Check] = []
    data: dict[str, Any] = {"call_hash": bundle.call_hash}

    sudo_key = str(await client.query(st.Sudo.Key))
    data["sudo_key"] = sudo_key

    # 1. structure
    parsed: Optional[ParsedBlob] = None
    try:
        parsed = parse_proposal_blob(bundle.blob)
        checks.append(
            Check(
                "call structure",
                True,
                "exactly proxy.proxy(real, None, sudoUncheckedWeight(setCode(code), "
                f"{{ref_time: {PROPOSAL_WEIGHT['ref_time']}, proof_size: "
                f"{PROPOSAL_WEIGHT['proof_size']}}})); runtime is "
                f"{len(parsed.code)} bytes",
            )
        )
    except ValueError as error:
        checks.append(Check("call structure", False, str(error)))

    if parsed is not None:
        data["proxied_account"] = parsed.real_ss58
        data["code_sha256"] = "0x" + sha256(parsed.code).hexdigest()

        # 2. reconstruction against live metadata
        reconstructed = await reconstruct_proxy_call(
            client, wasm=parsed.code, sudo_key=parsed.real_ss58
        )
        if bytes(reconstructed.data) == bundle.blob:
            checks.append(
                Check(
                    "re-encoding",
                    True,
                    "composing the same call against live chain metadata reproduces "
                    "the call data byte-for-byte",
                )
            )
        else:
            checks.append(
                Check(
                    "re-encoding",
                    False,
                    "re-encoded call differs from the call data "
                    f"({len(bytes(reconstructed.data))} vs {len(bundle.blob)} bytes) — "
                    "runtime metadata may disagree with the offline parse",
                )
            )

        # 3. runtime match (the trust anchor when --wasm is a local srtool build)
        if bundle.wasm is not None:
            if parsed.code == bundle.wasm:
                source = (
                    "your local build"
                    if bundle.wasm_source == "local"
                    else "the release's subtensor.wasm"
                )
                checks.append(
                    Check("runtime match", True, f"embedded runtime is byte-identical to {source}")
                )
            else:
                checks.append(
                    Check(
                        "runtime match",
                        False,
                        "embedded runtime differs from the provided wasm "
                        f"(sha256 {sha256(bundle.wasm).hexdigest()} vs "
                        f"{sha256(parsed.code).hexdigest()})",
                    )
                )
        else:
            checks.append(
                Check(
                    "runtime match",
                    None,
                    "no wasm provided — build from source with srtool and pass --wasm "
                    "to pin the call data to code you compiled yourself",
                )
            )

        # 4. digest
        if bundle.digest is not None:
            expected = str(bundle.digest.get("sha256", "")).removeprefix("0x").lower()
            actual = sha256(parsed.code).hexdigest()
            checks.append(
                Check(
                    "srtool digest",
                    actual == expected,
                    f"sha256 {'matches' if actual == expected else 'differs from'} "
                    "subtensor-digest.json",
                )
            )

        # 5. sudo key
        checks.append(
            Check(
                "sudo key",
                parsed.real_ss58 == sudo_key,
                f"proxied account {parsed.real_ss58} "
                f"{'is' if parsed.real_ss58 == sudo_key else 'IS NOT'} "
                f"the chain's sudo key {sudo_key}",
            )
        )

    # 6. on-chain proposal
    pending = await find_pending_upgrade(client, bundle.call_hash)
    if pending is not None:
        data["pending"] = pending
        checks.append(
            Check(
                "on-chain proposal",
                True,
                f"pending deployment-multisig op matches blake2_256(call data) "
                f"{bundle.call_hash} (proposed at {pending['timepoint_display']} "
                f"by {pending['ci_address']})",
            )
        )
    else:
        checks.append(
            Check(
                "on-chain proposal",
                False,
                f"no pending deployment-multisig op has call hash {bundle.call_hash} "
                "(already executed, cancelled, or not proposed on this network)",
            )
        )

    # 7. manifest cross-checks
    manifest = bundle.manifest
    if manifest:
        manifest_hash = str(manifest.get("call_hash", "")).lower()
        checks.append(
            Check(
                "manifest call hash",
                manifest_hash == bundle.call_hash.lower(),
                "upgrade-manifest.json call_hash "
                f"{'matches' if manifest_hash == bundle.call_hash.lower() else 'differs from'} "
                "the call data",
            )
        )
        if pending is not None and manifest.get("ci_address"):
            ci_matches = str(manifest["ci_address"]) == pending["ci_address"]
            checks.append(
                Check(
                    "manifest CI key",
                    ci_matches,
                    "manifest ci_address "
                    f"{'matches' if ci_matches else 'differs from'} the on-chain depositor",
                )
            )
        data["manifest"] = {
            key: manifest.get(key)
            for key in ("spec_version", "commit", "tag", "repo", "wasm_sha256")
        }

    return CheckOutcome(checks=checks, data=data)
