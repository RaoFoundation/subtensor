"""Shared per-invocation state and the single place chain work is executed.

The top-level callback builds one ``AppContext`` and stashes it on the Typer
context. Every command pulls global options (network, wallet, output mode) from
it instead of redeclaring them, and runs all its chain work through
``AppContext.run`` so connection lifecycle and error handling live in one place.
"""

from __future__ import annotations

import asyncio
import contextlib
import sys
from dataclasses import dataclass, replace
from types import SimpleNamespace
from typing import Awaitable, Callable, Optional, TypeVar

import typer

from .. import config as cfg
from .. import wallets
from .._generated import calls as generated_calls
from .._generated import runtime_apis, storage
from ..client import Client
from ..extension.client import BridgeError
from ..ledger import LedgerError, LedgerSigner
from ..result import (
    REMEDIATION,
    BittensorError,
    ChainError,
    ErrorCode,
    ExtrinsicResult,
    PolicyError,
    RpcConnectionError,
    RpcPolicyError,
)
from ..settings import error_docs_url
from ..signing import public_view
from ..sp_core import ss58_decode
from ..vault import VaultSigner
from ..wallets import is_bittensor_address
from . import multisig_helpers as ms_helpers
from .output import STYLE_WARNING, Output

T = TypeVar("T")


class _FeeAddressView:
    """Public-only keypair shape for ``estimate_fee`` (zeroed signature)."""

    crypto_type = 1  # sr25519

    def __init__(self, address: str):
        self.ss58_address = address
        self.public_key = bytes(ss58_decode(address))


def address_cli_name(param: str) -> str:
    """CLI flag for a param resolved by ``resolve_address`` (drops the ``_ss58`` suffix)."""
    base = param[: -len("_ss58")] if param.endswith("_ss58") else param.replace("_ss58", "")
    return "--" + base.replace("_", "-")


def ss58_param_help(param: str) -> str:
    """Help text for an address-typed CLI option (see AppContext.resolve_address)."""
    book = "address-book or proxy-book name, "
    if "hotkey" in param:
        text = f"ss58 address, {book}or a local hotkey name (HOTKEY or WALLET/HOTKEY)."
        if param == "hotkey_ss58":
            text += " Defaults to your wallet's hotkey."
    else:
        text = f"ss58 address, {book}or a local wallet name (uses its coldkey)."
        if param == "coldkey_ss58":
            text += " Defaults to your wallet's coldkey."
    return text


@dataclass
class AppContext:
    network: str
    wallet_name: str
    hotkey_name: str
    wallet_path: str
    assume_yes: bool
    dry_run: bool
    output: Output
    # Tri-state MEV shielding override (--mev-shield/--no-mev-shield). None
    # falls through to the persistent `mev_shield` config value, then to the
    # intent's own default (stake-trading intents shield, the rest don't).
    mev_shield: Optional[bool] = None
    # Whether --wallet/-w (or BT_WALLET) was passed explicitly, as opposed to
    # wallet_name being the config/built-in default. Wallet-scoped commands use
    # this to decide whether the target wallet still needs confirming.
    wallet_given: bool = False
    # Same for --wallet-hotkey/-H (or BT_WALLET_HOTKEY): hotkey-scoped commands
    # confirm the hotkey name when it was only defaulted.
    hotkey_given: bool = False
    # Diagnostic log verbosity (-v count); kept so per-command --quiet/--verbose
    # overrides can reconfigure logging without losing the root-level setting.
    verbosity: int = 0
    wallet_password_file: Optional[str] = None
    macos_password: bool = False
    keychain_password: bool = False
    # Connection resilience: comma-separated endpoint pools ("none" pins the
    # client to its primary endpoint; unset uses the network's public defaults).
    fallback_endpoints: Optional[str] = None
    archive_endpoints: Optional[str] = None
    retry_forever: bool = False
    signer_backend: Optional[str] = None
    signer_address: Optional[str] = None
    extension_source: Optional[str] = None
    extension_bridge_url: Optional[str] = None
    extension_browser: Optional[str] = None
    # Ledger derivation path (m/44'/354'/account'/0'/index').
    ledger_account: int = 0
    ledger_index: int = 0
    _extension_selection: Optional[object] = None
    _extension_bridge_ws_url: Optional[str] = None
    _ledger_signer: Optional[object] = None
    _vault_signer: Optional[VaultSigner] = None

    def reset_extension_session(self) -> None:
        self._extension_selection = None
        self._extension_bridge_ws_url = None

    def wallet(self):
        """Open the configured wallet handle (no key unlock; that happens on signing)."""
        return wallets.open_wallet(self.wallet_name, self.hotkey_name, self.wallet_path)

    def uses_extension_signer(self) -> bool:
        return (self.signer_backend or "").strip().lower() == "extension"

    def uses_ledger_signer(self) -> bool:
        return (self.signer_backend or "").strip().lower() == "ledger"

    def uses_vault_signer(self) -> bool:
        return (self.signer_backend or "").strip().lower() == "vault"

    def uses_external_signer(self) -> bool:
        """The signing key lives outside the local wallet files (extension,
        Ledger, or Polkadot Vault), so wallet confirmation and wallet-derived
        addressing don't apply."""
        return self.uses_extension_signer() or self.uses_ledger_signer() or self.uses_vault_signer()

    def ledger_signer(self):
        """Connect to the Ledger (once per invocation) and return the signer.

        Fails with a clean error when no device is reachable or the Polkadot
        app isn't open.
        """
        if self._ledger_signer is None:
            signer = LedgerSigner(account=self.ledger_account, index=self.ledger_index)
            if not self.output.quiet and not self.output.json_mode:
                self.output.message(
                    f"using ledger account {signer.ss58_address} "
                    f"(m/44'/354'/{self.ledger_account}'/0'/{self.ledger_index}')"
                )
            self._ledger_signer = signer
        return self._ledger_signer

    def vault_signer(self) -> VaultSigner:
        """The Polkadot Vault (QR) signer for this invocation (built once).

        The signing address is ``--signer-address`` (raw ss58 or an
        address-book name), else the persisted ``signer_address`` config
        value, falling back to the configured wallet's coldkeypub — a
        pubkey-only wallet is the natural companion to a Vault-held key.
        Nothing opens until the transport actually asks for a signature.
        """
        if self._vault_signer is None:
            address = self.signer_address or cfg.get("signer_address")
            if address and not is_bittensor_address(address):
                address = cfg.get_address(address) or address
            if not address:
                try:
                    address = self.wallet().coldkeypub.ss58_address
                except Exception:
                    self.output.error(
                        "the vault signer needs an account address",
                        help="pass --signer-address <ss58>, or configure a wallet whose "
                        "coldkeypub matches the key held in Polkadot Vault",
                    )
                    raise typer.Exit(2)
            if not is_bittensor_address(address):
                self.output.error(f"--signer-address {address!r} is not a valid ss58 address")
                raise typer.Exit(2)
            signer = VaultSigner(
                address,
                browser=self._extension_browser_choice(),
                # Same heuristic as the extension bridge: only pop a browser
                # tab for a human at a terminal.
                open_browser=not self.output.quiet and sys.stderr.isatty(),
                on_status=self.output.message,
            )
            if not self.output.quiet and not self.output.json_mode:
                self.output.message(f"using Polkadot Vault account {address}")
            self._vault_signer = signer
        return self._vault_signer

    async def extension_signer(self, *, pick_account: bool = False):
        """Connect to the bridge and return an extension-backed signer."""
        import sys

        from .. import config
        from ..extension import ensure_bridge, open_extension_signer, select_extension_account

        if pick_account or self._extension_selection is None:
            if not self.output.quiet:
                self.output.message(
                    "starting fresh extension bridge — authorize your wallet in the "
                    "browser when it opens"
                )

            url = await ensure_bridge(
                bridge_url=self.extension_bridge_url,
                open_browser=not self.output.quiet and sys.stderr.isatty(),
                browser=self._extension_browser_choice(),
                fresh=True,
                on_waiting=lambda http_url, status: self.output.message(
                    f"waiting for extension authorization in browser… ({http_url})"
                ),
            )
            self._extension_bridge_ws_url = url

            def _remember(account) -> None:
                config.set_value("signer_address", account.address)

            pinned_address = self.signer_address
            saved_address = config.get("signer_address")
            self._extension_selection = await select_extension_account(
                url,
                address=pinned_address,
                source=self.extension_source,
                interactive=(
                    pinned_address is None and sys.stdin.isatty() and not self.output.json_mode
                ),
                default_address=saved_address if pinned_address is None else None,
                on_picked=_remember,
            )
            if not self.output.quiet and not self.output.json_mode:
                picked = self._extension_selection.account
                reused = pinned_address is None and picked.address == saved_address
                self.output.message(
                    f"using extension account {picked.name} ({picked.address}, {picked.source})"
                    + (
                        " [dim]— saved default; pass --signer-address to switch[/dim]"
                        if reused
                        else ""
                    )
                )
        elif self._extension_bridge_ws_url is None:
            self._extension_bridge_ws_url = await ensure_bridge(
                bridge_url=self.extension_bridge_url,
                open_browser=False,
                browser=self._extension_browser_choice(),
            )

        assert self._extension_bridge_ws_url is not None
        assert self._extension_selection is not None
        return await open_extension_signer(self._extension_bridge_ws_url, self._extension_selection)

    def _extension_browser_choice(self) -> Optional[str]:
        from .. import config

        return self.extension_browser or config.get("extension_browser")

    async def resolve_signing_wallet(self, role: str = "coldkey", *, pick_account: bool = False):
        """Return the configured signer for ``role`` (local wallet, extension,
        Ledger, or Polkadot Vault).

        With an external backend the selected account *is* the signing key for
        either role — for hotkey intents, pick the account holding the hotkey.
        """
        backend = (self.signer_backend or "wallet").strip().lower()
        if backend not in ("wallet", "extension", "ledger", "vault"):
            self.output.error(
                f"unknown signing backend {self.signer_backend!r}",
                help="--signer takes wallet, extension, ledger, or vault",
            )
            raise typer.Exit(2)
        if self.uses_extension_signer():
            return await self.extension_signer(pick_account=pick_account)
        if self.uses_ledger_signer():
            return self.ledger_signer()
        if self.uses_vault_signer():
            return self.vault_signer()
        return self.signer(role)

    def signer(self, role: str = "coldkey"):
        """Signing handle with configured password sources (macOS dialog, Keychain, file)."""
        from ..signing import WalletSigner

        wallet = self.wallet()
        if not (self.macos_password or self.keychain_password or self.wallet_password_file):
            return wallet
        return WalletSigner(
            wallet,
            role,
            password_file=self.wallet_password_file,
            macos_prompt=self.macos_password,
            keychain=self.keychain_password,
        )

    def resolve_address(self, param: str, value: Optional[str]) -> Optional[str]:
        """Resolve an address-typed CLI value (any ``*_ss58`` param) to an ss58 address.

        Five accepted forms:
        - a raw ss58 address: used as-is;
        - an address-book name (``btcli addresses NAME SS58``);
        - a proxy-book name (``btcli proxy book add``);
        - a local key reference: hotkey params take ``HOTKEY`` (in the configured
          wallet) or ``WALLET/HOTKEY``; coldkey params take a ``WALLET`` name
          (resolved to its coldkey);
        - omitted: only the canonical ``hotkey_ss58`` / ``coldkey_ss58`` params
          fall back to the configured wallet's own key. Destination-style params
          (``--dest``, ``--destination-hotkey``, ...) never default.
        """
        kind = "hotkey" if "hotkey" in param else "coldkey"
        if value is None and param in ("coldkey_ss58", "hotkey_ss58"):
            # Inline import: prompt.py imports AppContext from this module, so a
            # top-level import here would be circular.
            from .prompt import confirm_wallet

            confirm_wallet(
                self,
                help_text=f"Wallet whose {kind} this command targets.",
                require_coldkey=param == "coldkey_ss58",
                hotkey_help=("Hotkey this command targets." if param == "hotkey_ss58" else None),
            )
        if value is not None and is_bittensor_address(value):
            booked = next(
                (e["name"] for e in cfg.load_addresses() if e.get("address") == value), None
            )
            self.output.name_address(value, booked)
            self.output.classify_address(value, kind)
            return value
        if value is not None:
            booked = cfg.get_address(value)
            if booked:
                self.output.name_address(booked, value)
                self.output.classify_address(booked, kind)
                return booked
            proxy_entry = cfg.get_proxy(value)
            proxied = proxy_entry.get("address") if proxy_entry else None
            if isinstance(proxied, str) and proxied:
                self.output.name_address(proxied, value)
                self.output.classify_address(proxied, kind)
                return proxied
        try:
            if value is None:
                if param == "hotkey_ss58":
                    address = self.wallet().hotkey.ss58_address
                    self.output.name_address(address, f"{self.wallet_name}/{self.hotkey_name}")
                    self.output.classify_address(address, "hotkey")
                    return address
                if param == "coldkey_ss58":
                    address = self.wallet().coldkeypub.ss58_address
                    self.output.name_address(address, self.wallet_name)
                    self.output.classify_address(address, "coldkey")
                    return address
                return None
            if "hotkey" in param:
                wallet_name, _, hotkey = value.rpartition("/")
                handle = wallets.open_wallet(
                    wallet_name or self.wallet_name, hotkey, self.wallet_path
                )
                self.output.name_address(handle.hotkey.ss58_address, value)
                self.output.classify_address(handle.hotkey.ss58_address, "hotkey")
                return handle.hotkey.ss58_address
            address = wallets.open_wallet(name=value, path=self.wallet_path).coldkeypub.ss58_address
            self.output.name_address(address, value)
            self.output.classify_address(address, "coldkey")
            return address
        except Exception as error:
            shown = value if value is not None else f"{self.wallet_name}/{self.hotkey_name}"
            self.output.error(f"cannot resolve {address_cli_name(param)} {shown!r}: {error}")
            raise typer.Exit(1)

    def resolve_signatory_list(self, raw: str) -> list[str]:
        """Resolve comma-separated signatory refs (ss58, address-book name, wallet)."""
        parts = [part.strip() for part in raw.split(",") if part.strip()]
        if not parts:
            raise ValueError("need at least one signatory")
        resolved: list[str] = []
        for part in parts:
            address = self.resolve_address("coldkey_ss58", part)
            if not address:
                raise ValueError(f"cannot resolve {part!r}")
            resolved.append(address)
        return list(dict.fromkeys(resolved))

    def _register_local_names(self, wallet) -> None:
        """Teach the renderer the local names for addresses it may print: the
        signing wallet's keys and every address-book contact."""
        try:
            self.output.name_address(wallet.coldkeypub.ss58_address, self.wallet_name)
            self.output.classify_address(wallet.coldkeypub.ss58_address, "coldkey")
        except Exception:
            pass  # coldkey-less wallet dir; names are cosmetic
        try:
            self.output.name_address(
                wallet.hotkey.ss58_address, f"{self.wallet_name}/{self.hotkey_name}"
            )
            self.output.classify_address(wallet.hotkey.ss58_address, "hotkey")
        except Exception:
            pass
        for entry in cfg.load_addresses():
            self.output.name_address(entry.get("address"), entry.get("name"))
        for entry in cfg.load_proxies():
            self.output.name_address(entry.get("address"), entry.get("name"))

    def submit(
        self,
        intent,
        *,
        proxy_for: Optional[str] = None,
        force_proxy_type: Optional[str] = None,
    ) -> Optional[ExtrinsicResult]:
        """Run a mutation with a uniform dry-run / confirm / execute / render flow.

        ``--dry-run`` shows the plan (fee, effects, warnings, policy) and stops.
        Otherwise the intent's own summary is the confirmation prompt; the intent
        is then executed, its result rendered, and the ``ExtrinsicResult``
        returned (None when the dry-run path stopped early). The prompt/summary
        is never hand-written per command — it comes from the intent.

        ``proxy_for`` dispatches the call as that account via ``Proxy.proxy``,
        signed by the local wallet key (which must be its registered proxy).
        When omitted, the persistent ``proxy_for`` config value (if set) is
        used; the sentinel ``self`` bypasses that default and signs directly.

        Stake-trading intents (``mev_shield_default``) are submitted
        MEV-shielded via ``client.submit_shielded`` unless the user opts out
        with ``--no-mev-shield`` or ``btcli config set mev_shield false``;
        ``--mev-shield`` (or config true) opts any mutation in.
        """
        # Inline import: prompt.py imports AppContext from this module, so a
        # top-level import here would be circular.
        from .prompt import confirm_wallet

        if proxy_for == "self":
            proxy_for = None
        elif proxy_for is None:
            configured = cfg.get("proxy_for")
            if configured:
                proxy_for = self.resolve_address("proxy_for", str(configured))
                self.output.message(
                    f"[dim]dispatching via configured proxy_for {configured!r} "
                    "— pass `--proxy-for self` to sign directly[/dim]"
                )

        # MEV shielding: explicit flag > persistent config > the intent's own
        # default. `forced` distinguishes "the user asked for shielding" (hard
        # failure when it can't be honored) from "the built-in stake default"
        # (downgraded with a notice when the chain or flow can't shield).
        # `mev_shield_required` intents (collateral AMM buys) refuse the
        # unshielded opt-out entirely.
        configured_shield = cfg.get("mev_shield")
        if intent.mev_shield_required:
            if self.mev_shield is False or configured_shield is False:
                self.output.error(
                    f"{intent.op} must be submitted MEV-shielded",
                    help=(
                        "collateral / burned-registration AMM fills cannot run "
                        "unshielded; omit --no-mev-shield"
                    ),
                )
                raise typer.Exit(2)
            shield = True
            shield_forced = True
        elif self.mev_shield is not None:
            shield = self.mev_shield
            shield_forced = shield
        elif configured_shield is not None:
            shield = bool(configured_shield)
            shield_forced = shield
        else:
            shield = intent.mev_shield_default
            shield_forced = False
        if shield and (proxy_for is not None or self.uses_extension_signer()):
            blocker = "a proxied call" if proxy_for is not None else "the extension signer"
            if shield_forced or intent.mev_shield_required:
                self.output.error(
                    f"MEV shielding cannot wrap {blocker}",
                    help=(
                        "collateral intents cannot fall back to unshielded; "
                        "sign directly without a proxy/extension"
                        if intent.mev_shield_required
                        else "pass --no-mev-shield to submit unshielded"
                    ),
                )
                raise typer.Exit(2)
            self.output.message(
                f"[dim]MEV shield skipped: cannot wrap {blocker} — submitting unshielded[/dim]"
            )
            shield = False

        # The signing wallet is confirmed when it was only defaulted (generated
        # tx commands already did this via signer_specs and set wallet_given).
        confirm_wallet(
            self,
            help_text=(
                "Wallet containing the signing hotkey."
                if intent.signer == "hotkey"
                else "Wallet whose coldkey signs this transaction."
            ),
            require_coldkey=intent.signer == "coldkey",
        )
        wallet = self.wallet()
        self._register_local_names(wallet)
        # Outer MevShield.submit_encrypted has no netuid, so fees_in_alpha cannot
        # route — free TAO must cover the carrier. Bare remove_stake (etc.) can
        # pay in alpha; downgrade the stake default before confirm so the summary
        # is honest and α-only coldkeys still unstake. External signers resolve
        # the fee payer later; _execute repeats this check for them.
        if shield and not self.uses_external_signer():
            try:
                fee_payer = public_view(wallet, intent.signer).ss58_address
            except Exception:
                fee_payer = None
            if fee_payer is not None:

                async def _shield_fee_preflight(client):
                    return await self._shield_outer_fee_shortfall(client, fee_payer)

                shortfall = self.run(_shield_fee_preflight)
                if shortfall is not None:
                    if shield_forced or intent.mev_shield_required:
                        self.output.error(
                            "MEV-shielded submission needs free TAO for the outer carrier fee",
                            help=(
                                f"{intent.op} cannot submit unshielded"
                                if intent.mev_shield_required
                                else "pass --no-mev-shield to submit unshielded "
                                "(alpha fees work on the bare call), or fund the "
                                "signing account with free TAO"
                            ),
                        )
                        raise typer.Exit(2)
                    self.output.message(
                        "[dim]MEV shield skipped: free TAO cannot cover the outer "
                        "carrier fee (alpha fees only work on the unshielded call) "
                        "— submitting unshielded[/dim]"
                    )
                    shield = False
        options = {"proxy_for": proxy_for, "proxy_type": force_proxy_type}
        registration_quote = None
        registration_flow = None
        summary = intent.summary()
        if intent.op == "register_subnet":

            async def _registration_preview(client):
                block = await client.block()
                quote_rao = await client.runtime(
                    runtime_apis.SubnetRegistrationRuntimeApi.get_network_registration_cost,
                    [],
                    block=block,
                )
                quote = client.balance(int(quote_rao))
                if self.assume_yes:
                    return quote, None
                networks, subnet_limit, cleanup_queue, registration_queue = await asyncio.gather(
                    client.query_map(storage.SubtensorModule.NetworksAdded, block=block),
                    client.query(storage.SubtensorModule.SubnetLimit, block=block),
                    client.query(storage.SubtensorModule.DissolveCleanupQueue, block=block),
                    client.query(storage.SubtensorModule.NetworkRegistrationQueue, block=block),
                )
                active = sum(1 for netuid, added in networks if int(netuid) != 0 and bool(added))
                cleanup_queue = list(cleanup_queue or [])
                registration_queue = list(registration_queue or [])
                if active + len(cleanup_queue) < int(subnet_limit):
                    return quote, "immediate · no deregistration needed"
                if len(cleanup_queue) > len(registration_queue):
                    target = int(cleanup_queue[0]) if cleanup_queue else None
                    flow = "queued · waits for deregistration cleanup"
                    if target is not None:
                        flow += f" of subnet {target}"
                    return quote, flow
                prune = await client.runtime(
                    runtime_apis.SubnetInfoRuntimeApi.get_subnet_to_prune,
                    [],
                    block=block,
                )
                if prune is None:
                    return quote, "blocked · no subnet is eligible for deregistration"
                return quote, f"queued · deregisters subnet {int(prune)} before registration"

            registration_quote, registration_flow = self.run(_registration_preview)
            summary += f" for {registration_quote.decimal:,.9f} TAO"
        summary += f" [as {proxy_for} via proxy]" if proxy_for else ""
        if shield:
            summary += " [MEV-shielded]"

        # Privileged intents say up front which key the chain will accept, so
        # nobody signs (or approves a multisig) before learning the call needs
        # a different origin.
        if intent.origin == "root":
            self.output.message("[dim]requires: chain sudo key (call wrapped in Sudo.sudo)[/dim]")
        elif intent.origin == "subnet_owner":
            self.output.message("[dim]requires: subnet owner coldkey[/dim]")

        if self.uses_extension_signer():
            self.reset_extension_session()
        if self.uses_vault_signer():
            # Display-only context for the vault page ("what am I signing?").
            self.vault_signer().summary = summary

        async def _plan(client):
            signer = await self.resolve_signing_wallet(intent.signer, pick_account=True)
            plan_target = signer if self.uses_external_signer() else wallet
            try:
                return await client.plan(intent, plan_target, **options)
            finally:
                if self.uses_extension_signer() and hasattr(signer, "close"):
                    await signer.close()

        if self.dry_run:
            plan = self.run(_plan)
            if shield:
                plan.effects.append(
                    "submitted MEV-shielded: the call stays encrypted in the "
                    "mempool until the block author decrypts and executes it"
                )

                async def _shield_fee_warning(client):
                    return await self._shield_outer_fee_shortfall(client, plan.signer_address)

                shortfall = self.run(_shield_fee_warning)
                if shortfall is not None:
                    plan.warnings.append(shortfall)
            self.output.plan(plan)
            if not plan.ok:
                raise typer.Exit(1)
            return None

        # Surface intent warnings before the user confirms (dry-run already
        # prints them via ``output.plan``). Keep this cheap: warnings-only, no
        # fee estimation. Skip when an external signer still needs its account
        # picker — those paths resolve the origin later.
        if not self.uses_external_signer():

            async def _warnings(client):
                try:
                    origin = public_view(wallet, intent.signer).ss58_address
                except Exception:
                    return []
                return list(await intent.warnings(client._substrate, proxy_for or origin))

            for warning in self.run(_warnings):
                rendered = self.output.with_subnets(self.output.with_names(warning))
                self.output.message(f"[{STYLE_WARNING}]warning:[/{STYLE_WARNING}] {rendered}")

        if self.uses_extension_signer():

            async def _prepare(_client):
                signer = await self.extension_signer(pick_account=True)
                await signer.close()

            self.run(_prepare)
            if registration_flow is not None and not self.assume_yes:
                self.output.message(f"[dim]{registration_flow}[/dim]")
            # The extension popup is the approval surface: it shows this same
            # transaction and requires an explicit approve/reject there, so a
            # terminal prompt would only bounce the user between windows.
            self.output.message(summary)
            self.output.message(
                "[dim]approve or reject the request in the wallet extension popup[/dim]"
            )
        else:
            if registration_flow is not None and not self.assume_yes:
                self.output.message(f"[dim]{registration_flow}[/dim]")
            self.confirm(f"{summary}?")

        # Native keyfiles unlock synchronously on their first signature. Unlock
        # encrypted coldkeys eagerly instead: a wrong password can be retried
        # on the spot (instead of aborting and losing every prompt answer),
        # and the password prompt stays visible — the lazy SDK signer would
        # prompt underneath register_subnet's live spinner and appear to hang.
        local_signer = None
        if (
            intent.signer == "coldkey"
            and not self.uses_external_signer()
            and (intent.op == "register_subnet" or _coldkey_encrypted(wallet))
        ):
            signing_key = self._unlock_coldkey(wallet)
            try:
                hotkey = wallet.hotkeypub
            except FileNotFoundError:
                try:
                    hotkey = wallet.hotkey
                except Exception:
                    hotkey = None  # coldkey-only wallet; intents needing it fail on use
            # Keep the wallet shape because intents may default their hotkey
            # from it, while replacing the signing side with the keypair that
            # was already unlocked above.
            local_signer = SimpleNamespace(
                coldkey=signing_key,
                coldkeypub=wallet.coldkeypub,
                hotkey=hotkey,
            )

        def activity_update(_text: str, _announce: bool = False) -> None:
            pass

        def _registration_progress(progress: dict) -> None:
            stage = progress.get("stage")
            cleanup = progress.get("deregistered_netuid") or progress.get("cleanup_netuid")
            if stage == "queued":
                if progress.get("deregistered_netuid") is not None:
                    text = f"capacity is full · deregistering subnet {cleanup} before registration"
                elif cleanup is not None:
                    text = f"registration queued · waiting for subnet {cleanup} cleanup"
                else:
                    text = "registration queued · waiting for deregistration cleanup"
                activity_update(text, True)
            elif stage == "waiting":
                subject = f"subnet {cleanup} cleanup" if cleanup is not None else "cleanup"
                elapsed = int(progress.get("blocks_since_call", 0))
                unit = "block" if elapsed == 1 else "blocks"
                activity_update(
                    f"{subject} · {elapsed} {unit} since call · waiting for NetworkAdded",
                    False,
                )
            elif stage == "registered":
                activity_update(
                    f"NetworkAdded · subnet {progress.get('netuid')} · "
                    f"block {progress.get('block')}",
                    False,
                )

        async def _execute(client):
            use_shield = shield
            if use_shield and await client.read("mev_shield_next_key") is None:
                # The MevShield pallet isn't active here (e.g. localnet). A
                # forced / required shield must fail loudly; the built-in
                # default degrades visibly so the command still works.
                if shield_forced or intent.mev_shield_required:
                    raise BittensorError(
                        "MEV shield is not active on this network "
                        "(MevShield.NextKey is unset); "
                        + (
                            f"{intent.op} cannot submit unshielded"
                            if intent.mev_shield_required
                            else "pass --no-mev-shield to submit unshielded"
                        )
                    )
                self.output.message(
                    "[dim]MEV shield is not active on this network — submitting unshielded[/dim]"
                )
                use_shield = False
            signer = local_signer or await self.resolve_signing_wallet(intent.signer)
            if use_shield and self.uses_external_signer():
                # Local wallets already preflighted before confirm; external
                # signers only know the fee payer after account pick.
                try:
                    fee_payer = public_view(signer, intent.signer).ss58_address
                except Exception:
                    fee_payer = None
                shortfall = (
                    await self._shield_outer_fee_shortfall(client, fee_payer)
                    if fee_payer is not None
                    else None
                )
                if shortfall is not None:
                    if shield_forced or intent.mev_shield_required:
                        raise BittensorError(
                            "MEV-shielded submission needs free TAO for the outer "
                            "carrier fee; "
                            + (
                                f"{intent.op} cannot submit unshielded"
                                if intent.mev_shield_required
                                else "pass --no-mev-shield to submit unshielded "
                                "(alpha fees work on the bare call)"
                            )
                        )
                    self.output.message(
                        "[dim]MEV shield skipped: free TAO cannot cover the outer "
                        "carrier fee (alpha fees only work on the unshielded call) "
                        "— submitting unshielded[/dim]"
                    )
                    use_shield = False
            if use_shield and self.uses_vault_signer():
                # Shielded signing runs against an 8-block (~96 s) era, and
                # a shielded submission means two QR scans (the shielded
                # transaction, then its encrypted carrier). Warm the page and
                # the user up first so the countdown starts when they're
                # ready, not while they're finding their phone.
                signer.two_stage = True
                self.output.message(
                    "MEV-shielded vault signing: two scans, ~90 seconds total — "
                    "have the phone unlocked with Vault's scanner open"
                )
                await signer.warm_up()
            result = None
            try:
                if use_shield:
                    shield_options = {"wait_for_finalization": False}
                    if intent.op == "register_subnet":
                        shield_options["on_progress"] = _registration_progress
                    result = await client.submit_shielded(
                        intent,
                        signer,
                        **shield_options,
                    )
                else:
                    execute_options = {
                        "wait_for_finalization": False,
                        **options,
                    }
                    if intent.op == "register_subnet":
                        execute_options["on_progress"] = _registration_progress
                    result = await client.execute(
                        intent,
                        signer,
                        **execute_options,
                    )
                    await self._attach_multisig_followup(client, intent, result)
                return result
            finally:
                if hasattr(signer, "report_transaction_result"):
                    with contextlib.suppress(Exception):
                        await signer.report_transaction_result(
                            bool(result is not None and result.success)
                        )
                if hasattr(signer, "close"):
                    await signer.close()

        if intent.op == "register_subnet":
            with self.output.activity("submitting subnet registration") as update:
                activity_update = update
                result = self.run(_execute)
            if (
                result.success
                and registration_quote is not None
                and "registration_price_rao" not in result.data
            ):
                result = replace(
                    result,
                    data={
                        **result.data,
                        "registration_price_rao": registration_quote.rao,
                    },
                )
        else:
            result = self.run(_execute)
        rendered = (
            self.output.registration_result(result)
            if intent.op == "register_subnet"
            else self.output.result(result, summary)
        )
        if not rendered:
            if shield and result.data.get("shielded") is not True:
                # The failure happened at (or before) the encrypted pool
                # submission. Outer carrier fees need free TAO; alpha fees only
                # apply to the bare call.
                self.output.message(
                    "[dim]the submission was MEV-shielded; the outer carrier needs "
                    "free TAO (alpha fees only work unshielded) — retry with "
                    "--no-mev-shield[/dim]"
                )
            raise typer.Exit(1)
        if intent.verify:
            # The paired read that confirms the effect actually landed.
            self.output.message(
                f"[dim]verify: `btcli query {intent.verify.replace('_', '-')} ...`[/dim]"
            )
        return result

    async def _shield_outer_fee_shortfall(
        self, client: Client, fee_payer_ss58: str
    ) -> Optional[str]:
        """Warning text when free TAO cannot cover a MevShield carrier fee.

        Returns ``None`` when the payer looks able to cover it (or the check
        cannot run). The outer ``submit_encrypted`` call has no netuid, so the
        fee pallet cannot fall back to alpha — unlike bare ``remove_stake``.
        """
        try:
            free = await client.balances.get(fee_payer_ss58)
            # Exact ciphertext size is unknown until encrypt; length fee is
            # 1 rao/byte so a padded dummy keeps the estimate conservative.
            outer = await client.compose(
                generated_calls.MevShield.submit_encrypted(ciphertext=bytes(8192))
            )
            fee = await client._substrate.estimate_fee(outer, _FeeAddressView(fee_payer_ss58))
        except Exception:
            # Best-effort: if we only know free is zero, that is enough to warn.
            try:
                free = await client.balances.get(fee_payer_ss58)
            except Exception:
                return None
            if free.rao > 0:
                return None
            return (
                "free TAO cannot cover the MEV-shield outer carrier fee; alpha "
                "fees only apply to the unshielded call — use --no-mev-shield"
            )
        if free.rao >= fee.rao:
            return None
        return (
            f"free TAO ({free}) cannot cover the MEV-shield outer carrier fee "
            f"(~{fee}); alpha fees only apply to the unshielded call — use "
            "--no-mev-shield"
        )

    def _unlock_coldkey(self, wallet):
        """Unlock the wallet coldkey, re-prompting on a wrong password.

        The retry only applies when the password came from an interactive
        prompt; non-interactive sources (env var, password file, Keychain,
        the macOS dialog) would fail identically on a retry, so they keep
        the single attempt and the usual remediation hint.
        """
        try:
            password = wallets.resolve_wallet_password(
                wallet,
                password_file=self.wallet_password_file,
                macos_prompt=self.macos_password,
                keychain=self.keychain_password,
            )
        except ValueError as error:
            self.output.error(str(error))
            raise typer.Exit(1)
        prompted = password is None and sys.stdin.isatty()
        for attempts_left in (2, 1, 0):
            try:
                return wallets.signing_keypair(
                    wallet,
                    "coldkey",
                    password=password,
                    macos_prompt=self.macos_password,
                    keychain=self.keychain_password,
                )
            except ValueError as error:
                wrong = str(error).lower().startswith("wrong password")
                if wrong and prompted and attempts_left:
                    self.output.error("wrong password")
                    continue
                if wrong:
                    self.output.error(
                        "wrong password",
                        help=(
                            "re-save with `btcli wallet keychain save`"
                            if self.keychain_password
                            else "re-run and enter the coldkey password used when "
                            "the key was created"
                        ),
                    )
                else:
                    self.output.error(str(error))
                raise typer.Exit(1)

    async def _attach_multisig_followup(self, client, intent, result: ExtrinsicResult) -> None:
        """After a multisig approve/execute intent, cache the inner call locally
        and attach co-signer instructions (timepoint, ready-to-run commands) so
        reviewers can act straight from `multisig pending`."""
        if not result.success or not result.data.get("multisig_call_hash"):
            return
        if self.uses_external_signer():
            return  # no local wallet to derive the signer address from
        try:
            wallet = self.wallet()
            signer_address = (
                wallet.hotkey.ss58_address
                if intent.signer == "hotkey"
                else wallet.coldkeypub.ss58_address
            )
            followup = await ms_helpers.multisig_followup_for_intent(
                client, self, intent=intent, result=result, signer_address=signer_address
            )
        except Exception:
            return  # advisory only; never fail a successful submission over it
        if followup:
            result.data["multisig_followup"] = followup

    def run(self, work: Callable[[Client], Awaitable[T]]) -> T:
        """Open a client, run ``work``, and translate SDK/connection errors into
        clean messages with a non-zero exit code (never a traceback).

        While connected, the netuid -> subnet-name cache is refreshed (at most
        once per TTL) concurrently with ``work``, so netuid references render
        as "4 (Targon)" everywhere — including prompts that run offline.
        """

        async def _main() -> T:
            async with Client(
                self.network,
                fallback_endpoints=_endpoint_pool(self.fallback_endpoints),
                archive_endpoints=_endpoint_pool(self.archive_endpoints),
                retry_forever=self.retry_forever,
            ) as client:
                # connect() just loaded the chain's token symbols (disk cache
                # or one map scan); hand them to the renderer so amounts and
                # netuid references carry the real symbols.
                self.output.update_token_symbols(client.token_symbols)
                names_task = (
                    None
                    if cfg.subnet_names_fresh(self.network)
                    else asyncio.create_task(client.read("subnet_names"))
                )
                try:
                    result = await work(client)
                except BaseException:
                    if names_task is not None:
                        names_task.cancel()
                        await asyncio.gather(names_task, return_exceptions=True)
                    raise
                if names_task is not None:
                    # names are cosmetic; never fail a command over them
                    with contextlib.suppress(Exception):
                        self.output.update_subnet_names(await names_task)
                return result

        try:
            return asyncio.run(_main())
        except (BittensorError, ValueError) as error:
            # A raised ChainError carries the same diagnostics a failed
            # ExtrinsicResult would (remediation, docs page with source links)
            # — print them instead of the bare message.
            if isinstance(error, ChainError):
                self.output.chain_error(error)
            elif isinstance(error, RpcPolicyError):
                retry = None
                if error.retry_after:
                    retry = (
                        f"wait {error.retry_after} seconds, then retry"
                        if error.retry_after.isdigit()
                        else f"retry after {error.retry_after}"
                    )
                self.output.error(
                    str(error),
                    help=retry or "reduce the request or connection rate, then retry",
                )
            elif isinstance(error, RpcConnectionError):
                self.output.error(
                    f"could not reach {self.network}: {error}",
                    help="check the endpoint and network connection, then retry",
                )
            elif isinstance(error, PolicyError):
                self.output.error(
                    str(error),
                    help=REMEDIATION[ErrorCode.POLICY_VIOLATION],
                    see=error_docs_url(ErrorCode.POLICY_VIOLATION.value),
                )
            elif str(error).lower().startswith("wrong password"):
                self.output.error(
                    "wrong password",
                    help=(
                        "re-save with `btcli wallet keychain save`"
                        if self.keychain_password
                        else "re-run and enter the coldkey password used when the key was created"
                    ),
                )
            else:
                self.output.error(str(error))
            raise typer.Exit(1)
        except BridgeError as error:
            self.output.error(str(error))
            raise typer.Exit(1)
        except LedgerError as error:
            self.output.error(
                str(error),
                help="check the device is connected, unlocked, and the Polkadot app is open",
            )
            raise typer.Exit(1)
        except RuntimeError as error:
            if "different loop" in str(error).lower():
                self.output.error("extension bridge connection lost", help="retry the command")
                raise typer.Exit(1)
            raise
        except TypeError as error:
            self.output.error(str(error))
            raise typer.Exit(1)
        except (ConnectionError, TimeoutError, OSError) as error:
            detail = str(error).strip() or "the connection timed out without a response"
            self.output.error(f"could not reach {self.network}: {detail}")
            raise typer.Exit(1)
        except KeyboardInterrupt:
            self.output.message("aborted.")
            raise typer.Exit(130)

    def confirm(self, prompt: str) -> None:
        """Gate a state-changing action. ``--yes`` skips it; a non-interactive
        session without ``--yes`` is refused rather than left hanging on a prompt."""
        if self.assume_yes:
            return
        if self.output.json_mode or not sys.stdin.isatty():
            self.output.error(
                "refusing to submit without confirmation",
                help="pass `--yes` to skip the prompt in non-interactive sessions",
            )
            raise typer.Exit(1)
        try:
            accepted = self.output.confirm(prompt)
        except (KeyboardInterrupt, EOFError):
            self.output.message("aborted.")
            raise typer.Exit(130)
        if not accepted:
            self.output.message("aborted.")
            raise typer.Exit(1)


def _coldkey_encrypted(wallet) -> bool:
    """Whether the wallet's coldkey file exists and is password-protected."""
    try:
        return bool(wallet.coldkey_file.is_encrypted())
    except Exception:
        return False


def _endpoint_pool(value: Optional[str]) -> Optional[list[str]]:
    """Parse a comma-separated endpoint list.

    None -> None (the network's public defaults apply); "none"/"off" -> []
    (pinned to the primary endpoint); otherwise the listed ws(s):// URLs.
    """
    if value is None:
        return None
    text = value.strip()
    if text.lower() in ("none", "off"):
        return []
    return [url.strip() for url in text.split(",") if url.strip()]


def ctx_of(ctx: typer.Context) -> AppContext:
    """Fetch the AppContext built by the root callback."""
    return ctx.obj
