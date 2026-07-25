import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import snapshot from '../../../../../public/catalog/root-reborn-snapshot.json';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'Root Reborn — The V438 Upgrade',
  description:
    'Nearly half of every TAO ever minted sits on root. Root Reborn turns its dividend ' +
    'stream into validator-curated beta baskets — live network numbers, how the fund ' +
    'works, and what to do.',
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
const MUTED = 'rgba(41, 41, 41, 0.45)';
const GOLD = '#e0a53f';

const fmt = new Intl.NumberFormat('en-US', {maximumFractionDigits: 0});
const pct = (x: number, digits = 1) => `${(x * 100).toFixed(digits)}%`;
const taoUsd = snapshot.taoUsd;

/** Compact USD for stock figures: $1.02B, $188M, $184k. */
const usd = (tao: number) => {
  const dollars = tao * taoUsd;
  const abs = Math.abs(dollars);
  if (abs >= 1e9) return `$${(dollars / 1e9).toFixed(2)}B`;
  if (abs >= 1e6) return `$${(dollars / 1e6).toFixed(0)}M`;
  if (abs >= 1e3) return `$${(dollars / 1e3).toFixed(0)}k`;
  return `$${dollars.toFixed(0)}`;
};

const rootDividendsPerDay = fmt.format(snapshot.rootDividendsTaoPerDay);
const buySideBoost = pct(snapshot.rootDividendsTaoPerDay / snapshot.taoPerDayIntoPools, 0);

/** Calculated day mint: 0.5 τ/block × 7200 blocks/day (BlockEmission storage may still read 1.0). */
const DAY_EMISSION_TAO = 3600;
const poolInjectPerDay = snapshot.taoPerDayIntoPools;
const rootMakebackPerDay = snapshot.rootDividendsTaoPerDay;
const saveShareOfMint = poolInjectPerDay / DAY_EMISSION_TAO;
const makebackShareOfMint = rootMakebackPerDay / DAY_EMISSION_TAO;
const retainedPreShare = saveShareOfMint;
const retainedPostShare = (poolInjectPerDay + rootMakebackPerDay) / DAY_EMISSION_TAO;
const marketFlowSwingPerDay = rootMakebackPerDay * 2;

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

const FlipDiagram = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 300'
    role='img'
    aria-label='Before the upgrade, the root dividend stream is sold every block into sell pressure. After it, the same stream is deployed across validator-curated baskets, compounds, and is realized only when the staker claims.'
  >
    <text {...GRAPH_TEXT} x='30' y='52' fill={MUTED}>
      BEFORE
    </text>
    <rect x='120' y='30' width='150' height='36' fill='none' stroke={MUTED} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='195' y='52' textAnchor='middle' fill={MUTED}>
      ROOT YIELD α
    </text>
    <line x1='270' y1='48' x2='420' y2='48' stroke={MUTED} strokeWidth='1.5' />
    <polygon points='420,48 412,44 412,52' fill={MUTED} />
    <text {...GRAPH_TEXT} x='345' y='38' textAnchor='middle' fill={MUTED}>
      SOLD EVERY BLOCK
    </text>
    <rect x='424' y='30' width='190' height='36' fill='none' stroke={MUTED} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='519' y='52' textAnchor='middle' fill={MUTED}>
      SELL PRESSURE · TAXED
    </text>
    <text {...GRAPH_TEXT} x='650' y='52' fill={MUTED}>
      GONE
    </text>

    <line x1='30' y1='96' x2='730' y2='96' stroke='rgba(41,41,41,0.15)' strokeWidth='1' />

    <text {...GRAPH_TEXT} x='30' y='150'>
      AFTER
    </text>
    <rect x='120' y='128' width='150' height='36' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='195' y='150' textAnchor='middle'>
      ROOT YIELD α
    </text>
    <line x1='270' y1='146' x2='340' y2='146' stroke={INK} strokeWidth='1.5' />
    <polygon points='340,146 332,142 332,150' fill={INK} />
    <text {...GRAPH_TEXT} x='305' y='136' textAnchor='middle'>
      SOLD ONCE
    </text>

    <rect
      x='344'
      y='110'
      width='200'
      height='104'
      fill='rgba(41, 41, 41, 0.03)'
      stroke='rgba(41, 41, 41, 0.4)'
      strokeWidth='1'
      strokeDasharray='4 4'
    />
    <text {...GRAPH_TEXT} x='444' y='126' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      VALIDATOR BASKET
    </text>
    <rect x='360' y='136' width='168' height='20' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='368' y='150'>
      SUBNET α HOLDINGS
    </text>
    <rect x='360' y='164' width='168' height='20' fill='none' stroke={INK} strokeWidth='1.5' />
    <rect x='364' y='168' width='8' height='8' fill={GOLD} />
    <text {...GRAPH_TEXT} x='378' y='178'>
      TAO SLOT
    </text>
    <text {...GRAPH_TEXT} x='444' y='202' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      COMPOUNDS EVERY EPOCH
    </text>

    <line x1='548' y1='146' x2='624' y2='146' stroke={INK} strokeWidth='1.5' />
    <polygon points='624,146 616,142 616,150' fill={INK} />
    <text {...GRAPH_TEXT} x='586' y='136' textAnchor='middle'>
      CLAIM
    </text>
    <rect x='628' y='128' width='102' height='36' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='679' y='144' textAnchor='middle'>
      TAO · YOURS
    </text>
    <text {...GRAPH_TEXT} x='679' y='157' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      WHEN YOU CHOOSE
    </text>
    <path
      d='M 712 118 l 5 5 l 9 -11'
      fill='none'
      stroke='#5a8f5a'
      strokeWidth='2'
      strokeLinecap='round'
      strokeLinejoin='round'
    />

    <text {...GRAPH_TEXT} x='380' y='262' textAnchor='middle'>
      SAME STREAM · {rootDividendsPerDay} τ / DAY ·{' '}
      {pct(snapshot.rootDividendsPctOfEmission, 0)} OF EMISSION
    </text>
    <text {...GRAPH_TEXT} x='380' y='278' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      REDEPLOYED ≈ +{buySideBoost} ON ALL TAO ENTERING SUBNET POOLS DAILY
    </text>
  </svg>
);

const BasketFlowDiagram = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 340'
    role='img'
    aria-label='Each epoch a validator’s root alpha dividend is sold to TAO and split across subnets per its root weights, buying alpha into an escrowed basket; root stakers are minted fund shares at NAV and later redeem them pro-rata as TAO staked back to root.'
  >
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

    <line x1='166' y1='130' x2='294' y2='130' stroke={INK} strokeWidth='1.5' />
    <polygon points='294,130 286,126 286,134' fill={INK} />
    <text {...GRAPH_TEXT} x='230' y='118' textAnchor='middle'>
      SOLD TO TAO
    </text>
    <text {...GRAPH_TEXT} x='230' y='146' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      SPLIT PER ROOT WEIGHTS
    </text>

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
    <rect x='334' y='192' width='8' height='8' fill={GOLD} />
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

    <line x1='546' y1='130' x2='624' y2='130' stroke={INK} strokeWidth='1.5' />
    <polygon points='624,130 616,126 616,134' fill={INK} />
    <text {...GRAPH_TEXT} x='585' y='118' textAnchor='middle'>
      SHARES
    </text>
    <text {...GRAPH_TEXT} x='585' y='146' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      MINTED AT NAV
    </text>

    <polygon points={hexPoints(672, 130, 40)} fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='672' y='127' textAnchor='middle'>
      ROOT
    </text>
    <text {...GRAPH_TEXT} x='672' y='141' textAnchor='middle'>
      STAKERS
    </text>

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
          <p className={styles.paper_title}>Root Reborn</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            The V438 Upgrade · July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p>
            TAO is not a fee token. It is a productive asset: the chain invests liquidity into{' '}
            {snapshot.registeredSubnets} competing subnets and takes a share of every one of
            them back, block by block. That share is called <strong>root proportion</strong> —
            the return owed to TAO itself. Where it lands is the root network, subnet 0, and
            what sits there is the deepest pool of capital in Bittensor:
          </p>
          <p className={styles.headline_number}>{fmt.format(snapshot.rootStakeTao)} τ</p>
          <p className={styles.headline_label}>
            {usd(snapshot.rootStakeTao)} staked on root — {pct(snapshot.rootShareOfIssuance)} of
            every TAO ever minted
          </p>
          <p>
            Nearly half of all TAO in existence, {pct(snapshot.rootShareOfStake, 0)} of all
            staked TAO, concentrated in one place — and until now it was the only capital in
            the network with no intelligence attached. Its yield was sold the moment it
            arrived, mechanically, every block. Spec <strong>438</strong> switches the giant
            on: per-subnet claimable dividends become{' '}
            <DocLink href='/docs/guides/root-reborn'>beta baskets</DocLink> — every root
            validator runs an escrowed index fund of subnet alpha, curated by its root
            weights, and stakers redeem fund shares with one parameterless claim.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>The network, in raw numbers</p>
          <table className={styles.metrics_table}>
            <tbody>
              <tr>
                <td>TAO price</td>
                <td>${taoUsd.toFixed(2)}</td>
              </tr>
              <tr>
                <td>TAO issued to date</td>
                <td>
                  {fmt.format(snapshot.totalIssuanceTao)} τ · {usd(snapshot.totalIssuanceTao)}
                </td>
              </tr>
              <tr>
                <td>Staked on the root network</td>
                <td>
                  {fmt.format(snapshot.rootStakeTao)} τ · {usd(snapshot.rootStakeTao)} ·{' '}
                  {pct(snapshot.rootShareOfIssuance)} of issuance
                </td>
              </tr>
              <tr>
                <td>Subnets live and earning</td>
                <td>
                  {snapshot.liveSubnets} of {snapshot.registeredSubnets}
                </td>
              </tr>
              <tr>
                <td>
                  Chain-held TAO in subnet pools
                  <br />
                  <span style={{opacity: 0.55, fontSize: 11}}>
                    from TAO→α buys — sits until deregister
                  </span>
                </td>
                <td>
                  {fmt.format(snapshot.taoInSubnetPools)} τ · {usd(snapshot.taoInSubnetPools)}
                </td>
              </tr>
              <tr>
                <td>Subnet alpha market cap</td>
                <td>
                  {fmt.format(snapshot.alphaMarketCapTao)} τ · {usd(snapshot.alphaMarketCapTao)}
                </td>
              </tr>
              <tr>
                <td>Root dividend stream (protocol revenue, 7-day avg)</td>
                <td>
                  {rootDividendsPerDay} τ / day · {usd(snapshot.rootDividendsTaoPerDay)} / day ·{' '}
                  {pct(snapshot.rootDividendsPctOfEmission, 0)} of emission
                </td>
              </tr>
              <tr>
                <td>Cumulative protocol revenue since dTAO</td>
                <td>
                  {fmt.format(snapshot.cumulativeRootRevenueTao)} τ ·{' '}
                  {usd(snapshot.cumulativeRootRevenueTao)}
                </td>
              </tr>
              <tr>
                <td>Base root yield at that run-rate</td>
                <td>{pct(snapshot.rootYieldApr)} / yr in TAO</td>
              </tr>
              <tr>
                <td>Paid to miners for useful work</td>
                <td>
                  {fmt.format(snapshot.minersTaoPerDay)} τ / day ·{' '}
                  {usd(snapshot.minersTaoPerDay)} / day
                </td>
              </tr>
              <tr>
                <td>Root dividend gate (Σ subnet EMA prices)</td>
                <td>
                  {snapshot.emaPriceSum.toFixed(2)} ·{' '}
                  {snapshot.rootDividendGateOpen ? 'open' : 'closed'}
                </td>
              </tr>
            </tbody>
          </table>
          <p className={styles.data_note}>
            finney block {fmt.format(snapshot.block)} · july 24, 2026 · ${taoUsd.toFixed(2)}/τ ·
            sources: taomarketcap api (subnets + protocol revenue + market candles) + chain
            storage
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Emission P&amp;L</p>
          <p>
            Three ledgers on one mint. The chain <strong>spends</strong> dilution,{' '}
            <strong>saves</strong> a slice as pool liquidity, and <strong>makes back</strong>{' '}
            root&apos;s share of subnet alpha as protocol revenue. Root Reborn does not change
            the spend — it flips the disposition of the makeback.
          </p>
          <p className={styles.headline_number}>
            {pct(retainedPreShare, 0)} → {pct(retainedPostShare, 0)}
          </p>
          <p className={styles.headline_label}>
            of daily mint kept productive — pool inject alone vs inject + basket redeploy
          </p>
          <table className={styles.metrics_table}>
            <tbody>
              <tr>
                <td>
                  Spend
                  <br />
                  <span style={{opacity: 0.55, fontSize: 11}}>
                    day mint · 0.5 τ/block × 7200
                  </span>
                </td>
                <td>
                  {fmt.format(DAY_EMISSION_TAO)} τ / day · {usd(DAY_EMISSION_TAO)} / day
                </td>
              </tr>
              <tr>
                <td>
                  Save
                  <br />
                  <span style={{opacity: 0.55, fontSize: 11}}>
                    pool inject → SubnetTAO
                  </span>
                </td>
                <td>
                  {fmt.format(poolInjectPerDay)} τ / day · {usd(poolInjectPerDay)} / day ·{' '}
                  {pct(saveShareOfMint, 0)} of mint
                </td>
              </tr>
              <tr>
                <td>
                  Makeback
                  <br />
                  <span style={{opacity: 0.55, fontSize: 11}}>
                    root dividends · TMC 7-day avg
                  </span>
                </td>
                <td>
                  {rootDividendsPerDay} τ / day · {usd(rootMakebackPerDay)} / day ·{' '}
                  {pct(makebackShareOfMint, 0)} of mint
                </td>
              </tr>
              <tr>
                <td>
                  Market flow swing
                  <br />
                  <span style={{opacity: 0.55, fontSize: 11}}>
                    stop selling + start buying the same stream
                  </span>
                </td>
                <td>
                  {fmt.format(marketFlowSwingPerDay)} τ / day · {usd(marketFlowSwingPerDay)}{' '}
                  / day
                </td>
              </tr>
            </tbody>
          </table>
          <p className={styles.data_note}>
            net organic pool accrual ≈ inject − makeback sales ≈{' '}
            {fmt.format(poolInjectPerDay - rootMakebackPerDay)} τ / day before private stake
            flows · makeback is a parallel cash ledger (root α marked to TAO), not a residual
            of the inject split
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>The leak, and the flip</p>
          <p>
            Every day the subnets pay root roughly <strong>{rootDividendsPerDay} TAO</strong>{' '}
            ({usd(snapshot.rootDividendsTaoPerDay)}/day) worth of their alpha —{' '}
            {pct(snapshot.rootDividendsPctOfEmission, 0)} of all new emission, and{' '}
            {fmt.format(snapshot.cumulativeRootRevenueTao)} τ (
            {usd(snapshot.cumulativeRootRevenueTao)}) collected since dTAO went live. Under
            the old machinery that entire stream was force-sold:
            instant sell pressure on every subnet token, a taxable event for every staker,
            value out the door on a schedule nobody chose.
          </p>
          <p>
            Root Reborn inverts the pipe. The same dividend stream is now deployed across
            baskets of subnet alpha curated by each root validator, held in a chain-owned
            escrow, compounding every epoch. For scale: about{' '}
            {fmt.format(snapshot.taoPerDayIntoPools)} TAO of fresh emission enters all subnet
            pools daily — redirecting the root stream adds up to{' '}
            <strong>{buySideBoost} more buy-side flow</strong> to the subnets validators
            believe in, and lifts chain-productive retention from{' '}
            {pct(retainedPreShare, 0)} to {pct(retainedPostShare, 0)} of mint — a market flow
            swing of about {usd(marketFlowSwingPerDay)}/day. The network&apos;s single largest
            source of mechanical selling becomes its single largest source of curated demand.
          </p>
          <FlipDiagram />
          <p className={styles.graph_caption}>
            Same stream, opposite sign. Yield that was sold every block now compounds in
            validator baskets and is realized only when the staker claims.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Validators become fund managers</p>
          <p>
            Each root validator now publishes a weight vector — its allocation across subnets,
            on-chain, for everyone to see. Dividends are deployed per that vector; stakers are
            minted shares of the resulting basket at net asset value, and redeem them with one
            claim, paid in TAO staked straight back to root. A weight on netuid 0 holds that
            slice as pure TAO, so fully passive validators remain exactly one setting away.
            The scoreboard is the simplest in finance:{' '}
            <strong>who made their stakers the most TAO?</strong>
          </p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Subnet</th>
                <th>Netuid</th>
                <th>Share of root dividends</th>
                <th>Pool depth τ</th>
              </tr>
            </thead>
            <tbody>
              {snapshot.topRootDividendSubnets.map((s) => (
                <tr key={s.netuid}>
                  <td>{s.name}</td>
                  <td>{s.netuid}</td>
                  <td>{pct(s.shareOfRootDividends)}</td>
                  <td>{fmt.format(s.poolTao)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className={styles.data_note}>
            top root-dividend payers — the raw material validators now allocate
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>How a basket works</p>
          <p>
            Each epoch, a validator&apos;s root alpha dividend is sold to TAO and redeployed
            across the subnets it has chosen, in proportion to its root weights — buying each
            destination&apos;s alpha into the basket. Holdings live under a chain-owned escrow
            account as real stake positions, so they keep earning. When a dividend lands,
            stakers are minted basket shares priced at the fund&apos;s{' '}
            <strong>realizable</strong> TAO quote — what selling the holdings would actually
            fetch at current pool depth, never a spot mark.
          </p>
          <BasketFlowDiagram />
          <p className={styles.graph_caption}>
            One validator&apos;s fund with a 30/50/20 vector. Dividends buy holdings; stakers
            hold shares; a claim redeems a pro-rata slice of every holding as TAO staked back
            to root.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>For root validators</p>
          <p>
            Set your distribution vector with your hotkey. A validator with no custom weights
            still accrues — dividends default to 100% root (TAO in the fund&apos;s root slot).
            Set the vector when you want to deploy into subnet alpha.
          </p>
          <pre className={styles.code_block}>
            {`btcli root set-weights --weights "0:0.2,4:0.3,8:0.5" -w my_wallet
btcli root get-weights --hotkey 5F...
btcli root show --hotkey 5F...         # your fund: weights, holdings, NAV`}
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
            Nothing to configure — stake TAO on root and shares accrue automatically.{' '}
            <code>btcli</code> shows root positions in beta (β): staked β is principal,
            accrued β is fund yield. Today&apos;s root stream is a{' '}
            {pct(snapshot.rootYieldApr)} base yield in TAO; after this release that floor is
            where the story starts — yield compounds inside the basket, and{' '}
            <strong>nothing is realized until you claim</strong>.
          </p>
          <pre className={styles.code_block}>
            {`btcli root list                    # staked β + accrued β, per validator
btcli tx claim-root -w my_coldkey  # claim-root: redeem across all validators`}
          </pre>
          <p>
            <DocLink href='/docs/tx/claim-root'>
              <code>claim_root</code>
            </DocLink>{' '}
            takes <strong>no parameters</strong>: it walks every validator you root-stake to,
            redeems your owed shares pro-rata from each basket, and stakes the TAO proceeds
            back to root. Per-validator payouts below the claim threshold (default 500,000
            rao; read it with{' '}
            <DocLink href='/docs/query/root-claim-threshold'>
              <code>root-claim-threshold</code>
            </DocLink>
            ) are skipped and keep accruing — there is no deadline and nothing expires.
            Unstaking past your staked β also merges accrued β automatically.
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
            <strong>Root validators:</strong> set your root weights with{' '}
            <code>btcli root set-weights</code> to curate subnet exposure (the default is
            100% root / TAO). <strong>Stakers:</strong> claims are now fund-level;{' '}
            <code>btcli root list</code> shows staked and accrued β and{' '}
            <code>btcli tx claim-root</code> redeems it. The retired{' '}
            <code>btcli stake set-claim</code> / <code>process-claim</code> commands are
            replaced by the <code>btcli root</code> suite.
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
          <Link href='/docs/guides/root-reborn'>Read the Root Reborn guide</Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
