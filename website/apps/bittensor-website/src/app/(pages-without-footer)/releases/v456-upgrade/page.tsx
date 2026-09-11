import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V456 Upgrade — Basket Trading',
  description:
    'V456 adds swap_basket: a root validator can sell one holding of its beta basket and buy ' +
    'another, through a dedicated BasketTrading proxy. Every trade is boxed in by a 2% ' +
    'per-leg price band, a token-bucket turnover budget of 10% of NAV per day, a 10% ' +
    'liquidity cap per pool, the 1/16 concentration cap, and governance freeze switches. ' +
    'Trading launches gated off.',
  alternates: {canonical: '/releases/v456-upgrade'},
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
          <h1 className={styles.paper_title}>The V456 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Basket Trading · September 2026
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            Spec <strong>456</strong> lets a root validator actively trade its beta basket.
            Until now a fund&apos;s composition changed only through the dividend stream:{' '}
            <code>set_root_weights</code> decides where new yield is deployed, but existing
            holdings stay where they are. The new{' '}
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
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>The operating model</h2>
          <p>
            The intended setup is a validator coldkey that grants a <code>BasketTrading</code>{' '}
            proxy (new <code>ProxyType</code>, index 18) to a trader account, usually a
            multisig. That proxy type admits exactly one call: <code>swap_basket</code>. It
            cannot stake, unstake, transfer, set weights, or change keys.
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

# the desk: sell 250 α of netuid 8 and buy netuid 64 in the validator's fund
btcli root trade --from 8 --to 64 --amount 250 --hotkey <validator hotkey> \\
  -w desk --proxy-for <validator coldkey>

# move part of the fund's netuid 3 position into cash (netuid 0)
btcli root trade --from 3 --to 0 --amount 1200 --hotkey <validator hotkey> \\
  -w desk --proxy-for <validator coldkey>`}
          </pre>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Guardrails</h2>
          <p>
            A trading key is a new way for a fund to lose value, so every trade must pass six
            checks. Two are price rules, one is a budget, two are shape rules, and the last is
            a set of switches. All numbers below are the launch defaults; the caps and budget
            are hyperparameters governance can move.
          </p>
          <ul className={styles.list}>
            <li>
              <strong>Per-leg price band: 2%.</strong> Each AMM leg must fill{' '}
              <em>completely</em> within 2% of the stricter of two references: the
              subnet&apos;s moving (EMA) price and its spot price. A buy may not fill above{' '}
              <code>1.02 × min(EMA, spot)</code>; a sell may not fill below{' '}
              <code>0.98 × max(EMA, spot)</code>. The EMA anchor defeats a pre-trade pump
              or dump; the spot anchor caps the trade&apos;s own price impact. Any miss is{' '}
              <code>SlippageTooHigh</code>, and the whole trade rolls back.
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
              realizable value may not end above <code>RootWeightsCap</code> of fund NAV, the
              same 1/16 rule and young-chain softening as <code>set_root_weights</code>.
              Selling out of an over-cap position is always allowed. Refusal is{' '}
              <code>RootWeightCapExceeded</code>.
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
              The 2% band is per leg, not per block or per day. With spot below the EMA,
              several legs in one block can walk a subnet&apos;s price up to{' '}
              <code>1.02 × EMA</code>. The EMA itself moves slowly: on mainnet its half-life
              is about eight hours.
            </li>
            <li>
              Because the band is anchored to the EMA, a fund cannot sell a holding whose spot
              has fallen more than 2% below the moving price, or buy one that has risen more
              than 2% above it, until the average catches up. After a 5% drop a sell is
              refused for roughly ten hours. Stop-losses are not possible by design.
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
                <code>SubtensorModule::swap_basket(hotkey, origin_netuid, destination_netuid, amount)</code>
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
              . <code>SlippageTooHigh</code> and <code>RootWeightCapExceeded</code> are reused
              for the band and the concentration cap.
            </li>
            <li>
              <strong>Storage</strong> <code>BasketTradingEnabled</code> (bool, default off),{' '}
              <code>BasketTradingFrozen</code> (per hotkey),{' '}
              <code>BasketDailyTurnoverCap</code> (u16, default 6553 = 10%),{' '}
              <code>BasketLiquidityCap</code> (u16, default 6553 = 10%), and{' '}
              <code>BasketTradeBucket</code> (per hotkey:{' '}
              <code>(tao_available, last_refill_block)</code>; a missing row is a full bucket).
            </li>
            <li>
              <strong>Admin setters</strong> (root-only, in <code>AdminUtils</code>):{' '}
              <code>sudo_set_basket_trading_enabled(enabled)</code> (106),{' '}
              <code>sudo_set_basket_trading_frozen(hotkey, frozen)</code> (107),{' '}
              <code>sudo_set_basket_daily_turnover_cap(cap)</code> (108), and{' '}
              <code>sudo_set_basket_liquidity_cap(cap)</code> (109), with events{' '}
              <code>BasketTradingToggled</code>, <code>BasketTradingFrozenSet</code>,{' '}
              <code>BasketDailyTurnoverCapSet</code>, and <code>BasketLiquidityCapSet</code>.
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
              <strong>Weights.</strong> <code>swap_basket</code> and the four setters have
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
            <code>BasketTrading</code> proxy type, and descriptions for every new error.{' '}
            <code>btcli root trade --from --to --amount [--hotkey] [--proxy-for]</code> is the
            CLI surface; its review card shows the origin and destination holdings and how much
            of the bucket is left before you sign.
          </p>
          <pre className={styles.code_block}>
            {`import bittensor as bt
from bittensor.wallet import Wallet

desk = Wallet(name="desk")

async with bt.Subtensor("finney") as client:
    status = await client.read("basket_trading_status", hotkey_ss58=HOTKEY)
    # {'enabled': True, 'frozen': False, 'budget_tao': τ1,250, 'remaining_tao': τ980, ...}

    intent = bt.SwapBasket(hotkey_ss58=HOTKEY, origin_netuid=8, dest_netuid=64, amount=250)
    result = await client.execute(intent, desk, proxy_for=VALIDATOR_COLDKEY)
    if not result.success:
        print(result.error.code, result.error.remediation)`}
          </pre>
          <p>
            Upgrade with <code>pip install -U bittensor</code>. Reference pages:{' '}
            <DocLink href='/docs/tx/swap-basket'>
              <code>swap-basket</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/query/basket-trading-status'>
              <code>basket-trading-status</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/hyperparameters'>hyperparameters</DocLink>. Guides:{' '}
            <DocLink href='/docs/guides/root-reborn'>Root Reborn</DocLink>,{' '}
            <DocLink href='/docs/guides/proxies'>Proxies</DocLink>.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
