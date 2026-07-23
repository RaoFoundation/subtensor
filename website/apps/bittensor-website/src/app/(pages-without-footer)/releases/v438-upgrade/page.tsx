import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'The V438 Upgrade — Root Reborn',
  description:
    'Root dividends become beta baskets: every root validator runs an escrowed index fund of ' +
    'subnet alpha, curated by its root weights, and stakers redeem fund shares with one ' +
    'parameterless claim.',
  alternates: {canonical: '/releases/v438-upgrade'},
};

const DocLink = ({href, children}: {href: string; children: React.ReactNode}) => (
  <Link href={href} className={styles.inline_link}>
    {children}
  </Link>
);

const GRAPH_TEXT = {
  fontFamily: 'FiraCode',
  fontSize: 10,
  fill: 'rgb(41, 41, 41)',
} as const;

const INK = 'rgb(41, 41, 41)';

// Pointy-top hexagon centered at (cx, cy) with circumradius r.
const hexPoints = (cx: number, cy: number, r: number) => {
  const dx = r * 0.866;
  return [
    [cx, cy - r],
    [cx + dx, cy - r / 2],
    [cx + dx, cy + r / 2],
    [cx, cy + r],
    [cx - dx, cy + r / 2],
    [cx - dx, cy - r / 2],
  ]
    .map(([x, y]) => `${x},${y}`)
    .join(' ');
};

const BasketFlowDiagram = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 340'
    role='img'
    aria-label='Each epoch a validator’s root alpha dividend is sold to TAO and split across subnets per its root weights, buying alpha into an escrowed basket; root stakers are minted fund shares at NAV and later redeem them pro-rata as TAO staked back to root.'
  >
    {/* Escrowed basket region */}
    <rect
      x='300'
      y='50'
      width='240'
      height='240'
      fill='rgba(41, 41, 41, 0.03)'
      stroke='rgba(41, 41, 41, 0.4)'
      strokeWidth='1'
      strokeDasharray='4 4'
    />
    <text {...GRAPH_TEXT} x='420' y='68' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      BETA BASKET · ESCROWED
    </text>

    {/* Dividend input node */}
    <polygon points={hexPoints(120, 130, 46)} fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='120' y='127' textAnchor='middle'>
      ROOT
    </text>
    <text {...GRAPH_TEXT} x='120' y='141' textAnchor='middle'>
      DIVIDEND α
    </text>
    <text {...GRAPH_TEXT} x='120' y='196' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      EVERY EPOCH
    </text>

    {/* Sold to TAO, split per weights */}
    <line x1='166' y1='130' x2='294' y2='130' stroke={INK} strokeWidth='1.5' />
    <polygon points='294,130 286,126 286,134' fill={INK} />
    <text {...GRAPH_TEXT} x='230' y='118' textAnchor='middle'>
      SOLD TO TAO
    </text>
    <text {...GRAPH_TEXT} x='230' y='146' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      SPLIT PER ROOT WEIGHTS
    </text>

    {/* Holdings inside the basket */}
    <rect x='330' y='92' width='180' height='30' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='340' y='111'>
      SN 4 · α HOLDING
    </text>
    <text {...GRAPH_TEXT} x='500' y='111' textAnchor='end'>
      30%
    </text>

    <rect x='330' y='140' width='180' height='30' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='340' y='159'>
      SN 8 · α HOLDING
    </text>
    <text {...GRAPH_TEXT} x='500' y='159' textAnchor='end'>
      50%
    </text>

    <rect x='330' y='188' width='180' height='30' fill='none' stroke={INK} strokeWidth='1.5' />
    <rect x='334' y='192' width='8' height='8' fill='#e0a53f' />
    <text {...GRAPH_TEXT} x='350' y='207'>
      NETUID 0 · TAO SLOT
    </text>
    <text {...GRAPH_TEXT} x='500' y='207' textAnchor='end'>
      20%
    </text>

    <text {...GRAPH_TEXT} x='420' y='252' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      NAV = REALIZABLE TAO QUOTE
    </text>
    <text {...GRAPH_TEXT} x='420' y='266' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      NOT A SPOT MARK
    </text>

    {/* Shares minted to stakers */}
    <line x1='546' y1='130' x2='624' y2='130' stroke={INK} strokeWidth='1.5' />
    <polygon points='624,130 616,126 616,134' fill={INK} />
    <text {...GRAPH_TEXT} x='585' y='118' textAnchor='middle'>
      SHARES
    </text>
    <text {...GRAPH_TEXT} x='585' y='146' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      MINTED AT NAV
    </text>

    {/* Staker node */}
    <polygon points={hexPoints(672, 130, 40)} fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='672' y='127' textAnchor='middle'>
      ROOT
    </text>
    <text {...GRAPH_TEXT} x='672' y='141' textAnchor='middle'>
      STAKERS
    </text>

    {/* Claim path back out */}
    <line x1='672' y1='172' x2='672' y2='226' stroke={INK} strokeWidth='1.5' />
    <line x1='672' y1='226' x2='548' y2='226' stroke={INK} strokeWidth='1.5' />
    <polygon points='548,226 556,222 556,230' fill={INK} />
    <text {...GRAPH_TEXT} x='680' y='200'>
      CLAIM
    </text>
    <text {...GRAPH_TEXT} x='614' y='214' textAnchor='middle'>
      PRO-RATA
    </text>
    <text {...GRAPH_TEXT} x='614' y='242' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      SOLD TO TAO → ROOT STAKE
    </text>

    {/* check glyph on claim */}
    <path
      d='M 700 220 l 6 6 l 10 -12'
      fill='none'
      stroke='#5a8f5a'
      strokeWidth='2'
      strokeLinecap='round'
      strokeLinejoin='round'
    />
  </svg>
);

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The V438 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Root Reborn · July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Spec <strong>438</strong> is the next mainnet runtime after{' '}
            <DocLink href='/releases/v436-upgrade'>v437</DocLink>. It rebuilds how root stake
            earns: per-subnet claimable dividends are replaced by{' '}
            <DocLink href='/docs/guides/root-dividends'>beta baskets</DocLink> — every root
            validator runs an escrowed index fund of subnet alpha, curated by a new{' '}
            <strong>root weights</strong> vector, and root stakers accrue shares of that fund
            which they redeem with one parameterless claim. Root validators become fund
            managers: allocation quality shows up directly in what their stakers earn, so
            capital can flow to the validators who deploy it best.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Beta baskets</p>
          <p>
            Each epoch, a validator&apos;s root alpha dividend is sold to TAO and redeployed
            across the subnets the validator has chosen, in proportion to its root weights —
            buying each destination&apos;s alpha into the validator&apos;s basket. Holdings
            live under a chain-owned escrow account as real stake positions, so they keep
            earning like any other stake. When a dividend lands, stakers are minted basket
            shares in proportion to their root stake, priced at the fund&apos;s current net
            asset value — exactly like buying into an index fund. A weight on{' '}
            <strong>netuid 0</strong> holds that slice as TAO instead of subnet alpha, letting
            a validator keep part of the fund out of subnet exposure.
          </p>
          <BasketFlowDiagram />
          <p className={styles.graph_caption}>
            One validator&apos;s fund with a 30/50/20 vector. Dividends buy holdings; stakers
            hold shares; a claim redeems the staker&apos;s owed shares as a pro-rata slice of
            every holding, sold to TAO and staked back to root on the same validator.
          </p>
          <p>
            Valuation is deliberately conservative: the basket is marked at its{' '}
            <strong>realizable</strong> TAO quote — what selling the holdings would actually
            fetch at current pool depth, net of fees — never at spot price. A thin pool
            cannot inflate a fund&apos;s book value, and what{' '}
            <DocLink href='/docs/query/root-basket-owed'>
              <code>root-basket-owed</code>
            </DocLink>{' '}
            reports is what a claim would actually pay.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>For root validators</p>
          <p>
            Set your distribution vector with your hotkey. This is the one action the release
            requires of you: <strong>a validator with no root weights set has its root
            dividends recycled</strong> — set the vector to start accruing a basket for your
            stakers.
          </p>
          <pre className={styles.code_block}>
            {`btcli weights set-root --weights "0:0.2,4:0.3,8:0.5" -w my_wallet
btcli weights get-root --hotkey 5F...
btcli stake basket --hotkey 5F...      # your fund: holdings + NAV`}
          </pre>
          <p>
            Weights are relative and normalized before submission (
            <DocLink href='/docs/tx/set-root-weights'>
              <code>set-root-weights</code>
            </DocLink>
            , call index 146); every destination must be netuid 0 or an existing subnet, and
            the root weights rate limit applies. The SDK surfaces the same path as{' '}
            <code>bt.SetRootWeights</code>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>For root stakers</p>
          <p>
            Nothing to configure — staking TAO on root with a validator is all it takes to
            accrue shares. Check what you&apos;re owed, then redeem everything in one
            transaction:
          </p>
          <pre className={styles.code_block}>
            {`btcli stake owed                   # pending TAO, itemized per validator
btcli stake claim -w my_coldkey    # claim-root: redeem across all validators`}
          </pre>
          <p>
            <DocLink href='/docs/tx/claim-root'>
              <code>claim_root</code>
            </DocLink>{' '}
            now takes <strong>no parameters</strong>: it walks every validator you root-stake
            to, redeems your owed shares pro-rata from each basket, and stakes the TAO
            proceeds back to root on the same validator. Per-validator payouts below the
            claim threshold (default 500,000 rao; read it with{' '}
            <DocLink href='/docs/query/root-claim-threshold'>
              <code>root-claim-threshold</code>
            </DocLink>
            ) are skipped and keep accruing — there is no deadline and nothing expires.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Breaking changes</p>
          <ul className={styles.list}>
            <li>
              <code>claim_root</code> (call index 121) changed signature: the{' '}
              <code>subnets</code> argument is gone. Old-format transactions no longer
              decode; clients must regenerate metadata.
            </li>
            <li>
              <code>set_root_claim_type</code> (122) and{' '}
              <code>sudo_set_num_root_claims</code> (123) are <strong>removed</strong>.
              Payouts are always TAO staked back to root — the Swap / Keep / KeepSubnets
              setting and the automatic per-block claim sweep are gone. To hold subnet
              alpha, stake on the subnet directly.
            </li>
            <li>
              The <code>RootClaimable</code> / <code>RootClaimed</code> per-subnet state is{' '}
              <strong>migrated into baskets</strong> at upgrade: previously accrued claimable
              alpha becomes basket holdings and shares on the same validator. Nothing is
              lost, and no user action is needed for past accruals.
            </li>
            <li>
              Hotkey and coldkey swaps carry basket state (holdings, shares, claim
              watermarks) to the new key; a hotkey with a live basket is not
              &quot;clean&quot; for reuse. Subnet dissolution converts that subnet&apos;s
              basket holdings into the TAO slot of each affected fund.
            </li>
          </ul>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>New runtime APIs</p>
          <p>
            A <code>BetaBasketRuntimeApi</code> ships with the release, mirrored as{' '}
            <code>betaBasket_*</code> RPC methods and as named SDK reads (available under{' '}
            <code>btcli query</code> and <code>client.read</code>):
          </p>
          <pre className={styles.code_block}>
            {`betaBasket_getStakerOwed(coldkey)            root-basket-owed
betaBasket_getStakerValidatorOwed(hk, ck)    root-basket-owed-breakdown
betaBasket_getValidatorBasket(hotkey)        validator-basket
betaBasket_getValidatorNav(hotkey)           validator-basket-nav
betaBasket_getTotalNav()                     root-basket-total-nav
betaBasket_getValidatorWeights(hotkey)       validator-root-weights`}
          </pre>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <p>
            Operators should wait for the on-chain <code>spec_version</code> to move to 438,
            then upgrade nodes and clients. SDK users should pull the matching bittensor
            release once the train publishes it.
          </p>
          <p>
            <strong>Root validators:</strong> set your root weights (
            <code>btcli weights set-root</code>) — until you do, your root dividends are
            recycled rather than accruing to your stakers.{' '}
            <strong>Stakers:</strong> claims are now fund-level; <code>btcli stake owed</code>{' '}
            shows pending TAO and <code>btcli stake claim</code> redeems it. The retired{' '}
            <code>btcli stake set-claim</code> / <code>process-claim</code> commands are
            replaced by <code>stake claim</code>.
          </p>
          <p>
            <strong>Indexers and integrators:</strong> regenerate metadata for the changed{' '}
            <code>claim_root</code> and new <code>set_root_weights</code> (146) calls, drop
            the retired 122/123 calls, and add the <code>RootWeightsSet</code>,{' '}
            <code>BasketDeposited</code>, <code>BasketClaimed</code>, <code>RootClaimed</code>
            , and <code>BasketHoldingConverted</code> events plus the{' '}
            <code>betaBasket_*</code> RPC namespace. The claim threshold is root-settable via{' '}
            <code>sudo_set_root_claim_threshold</code> (124), wrapped as{' '}
            <DocLink href='/docs/tx/set-root-claim-threshold'>
              <code>set-root-claim-threshold</code>
            </DocLink>
            .
          </p>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v438 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/docs/guides/root-dividends'>Read the root dividends guide</Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
