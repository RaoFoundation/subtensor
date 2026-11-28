"""Configures the rig's hub, in two steps.

``prepare`` runs before any contract is deployed: it makes the localnet's EVM usable and
funds the accounts that deploy and carry messages.

``configure`` runs when the contracts are deployed and the transport's agents run. It does
what the owner of the rails does at a launch, in the same order: approve the bridge, name
the gateway, create the subnets, open the pool, register the tokens, open the markets.
Then it writes the manifest that btcli, the e2e and the swap page read.
"""

from __future__ import annotations

import asyncio
import json
import sys

from rig import (
    DEPLOYER,
    DEPLOYER_KEY,
    DEPLOYMENTS,
    HUB_CHAIN_ID,
    HUB_RPC,
    HUB_WS,
    KEEPER,
    MANIFEST,
    RELAYER,
    SPOKE_BRIDGE,
    SPOKE_CHAIN_ID,
    SPOKE_DOMAIN,
    SPOKE_NAME,
    SPOKE_RPC,
    STATE,
    SUBNET_NAMES,
    TAO_PRICE,
    UNIT,
    USDC,
    USER,
    USER_KEY,
    VALIDATED,
    VALIDATOR,
    VALIDATORS,
    Spoke,
    client,
    dev_wallet,
    index_hotkey,
    load_json,
    miner_wallet,
    mirror,
    oracle_wallet,
    score_miner,
    submit,
    validator_wallets,
    wait_for,
)

import bittensor as bt
from bittensor import rails
from bittensor.evm.addresses import ss58_to_pubkey

# The TAO a subnet's pool starts with, and the TAO each of the rig's validators stakes.
SUBNET_POOL = 100_000
VALIDATOR_STAKE = 10_000

# What the first liquidity provider puts in the pool: 2,500 TAO and 1,000,000 tUSD.
POOL_TUSD = 1_000_000
POOL_TAO = POOL_TUSD // TAO_PRICE

# What `just rails-page` gives the swap page: the rig's addresses.
PAGE_ENV = STATE / "page.env"


def owner_wallet(index: int):
    """The owner of the rig's subnet number `index`: Alice, with a hotkey for each subnet."""
    return dev_wallet("//Alice", f"//Alice//rails//{index}")


async def prepare() -> None:
    alice = dev_wallet()
    oracle = oracle_wallet()
    async with client() as chain:
        root = await chain.query(bt.storage.Sudo.Key)
        assert root == alice.coldkey.ss58_address, "the rig needs a localnet whose root is Alice"

        await submit(
            chain,
            bt.calls.AdminUtils.sudo_set_evm_chain_id(chain_id=HUB_CHAIN_ID),
            alice,
            root=True,
        )
        await submit(chain, bt.calls.EVM.disable_whitelist(disabled=True), alice, root=True)
        # The accounts that pay gas on the hub's EVM, and the account that feeds the price.
        for account in map(mirror, (DEPLOYER, VALIDATOR, RELAYER, KEEPER, USER)):
            await submit(
                chain,
                bt.calls.Balances.transfer_keep_alive(dest=account, value=1_000 * UNIT),
                alice,
            )
        await submit(
            chain,
            bt.calls.Balances.transfer_keep_alive(
                dest=oracle.coldkey.ss58_address, value=100 * UNIT
            ),
            alice,
        )
        await submit(
            chain,
            bt.calls.AdminUtils.sudo_set_tao_usd_price_oracle(oracle=oracle.coldkey.ss58_address),
            alice,
            root=True,
        )
    print("[rails] the hub is ready for contracts")


async def create_subnets(chain: bt.Client, alice) -> list[dict]:
    """Creates the rig's subnets, with a service on each that takes credits."""
    for call in (
        bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0),
        bt.calls.AdminUtils.sudo_set_start_call_delay(delay=0),
        # A localnet's epochs are short: without this, settings are refused near each one.
        bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0),
        bt.calls.AdminUtils.sudo_set_gamma_enabled(enabled=True),
        # An index hotkey takes new children as often as an owner could change them. In
        # production that is a day. Here it is a few blocks, so that the rig can show it.
        bt.calls.SubtensorModule.set_pending_childkey_cooldown(cooldown=20),
        # What a subnet's owner locks becomes the subnet's pool. A localnet pays all of
        # its emission into two small pools, so their prices would climb by the second.
        # Deep pools hold their price, as the pools of real subnets do.
        bt.calls.AdminUtils.sudo_set_network_min_lock_cost(lock_cost=SUBNET_POOL * UNIT),
    ):
        await submit(chain, call, alice, root=True)

    subnets = []
    for index, name in enumerate(SUBNET_NAMES):
        owner = owner_wallet(index)
        hotkey = owner.hotkey.ss58_address
        before = {subnet.netuid for subnet in await chain.subnets.all()}
        identity = {
            "subnet_name": name,
            "github_repo": "",
            "subnet_contact": "",
            "subnet_url": "",
            "discord": "",
            "description": f"The rig's copy of {name}",
            "logo_url": "",
            "additional": "",
        }
        await submit(
            chain,
            bt.calls.SubtensorModule.register_network_with_identity(
                hotkey=hotkey, identity=identity
            ),
            alice,
        )
        (netuid,) = {subnet.netuid for subnet in await chain.subnets.all()} - before
        await submit(chain, bt.calls.SubtensorModule.start_call(netuid=netuid), alice)
        await submit(
            chain,
            bt.calls.AdminUtils.sudo_set_subnet_gamma_enabled(netuid=netuid, enabled=True),
            alice,
            root=True,
        )
        await submit(
            chain,
            bt.calls.SubtensorModule.set_service(
                netuid=netuid, hotkey=hotkey, url=f"https://{name.lower()}.example"
            ),
            alice,
        )
        await add_validators(chain, alice, index, netuid)
        subnets.append({"netuid": netuid, "name": name, "service": hotkey})
        print(f"[rails] subnet {netuid} is {name}", flush=True)
    return subnets


async def add_validators(chain: bt.Client, alice, index: int, netuid: int) -> None:
    """Gives the subnet validators that earn, so that its index hotkey has children.

    Four validators with the same stake score one miner. Each then has the same
    dividend, and the index rule lends a quarter of the index's stake weight to each.
    """
    storage = bt.storage.SubtensorModule
    for call in (
        bt.calls.AdminUtils.sudo_set_weights_set_rate_limit(
            netuid=netuid, weights_set_rate_limit=0
        ),
        bt.calls.AdminUtils.sudo_set_commit_reveal_weights_enabled(netuid=netuid, enabled=False),
        bt.calls.AdminUtils.sudo_set_max_registrations_per_block(
            netuid=netuid, max_registrations_per_block=VALIDATORS + 1
        ),
        bt.calls.AdminUtils.sudo_set_target_registrations_per_interval(
            netuid=netuid, target_registrations_per_interval=VALIDATORS + 1
        ),
        # A validator stays active for this many tempos after it last set weights: the
        # most the chain allows. On a localnet that is two minutes, so the keeper has
        # the validators score again and again.
        bt.calls.AdminUtils.sudo_set_activity_cutoff_factor(netuid=netuid, factor_milli=50_000),
    ):
        await submit(chain, call, alice, root=True)

    miner = miner_wallet(index)
    validators = validator_wallets(index)
    for wallet in (miner, *validators):
        hotkey = wallet.hotkey.ss58_address
        await submit(
            chain, bt.calls.SubtensorModule.burned_register(netuid=netuid, hotkey=hotkey), alice
        )
    for wallet in validators:
        hotkey = wallet.hotkey.ss58_address
        # A validator signs its weights with its hotkey, which pays the fee.
        await submit(
            chain, bt.calls.Balances.transfer_keep_alive(dest=hotkey, value=10 * UNIT), alice
        )
        await submit(
            chain,
            bt.calls.SubtensorModule.add_stake(
                hotkey=hotkey, netuid=netuid, amount_staked=VALIDATOR_STAKE * UNIT
            ),
            alice,
        )
    uids = [
        await chain.query(storage.Uids, [netuid, wallet.hotkey.ss58_address])
        for wallet in validators
    ]

    async def permitted():
        permits = await chain.query(storage.ValidatorPermit, [netuid])
        return all(uid < len(permits) and permits[uid] for uid in uids)

    # An epoch gives the permits. Only then may a validator score a miner.
    await wait_for(f"the validator permits on subnet {netuid}", permitted)
    miner_uid = await chain.query(storage.Uids, [netuid, miner.hotkey.ss58_address])
    await score_miner(chain, index, netuid, miner_uid)
    # From here on the keeper keeps these validators active.
    scored = load_json(VALIDATED) if VALIDATED.exists() else []
    scored.append({"index": index, "netuid": netuid, "miner_uid": miner_uid})
    VALIDATED.write_text(json.dumps(scored))


async def wait_for_index(chain: bt.Client, netuid: int) -> None:
    """Waits until the index hotkey of `netuid` lends to all of the rig's validators."""

    async def lent():
        children = await chain.query(
            bt.storage.SubtensorModule.ChildKeys, [index_hotkey(netuid), netuid]
        )
        return len(children or []) == VALIDATORS

    await wait_for(f"the index hotkey of subnet {netuid} to pick its validators", lent)


async def open_pool(chain: bt.Client, spoke: Spoke, alice) -> None:
    """Enables the pool and puts the first liquidity in it.

    tUSD exists only against the stablecoin a vault holds, so the first liquidity
    provider does what anyone does: deposits the stablecoin on the spoke, receives tUSD
    on Bittensor, and adds it to the pool with TAO.
    """
    await submit(chain, bt.calls.CanonicalPool.set_enabled(enabled=True), alice, root=True)
    address = alice.coldkey.ss58_address
    spoke.mint_stable(DEPLOYER_KEY, DEPLOYER, POOL_TUSD * USDC)
    spoke.deposit(DEPLOYER_KEY, POOL_TUSD * USDC, rails.to_native_tusd(address))

    async def arrived():
        return await chain.query(bt.storage.Tusd.Balances, [address]) >= POOL_TUSD * UNIT

    await wait_for("the first tUSD to cross the bridge", arrived)
    await submit(
        chain,
        bt.calls.CanonicalPool.add_liquidity(
            max_tao=POOL_TAO * UNIT, max_tusd=POOL_TUSD * UNIT, min_shares=0
        ),
        alice,
    )
    print(f"[rails] the pool holds {POOL_TAO} TAO and {POOL_TUSD} tUSD")


async def open_markets(chain: bt.Client, spoke: Spoke, alice, subnets: list[dict]) -> None:
    """Registers canonical TAO and opens a market for the alpha of each subnet."""
    await submit(
        chain,
        bt.calls.Rails.register_token(bridge=SPOKE_BRIDGE, asset="Tao", address=spoke.tao),
        alice,
        root=True,
    )
    await submit(
        chain, bt.calls.Rails.set_tao_cap(bridge=SPOKE_BRIDGE, cap=100_000 * UNIT), alice, root=True
    )
    for subnet in subnets:
        netuid = subnet["netuid"]
        await submit(
            chain, bt.calls.Rails.open_market(netuid=netuid, cap=1_000_000 * UNIT), alice, root=True
        )
        # Opening a market opens the subnet's index hotkey.
        await wait_for_index(chain, netuid)
        await submit(
            chain,
            bt.calls.Rails.list_market(
                netuid=netuid, bridge=SPOKE_BRIDGE, address=spoke.alpha_token_address(netuid)
            ),
            alice,
            root=True,
        )

    # Listing a market tells the portal to create the token. Wait for each one.
    for subnet in subnets:
        netuid = subnet["netuid"]

        async def created(netuid=netuid):
            token = spoke.alpha_token(netuid)
            return token if int(token, 16) else None

        subnet["token"] = await wait_for(f"the token of subnet {netuid}", created)
        print(f"[rails] subnet {netuid}: token at {subnet['token']}")


def write_manifest(hub: dict, addresses: dict, subnets: list[dict]) -> None:
    manifest = {
        "hub": {
            "ws": HUB_WS,
            "rpcUrl": HUB_RPC,
            "chainId": HUB_CHAIN_ID,
            "gateway": hub["gateway"],
        },
        "spokes": {
            SPOKE_NAME: {
                "bridge": SPOKE_BRIDGE,
                "chainId": SPOKE_CHAIN_ID,
                "rpcUrl": SPOKE_RPC,
                "vault": addresses["vault"],
                "portal": addresses["portal"],
                "stable": addresses["stable"],
                "tao": addresses["tao"],
                "stableSymbol": "USDC",
                "stableDecimals": 6,
            }
        },
        "markets": subnets,
    }
    MANIFEST.write_text(json.dumps(manifest, indent=2) + "\n")

    # The swap page's configuration in local mode: see the page's config.ts.
    page = {
        "hub": {"name": "Bittensor localnet", "rpcUrl": HUB_RPC, "bridge": SPOKE_BRIDGE},
        "spoke": {
            "name": "Base localnet",
            "chainId": SPOKE_CHAIN_ID,
            "rpcUrl": SPOKE_RPC,
            "explorer": None,
            "nativeCurrency": {"name": "Ether", "symbol": "ETH", "decimals": 18},
        },
        "contracts": {key: addresses[key] for key in ("vault", "portal", "stable")},
        "markets": [subnet["netuid"] for subnet in subnets],
        "services": [
            {
                "netuid": subnet["netuid"],
                "name": subnet["symbol"],
                "account": ss58_to_pubkey(subnet["service"]),
            }
            for subnet in subnets
        ],
        "local": {"faucet": True, "testKey": USER_KEY},
    }
    PAGE_ENV.write_text(
        "# Written by scripts/rails/setup.py: the swap page against the local rig.\n"
        "NEXT_PUBLIC_RAILS_ENV=local\n"
        f"NEXT_PUBLIC_RAILS_LOCAL='{json.dumps(page, separators=(',', ':'))}'\n"
    )
    print(f"[rails] wrote {MANIFEST} and {PAGE_ENV}")


async def configure() -> None:
    alice = dev_wallet()
    hub = load_json(DEPLOYMENTS / "hub.local.json")
    addresses = load_json(DEPLOYMENTS / "spoke.local.json")
    spoke = Spoke(addresses)
    portal = "0x" + bytes(12).hex() + addresses["portal"][2:].lower()

    async with client() as chain:
        await submit(
            chain,
            bt.calls.Tusd.set_bridge(
                id=SPOKE_BRIDGE,
                route={
                    "origin": {"Evm": SPOKE_CHAIN_ID},
                    "domain": SPOKE_DOMAIN,
                    "portal": portal,
                    "asset": "Usdc",
                },
                mint_cap=10_000_000 * UNIT,
                window_limit=5_000_000 * UNIT,
                window_period=100,
            ),
            alice,
            root=True,
        )
        await submit(
            chain,
            bt.calls.Tusd.set_bridge_enabled(id=SPOKE_BRIDGE, enabled=True),
            alice,
            root=True,
        )
        await submit(
            chain, bt.calls.Rails.set_gateway(gateway=mirror(hub["gateway"])), alice, root=True
        )

        async def priced():
            return await chain.query(bt.storage.SubtensorModule.TaoUsdPrice)

        await wait_for("the oracle's first price", priced)
        subnets = await create_subnets(chain, alice)
        await open_pool(chain, spoke, alice)
        await open_markets(chain, spoke, alice, subnets)

        for subnet in subnets:
            listed = await chain.query(
                bt.storage.Rails.Tokens, [SPOKE_BRIDGE, {"Alpha": subnet["netuid"]}]
            )
            assert listed, f"subnet {subnet['netuid']} has no token on the hub"
            subnet["symbol"] = spoke.symbol(subnet["token"])

    write_manifest(hub, addresses, subnets)


if __name__ == "__main__":
    step = {"prepare": prepare, "configure": configure}.get(sys.argv[1] if sys.argv[1:] else "")
    if step is None:
        sys.exit("usage: setup.py prepare|configure")
    asyncio.run(step())
