# Lending v2.1 shorts: mainnet-clone testnet

A private clone of Bittensor mainnet (Finney) state with the `lending-v2-synthetic`
runtime (spec v2.1) applied. It is a throwaway chain. Balances are play money, and
nothing here touches the production chain. All times below are UTC.

## Chain

| | |
| --- | --- |
| Endpoint | `ws://3.21.143.83:9944` (public RPC, safe methods only) |
| Fork | mainnet block #9,605,237, scraped 2026-11-28 11:55 UTC (130 subnets plus root) |
| Runtime | `node-subtensor` spec_version **499**, applied by a sudo `set_code` at chain block #3 |
| Source | `RaoFoundation/subtensor`, branch `lending-v2-synthetic`, commit `282436d927c76757b54720a595f5e1a1f7c52881` |
| Block time | 12 s, as on mainnet |
| Lending | `Enabled = true`, `LongsEnabled = false` (longs are on hold; shorts only) |

The chain started with block #1 at about 11:59 UTC on 2026-11-28, so block numbers
are small. Subnet 58 was dissolved by the smoke test (see below); 129 subnets remain.

## Launch parameters (production-like clocks)

| Group | Values |
| --- | --- |
| Rate | `apr_bps 2000` (20%), `rate_k 1`, `rate_cap_bps 20000` (200%) |
| Rate formula | `rate = min(200%, 20% * max(1, u^k))`, `u` = debt / holdings at the slow price (integer floor) |
| Rate delay | `rate_delay_blocks 21600` (3 days). A tightening of the rate terms waits this long. A loosening is live next block |
| Charging | `charge_interval_blocks 360` (72 min). Interest is charged to the collateral |
| Minimums | `min_pool_tao` 1,000 TAO, `min_notional` 10 TAO (at the slow price), refundable `deposit` 0.1 TAO |
| Price | TWAP of 72 samples every 300 blocks, median; the interest base is `q * TWAP` |
| Short | `stress_bps 30000` (collateral plus proceeds must reach 3x the debt value), `open_limit_bps 100` (1% of the pool's alpha per block) |

Rules to know:

- A short opens or grows only on a subnet that is **in the emission set** now
  (`SubnetNotInEmission` otherwise). Closing and adding collateral are always allowed.
  At launch about 20 subnets were not in the set. Check with a quote (below).
- There is no collateral withdrawal. Collateral comes back when you close.
- One short per coldkey per subnet. Grow it with `increase-short`.
- When a subnet dissolves, its shorts are settled automatically at the dereg rate,
  the rest of the custody (deposit included) returns to the holder, and the settled
  TAO goes to the stakers.

## Getting test TAO (faucet)

Create a **fresh** test coldkey (never reuse a mainnet key or mnemonic) and send
its ss58 address to the operator. The faucet sends 1,000 TAO by default (more on
request):

```bash
btcli --yes wallet create -w mytest -H default --no-password
btcli -n ws://3.21.143.83:9944 wallet show -w mytest     # prints the coldkey ss58
```

The operator credits the address after you send it. Please do not use the
public `//Alice` key yourself: it is the chain's sudo and sole block author, and
parameter changes and dissolutions go through the operator so the clone stays
consistent.

## Install the SDK from the branch

Python 3.10 to 3.14 and [uv](https://docs.astral.sh/uv/):

```bash
uv venv && source .venv/bin/activate
uv pip install 'git+https://github.com/RaoFoundation/subtensor@lending-v2-synthetic#subdirectory=sdk/python'
# or pin the exact commit:
# uv pip install 'git+https://github.com/RaoFoundation/subtensor@282436d927c76757b54720a595f5e1a1f7c52881#subdirectory=sdk/python'
```

The install builds a Rust extension, so a Rust toolchain must be present and the
first build takes a few minutes. It provides `btcli` and `import bittensor as bt`.

## CLI examples

Point every command at the clone with `-n ws://3.21.143.83:9944` (or
`export BT_NETWORK=ws://3.21.143.83:9944`). On this clone, **add `--no-mev-shield` to
`tx` commands**: the CLI shields by default and MEV-shielded submission is not set up on the clone.

```bash
N="-n ws://3.21.143.83:9944"
btcli $N query lending-params                              # live groups and PendingLendingParams
btcli $N query lending-market --netuid 4                   # pool, books, spot / twap / sma
btcli $N query quote-open-short --netuid 4 --amount-alpha 2000
btcli $N -w mytest tx open-short --netuid 4 --amount-alpha 2000 \
        --collateral-tao 215 --dry-run --no-mev-shield     # preview, nothing sent
btcli $N -w mytest --yes tx open-short --netuid 4 --amount-alpha 2000 \
        --collateral-tao 215 --no-mev-shield
btcli $N query lending-position --coldkey <your ss58> --netuid 4 --side short
btcli $N query lending-positions --coldkey <your ss58>
btcli $N -w mytest --yes tx short-add-collateral --help    # add collateral
btcli $N -w mytest --yes tx increase-short --help          # grow the short
btcli $N -w mytest --yes tx close-short --netuid 4 --amount-alpha all --no-mev-shield
```

Sizing: `collateral-tao` must be at least the quote's `min_collateral`. The notional
(`amount * TWAP`) must reach 10 TAO, and one block may mint at most 1% of the
pool's alpha, so a small subnet needs a smaller amount. Quote errors name the rule
that failed.

## Python examples

```python
import bittensor as bt
from bittensor.wallet import Wallet

sub = bt.Subtensor(network="ws://3.21.143.83:9944")
wallet = Wallet(name="mytest", hotkey="default")
me = wallet.coldkeypub.ss58_address

print(sub.read("lending_params"))
print(sub.read("lending_market", netuid=4))
q = sub.read("quote_open_short", netuid=4, amount_alpha=2000.0)  # raises the chain error if it would fail
print(q["proceeds"], q["min_collateral"], q["coverage_need"])

r = sub.execute(bt.OpenShort(netuid=4, amount_alpha=2000.0,
                             collateral_tao=float(q["min_collateral"].tao) + 0.05,
                             rate_tolerance=0.05), wallet)
print(r.success, r.error)
print(sub.read("lending_position", coldkey_ss58=me, netuid=4, side="short"))

r = sub.execute(bt.CloseShort(netuid=4, amount_alpha="all", rate_tolerance=0.05), wallet)
```

`sub.plan(intent, wallet)` previews without sending. The SDK refuses an
under-collateralised open before it reaches the chain. To see the chain's own
`CoverageNotMet` or `SubnetNotInEmission`, submit `Lending.open_short(netuid, q,
collateral, min_tao_out)` with a raw extrinsic (amounts in rao).

## Reading the interest

Each charge emits `Lending.InterestCharged { who, netuid, side, to_pool, rate_bps }`.
Expect `rate_bps = 2000` while `u <= 1`, which a position at the minimum
collateral is (u is about 0.33). Interest per charge is
`q * TWAP * rate_bps * blocks / (10_000 * 7_200 * 365)` rao, where `blocks` is the
time since the last accrual.

## Smoke results at hand-over (2026-11-28, 12:00 to 12:19 UTC)

| Check | Result |
| --- | --- |
| Open and close a short, emission subnet 4 | passed, SDK and CLI. Opened at block #30 with 2,000 alpha and 209.906 TAO collateral, closed in full with a 0.10 TAO buy-back shortfall paid from collateral |
| Rate formula | `InterestCharged.rate_bps = 2000` at every charge; charged 79,781 rao over 10 blocks against 79,781 expected from `q * TWAP * 20% * 10 / year` |
| Coverage | raw open at 98% of `min_collateral` fails with `CoverageNotMet`; at the minimum it opens |
| Emission gate | raw open on subnet 2 (not in the emission set) fails with `SubnetNotInEmission` |
| Rate delay | raising `apr_bps` to 2500 gave `PendingLendingParams` with `effective_at` = call block + 21600; a follow-up call replaced it and the pending group cleared |
| Dissolution | root dissolve of subnet 58 with one short and one staker: the short settled (`ShortSettledOnDissolution`, paid 1.709 TAO = `q * r`), the holder got the rest of the custody back, the staker was paid, the cleanup queue emptied in 24 blocks |

Not exercised: the `u > 1` branch of the rate curve (the slow price needs hours to
move, and tightening the TWAP window is itself delayed by 3 days), and
`force_settle_short` on a stuck short (none occurred; it correctly rejects a short
that is not stuck with `ShortNotStuck`).

## Resets and caveats

- The chain can be rebuilt from a fresh mainnet scrape when the branch changes.
  Positions and balances do not survive a rebuild.
- Parameters are at their launch values above. Any change is announced here.
- The public RPC allows safe methods only.
