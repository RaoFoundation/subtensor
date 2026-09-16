import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V461 Upgrade — Basket Trading',
  description:
    'V461 adds swap_basket: a root validator can sell one holding of its beta basket and buy ' +
    'another, through a dedicated BasketTrading proxy. Every trade is boxed in by a 2% ' +
    'per-leg price band, a token-bucket turnover budget of 10% of NAV per day, a 10% ' +
    'liquidity cap per pool, the 1/16 concentration cap, and governance freeze switches. ' +
    'Trading launches gated off.',
  alternates: {canonical: '/releases/v461-upgrade'},
};

const DocLink = ({href, children}: {href: string; children: React.ReactNode}) => (
  <Link href={href} className={styles.inline_link}>
    {children}
  </Link>
);

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V461 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Basket Trading · September 2026
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            Spec <strong>461</strong> lets a root validator actively trade its beta basket, and
            makes trading the <em>only</em> way a fund&apos;s composition changes. Until now a
            fund could also be steered through the dividend stream: a{' '}
            <code>set_root_weights</code> vector decided where new yield was deployed. That
            design is removed — dividends now always accumulate in place on the subnet they
            were earned on, direct deposits mirror the fund&apos;s current holdings, and the
            new{' '}
            <DocLink href='/docs/tx/swap-basket'>
              <code>swap_basket</code>
            </DocLink>{' '}
            call sells part of one holding for TAO and buys another with it. Netuid 0 on either
            side is the fund&apos;s TAO cash slot, so a validator can also move into or out of
            cash.
          </p>
          <p>
            Fund shares (β) and every staker&apos;s entitlement are untouched by a trade. Only
            what the fund holds changes, and with it the fund&apos;s NAV over time. Stakers keep
            choosing a validator; validators now also choose how to manage the basket between
            dividends.
          </p>
          <p>
            <strong>Trading launches gated off network-wide.</strong> After the upgrade{' '}
            <code>swap_basket</code> fails with <code>BasketTradingDisabled</code> until
            governance flips <code>BasketTradingEnabled</code> on. Everything else in this
            release (proxy type, budgets, caps, reads, btcli) is live from the upgrade block.
            The operator walkthrough is the{' '}
            <DocLink href='/docs/guides/basket-trading'>Basket trading guide</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>The operating model</h2>
          <p>
            The intended setup is a validator coldkey that grants a <code>BasketTrading</code>{' '}
            proxy (new <code>ProxyType</code>, index 18) to a trader account, usually a
            multisig. That proxy type admits exactly one call: <code>swap_basket</code>. It
            cannot stake, unstake, transfer, claim, or change keys.
          </p>
          <p>
            The grant is opt-in in both directions. No existing proxy gains trading rights at
            the upgrade: <code>NonTransfer</code>, <code>NonCritical</code>,{' '}
            <code>Staking</code>, and <code>NonFungible</code> delegates are all refused{' '}
            <code>swap_basket</code>. Only <code>Any</code> and <code>BasketTrading</code>{' '}
            admit it. Trades are submitted through the MEV shield by default (the SDK intent
            sets <code>mev_shield_default = True</code>), so a pending trade does not
            advertise its legs to the block&apos;s other traders.
          </p>
          <pre className={styles.code_block}>
            {`# validator coldkey: delegate trading to the desk multisig
btcli proxy add --delegate <desk multisig> --proxy-type BasketTrading -w validator_cold

# the desk: sell 250 α of netuid 8 and buy netuid 64 in the validator's fund,
# insisting on at least 99% of the quoted fill (--max-slippage defaults to 1)
btcli root swap --from 8 --to 64 --amount 250 --hotkey <validator hotkey> \\
  --max-slippage 1 -w desk --proxy-for <validator coldkey>

# move part of the fund's netuid 3 position into cash (netuid 0)
btcli root swap --from 3 --to 0 --amount 1200 --hotkey <validator hotkey> \\
  -w desk --proxy-for <validator coldkey>`}
          </pre>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Guardrails</h2>
          <p>
            A trading key is a new way for a fund to lose value, so every trade must pass six
            checks. Two are price rules, one is a budget, two are shape rules, and the last is
            a set of switches. All numbers below are the launch defaults; the caps and budget
            are hyperparameters governance can move. On top of them the caller sets its own
            floor on the fill.
          </p>
          <ul className={styles.list}>
            <li>
              <strong>Per-leg price band: 2%.</strong> Each AMM leg must fill{' '}
              <em>completely</em> within 2% of the strictest of three references: the
              subnet&apos;s slow moving price (the monthly emission EMA), its new fast moving
              price (<code>SubnetFastMovingPrice</code>, a two-hour-half-life EMA of spot
              written each block from the previous block&apos;s close), and its spot price.
              A buy may not fill above <code>1.02 × min(slow, fast, spot)</code>; a sell may
              not fill below <code>0.98 × max(slow, fast, spot)</code>. The fast anchor is a
              price nobody can move inside a block, so a same-block pump or dump cannot make
              the fund fill at the manipulated price; the slow anchor caps how far a held pump
              can carry it; the spot anchor caps the trade&apos;s own price impact. Any miss is{' '}
              <code>SlippageTooHigh</code>, and the whole trade rolls back.
            </li>
            <li>
              <strong>Caller floor: <code>min_amount_out</code>.</strong> The last argument
              of <code>swap_basket</code> is the least the buy leg must credit to the
              destination holding, after fees, in the destination subnet&apos;s alpha (TAO when
              the destination is netuid 0). Below it the trade fails with{' '}
              <code>BasketMinOutNotMet</code> and rolls back. <code>0</code> sets no floor.
              This is the caller&apos;s protection against a fill worse than the quote it signed
              on; the band is the protocol&apos;s protection of the fund, is a price rule per
              leg, and applies regardless. <code>btcli root swap</code> derives the floor from a
              quote and <code>--max-slippage</code> (percent, default 1).
            </li>
            <li>
              <strong>Turnover budget: a token bucket of 10% of NAV.</strong> Every fund has a
              bucket whose capacity is <code>BasketDailyTurnoverCap</code> of its current NAV
              (default <code>u16::MAX / 10</code>, 10%). The TAO that passes through the
              middle of each trade is taken out of the bucket; the bucket refills
              continuously at <code>capacity / 7200</code> per block, so an empty bucket is
              full again after one day (7200 blocks at 12 s). A fund that has never traded
              starts full. The level is clamped to one capacity, so{' '}
              <strong>at most one budget can be spent at any instant</strong> and about one per
              day sustained. This replaces the fixed daily window of the first draft, which let
              two full budgets through in two adjacent blocks at the window edge. Refusal is{' '}
              <code>BasketTurnoverBudgetExceeded</code>.
            </li>
            <li>
              <strong>Liquidity cap: 10% of the destination pool&apos;s alpha reserve.</strong>{' '}
              After the buy leg, the fund may not hold more than{' '}
              <code>BasketLiquidityCap</code> (default <code>u16::MAX / 10</code>, 10%) of the
              destination subnet&apos;s alpha reserve (<code>SubnetAlphaIn</code>). Root has
              no pool and is exempt; selling is never capped. This is the rule that bounds a
              thin-pool drain (below). Refusal is <code>BasketLiquidityCapExceeded</code>.
            </li>
            <li>
              <strong>Concentration cap: 1/16 of NAV.</strong> The destination holding&apos;s
              realizable value may not end above <code>BasketConcentrationCap</code> of fund
              NAV — the 1/16 rule (and young-chain softening) that used to bound{' '}
              <code>set_root_weights</code> vectors, carried over as a pure trade guardrail.
              Selling out of an over-cap position is always allowed. Refusal is{' '}
              <code>BasketConcentrationCapExceeded</code>.
            </li>
            <li>
              <strong>Switches.</strong> <code>BasketTradingEnabled</code> is the network-wide
              gate (default off). <code>BasketTradingFrozen[hotkey]</code> lets governance
              freeze one fund&apos;s trading, for example after a suspected key compromise; a
              frozen fund still accepts deposits and pays claims. Both follow the fund through
              a hotkey swap, as does the bucket. A trade is also refused while the beta-basket
              seed migration runs or a coldkey swap is announced for the signer.
            </li>
            <li>
              <strong>Ownership.</strong> The signer must be the coldkey that owns the hotkey
              (or its <code>BasketTrading</code> proxy), the hotkey must be registered on root,
              both subnets must exist, and the destination&apos;s subtoken must be enabled.
            </li>
          </ul>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What a rogue key can cost the fund</h2>
          <p>
            The question these rules answer is: if the trading key is stolen and the thief
            colludes with a counterparty, how much of the fund can they extract before
            governance freezes the hotkey? The original proposal stated the bound as{' '}
            <code>cap × (2 × 2% + fees) ≈ 0.43% of NAV per day</code>. That figure counted
            only the price band and was too low. The calibration pass on the first draft
            reproduced a loss of 8.3% of NAV in one window, with every guardrail passing on
            every trade, by buying a thin subnet in 2%-band slices while a counterparty sold
            alpha back to the EMA between slices. The concentration cap could not see it,
            because it marks holdings at <em>realizable</em> value, which is bounded by the
            pool&apos;s TAO reserve and so never grows no matter how much of the pool&apos;s
            supply the fund accumulates.
          </p>
          <p>
            The liquidity cap closes that path. With cap <code>L</code> on a pool with TAO
            reserve <code>R</code>, the most a drain can extract from that pool is about{' '}
            <code>R × L² / (1 + L)</code>: at the 10% default, about 0.9% of the pool&apos;s TAO
            reserve, or about 9% of the TAO the fund spent buying into it. The realizable
            haircut on any holding is capped at about <code>L / (1 + L)</code> ≈ 9%. On a
            1,000 τ pool that is roughly 10 τ; before the cap it was the whole turnover
            budget.
          </p>
          <p>
            Put together, the adversarial bound per unit of turnover is about{' '}
            <strong>9% (liquidity drain) + 4% (two legs at the edge of the band) + fees</strong>
            , roughly 13% of the TAO traded. Because the bucket holds one budget at a time,
            the most that can be moved through the fund at any instant is 10% of NAV, so the
            worst case is about <strong>1.3% of NAV in a burst and about 1.3% of NAV per day
            sustained</strong> until the fund is frozen, spread across as many pools as the
            budget and the 1/16 cap allow. Honest trading pays only AMM fees and the price
            impact of its own legs. These are upper bounds for a stolen key and a willing
            counterparty, not expected costs.
          </p>
          <p>
            That 1.3% figure assumed the band&apos;s moving-price anchor sits near spot. The
            security review showed it did not have to: the only smoothed reference was the
            monthly emission EMA, which sits stale after any real move, and the band
            re-anchored to spot on every leg. A key holder could lift spot toward a stale-high
            EMA and have the fund chain-buy near it, or dump toward a stale-low EMA and have
            it chain-sell — 1.1% / 4.1% / 6.9% of NAV per day on the buy leg and 1.1% / 3.1% /
            4.7% on the sell leg at EMA-to-spot ratios of 1.35, 2 and 4, all inside one block.
            The fast moving price closes this: replaying those sequences against the fixed
            band, the attacker&apos;s profit is at most rounding on either leg and the fund
            fills one in-band leg at the pre-move price. Regaining the old extraction now
            means holding a 30–100%-of-reserve pump against arbitrage for about ten hours, at
            which point the 1.3%-per-day bound above applies — at that capital-time cost,
            not for free.
          </p>
          <p>
            The same review found the concentration cap&apos;s and the turnover budget&apos;s
            NAV denominator pumpable: a same-block pump of a thin pool the fund holds marked
            that holding at roughly the pump size, so a 1,500 τ buy the cap had refused
            became admitted, and a 9,500 τ buy the budget had refused too. Both are now
            measured against a <strong>guarded NAV</strong> that marks every holding at the
            lower of its realizable value and its alpha times the slow moving price, which
            cannot be moved inside a block; the same buys stay refused under the pump.
          </p>
          <p>
            For today&apos;s largest funds (about 12,500 τ) the liquidity cap barely binds: 10%
            of a median mainnet pool is about 700 τ, which is about the fund&apos;s 1/16 slice.
            It binds correctly as funds grow. Raise <code>BasketLiquidityCap</code> if funds
            legitimately need larger positions in mid-depth pools; lower it to tighten the
            per-pool value at risk.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Edges to know about</h2>
          <ul className={styles.list}>
            <li>
              The 2% band is per leg, not per block or per day, but every leg is bound to
              the fast moving price as well, so several legs in one block cannot walk a
              subnet&apos;s price more than about 2% from where it opened the block. The
              slow EMA moves slowly (a half-life of about a month on mainnet); the fast one
              has a two-hour half-life.
            </li>
            <li>
              Because the band is anchored to the moving prices, a fund cannot sell a holding
              whose spot has fallen more than 2% below either average, or buy one that has
              risen more than 2% above either, until the averages catch up. After a real 5%
              move the fast anchor is back in band in about four hours; the slow anchor can
              hold the direction shut for weeks. Stop-losses are not possible by design.
            </li>
            <li>
              A subnet with no moving price yet (before <code>start_call</code>, or newly
              started) cannot be traded until its EMA has warmed up. The EMA is{' '}
              <code>min(price, 1)</code>, so a subnet trading above 1 τ per α cannot be bought
              within the band either.
            </li>
            <li>
              The cash slot is a capped destination too: moving more than 1/16 of NAV into
              netuid 0 in one trade is refused by the concentration cap.
            </li>
          </ul>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What changed on chain</h2>
          <ul className={styles.list}>
            <li>
              <strong>Call</strong>{' '}
              <DocLink href='/docs/tx/swap-basket'>
                <code>
                  SubtensorModule::swap_basket(hotkey, origin_netuid, destination_netuid,
                  amount, min_amount_out)
                </code>
              </DocLink>{' '}
              (call index 150). Declared weight is a cap sized for 256 holdings plus any
              pending-deposit flush work; the actual weight is computed from the real holding
              count and refunded post-dispatch.
            </li>
            <li>
              <strong>Event</strong> <code>BasketSwapped {'{'} hotkey, origin_netuid,
              destination_netuid, alpha_sold, tao_mid, alpha_bought {'}'}</code>, appended at
              the tail of the event enum (index 149) so existing event indices are unchanged.
            </li>
            <li>
              <strong>Errors</strong>{' '}
              <DocLink href='/docs/errors/chain/BasketTradingDisabled'>
                <code>BasketTradingDisabled</code>
              </DocLink>
              ,{' '}
              <DocLink href='/docs/errors/chain/BasketTradingFrozen'>
                <code>BasketTradingFrozen</code>
              </DocLink>
              ,{' '}
              <DocLink href='/docs/errors/chain/BasketTurnoverBudgetExceeded'>
                <code>BasketTurnoverBudgetExceeded</code>
              </DocLink>
              ,{' '}
              <DocLink href='/docs/errors/chain/BasketSameSubnet'>
                <code>BasketSameSubnet</code>
              </DocLink>
              ,{' '}
              <DocLink href='/docs/errors/chain/BasketLiquidityCapExceeded'>
                <code>BasketLiquidityCapExceeded</code>
              </DocLink>
              ,{' '}
              <DocLink href='/docs/errors/chain/BasketMinOutNotMet'>
                <code>BasketMinOutNotMet</code>
              </DocLink>
              , and{' '}
              <DocLink href='/docs/errors/chain/BasketConcentrationCapExceeded'>
                <code>BasketConcentrationCapExceeded</code>
              </DocLink>
              . <code>SlippageTooHigh</code> is reused for the band.
            </li>
            <li>
              <strong>Storage</strong> <code>BasketTradingEnabled</code> (bool, default off),{' '}
              <code>BasketTradingFrozen</code> (per hotkey),{' '}
              <code>BasketDailyTurnoverCap</code> (u16, default 6553 = 10%),{' '}
              <code>BasketLiquidityCap</code> (u16, default 6553 = 10%),{' '}
              <code>BasketConcentrationCap</code> (u16, default 4096 = 1/16; the value
              governance had set in <code>RootWeightsCap</code> is carried over), and{' '}
              <code>BasketTradeBucket</code> (per hotkey:{' '}
              <code>(tao_available, last_refill_block)</code>; a missing row is a full bucket).
            </li>
            <li>
              <strong>Removed.</strong> The <code>set_root_weights</code> extrinsic (call 146),
              its <code>RootWeightSettingEnabled</code> gate and{' '}
              <code>sudo_set_root_weight_setting_enabled</code> setter, the{' '}
              <code>RootWeightsCap</code> map, the <code>validator_root_weights</code> read /{' '}
              <code>get_validator_weights</code> runtime API, and the <code>weights</code> field
              of <code>BasketSummary</code>. A migration clears every stored root vector; no
              fund&apos;s holdings change. Weights only ever decided how an earned dividend was
              deployed, never what a validator earned, so no validator&apos;s income moves.
            </li>
            <li>
              <strong>Admin setters</strong> (root-only, in <code>AdminUtils</code>):{' '}
              <code>sudo_set_basket_trading_enabled(enabled)</code> (106),{' '}
              <code>sudo_set_basket_trading_frozen(hotkey, frozen)</code> (107),{' '}
              <code>sudo_set_basket_daily_turnover_cap(cap)</code> (108),{' '}
              <code>sudo_set_basket_liquidity_cap(cap)</code> (109), and{' '}
              <code>sudo_set_basket_concentration_cap(cap)</code> (105, replacing{' '}
              <code>sudo_set_root_weights_cap</code>), with events{' '}
              <code>BasketTradingToggled</code>, <code>BasketTradingFrozenSet</code>,{' '}
              <code>BasketDailyTurnoverCapSet</code>, <code>BasketLiquidityCapSet</code>, and{' '}
              <code>BasketConcentrationCapSet</code>.
              A zero cap is rejected with <code>ValueNotInBounds</code>.
            </li>
            <li>
              <strong>Proxy</strong> <code>ProxyType::BasketTrading</code> (index 18), whose
              filter admits only <code>swap_basket</code>. The broad proxy types do not gain
              the call.
            </li>
            <li>
              <strong>Runtime API</strong> <code>BetaBasketRuntimeApi</code> v4 adds{' '}
              <code>get_basket_trading_status(hotkey)</code> returning{' '}
              <code>{'{'} enabled, frozen, refill_blocks, tao_available, budget_tao {'}'}</code>
              : the gates, the bucket&apos;s refill period, what a trade at this block could
              push through, and the bucket&apos;s capacity at current NAV.
            </li>
            <li>
              <strong>Weights.</strong> <code>swap_basket</code> and the five setters have
              their own benchmarks and <code>WeightInfo</code> entries, measured on the
              reference benchmarking hardware.
            </li>
          </ul>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>SDK and btcli</h2>
          <p>
            The SDK release that ships with the runtime adds the <code>SwapBasket</code>{' '}
            intent (<code>amount=&quot;all&quot;</code> sells the whole origin holding), the{' '}
            <DocLink href='/docs/query/basket-trading-status'>
              <code>basket_trading_status</code>
            </DocLink>{' '}
            read (<code>enabled</code>, <code>frozen</code>, <code>budget_tao</code>,{' '}
            <code>remaining_tao</code>, <code>used_tao</code>,{' '}
            <code>refill_per_block_tao</code>, <code>refill_blocks</code>), the{' '}
            <code>BasketTrading</code> proxy type, and descriptions for every new error. The
            intent takes <code>min_amount_out</code> (destination alpha, or TAO for netuid 0;
            default 0, no floor).{' '}
            <code>
              btcli root swap --from --to --amount [--max-slippage] [--hotkey] [--proxy-for]
            </code>{' '}
            is the CLI surface; it quotes both legs, sets the floor to the quote less{' '}
            <code>--max-slippage</code> percent (default 1), and its review card shows the
            origin and destination holdings, the expected and minimum output, and how much of
            the bucket is left before you sign. If the quote is unavailable the floor is 0 and
            the card says so.
          </p>
          <pre className={styles.code_block}>
            {`import bittensor as bt
from bittensor.wallet import Wallet

desk = Wallet(name="desk")

async with bt.Subtensor("finney") as client:
    status = await client.read("basket_trading_status", hotkey_ss58=HOTKEY)
    # {'enabled': True, 'frozen': False, 'budget_tao': τ1,250, 'remaining_tao': τ980, ...}

    tao_mid = (await client.read("quote_unstake", netuid=8, amount_alpha=250)).tao
    expected = (await client.read("quote_stake", netuid=64, amount_tao=tao_mid.tao)).alpha
    floor = bt.Balance.from_rao(expected.rao * 99 // 100, 64)   # 1% under the quote

    intent = bt.SwapBasket(
        hotkey_ss58=HOTKEY, origin_netuid=8, dest_netuid=64, amount=250, min_amount_out=floor
    )
    result = await client.execute(intent, desk, proxy_for=VALIDATOR_COLDKEY)
    if not result.success:
        print(result.error.code, result.error.remediation)`}
          </pre>
          <p>
            This runtime is live on testnet and not yet on mainnet, so the stable{' '}
            <code>bittensor</code> release on PyPI does not have{' '}
            <code>btcli root swap</code>. The first testnet release candidate,{' '}
            <code>11.3.0rc46</code>, shipped a broken <code>btcli</code> (no{' '}
            <code>root swap</code> command and a crash at exit); <code>11.3.0rc47</code>{' '}
            fixes both. For the testnet trial install{' '}
            <code>pip install &quot;bittensor==11.3.0rc47&quot;</code> and pass{' '}
            <code>-n test</code>. Once the runtime reaches mainnet, upgrade to the stable
            release that ships with it (<code>11.3.0</code> or later). Reference pages:{' '}
            <DocLink href='/docs/tx/swap-basket'>
              <code>swap-basket</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/query/basket-trading-status'>
              <code>basket-trading-status</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/hyperparameters'>hyperparameters</DocLink>. Guides:{' '}
            <DocLink href='/docs/guides/basket-trading'>Basket trading</DocLink> (the
            operator walkthrough for this release),{' '}
            <DocLink href='/docs/guides/basket-trading-governance'>
              Basket trading for governance
            </DocLink>{' '}
            (enable, freeze, tune, and the rogue-key playbook),{' '}
            <DocLink href='/docs/guides/basket-trading-for-stakers'>
              Basket trading for stakers
            </DocLink>{' '}
            (nothing to do, and how to watch),{' '}
            <DocLink href='/docs/guides/root-reborn'>Root Reborn</DocLink>,{' '}
            <DocLink href='/docs/guides/proxies'>Proxies</DocLink>.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
