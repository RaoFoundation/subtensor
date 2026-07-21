import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'The V435 Upgrade — The Collateral Release',
  description:
    'Miner registration collateral: subnets can lock a share of the registration price as a ' +
    'bond miners earn back through incentive. Plus one-call stake transfer to a new coldkey ' +
    'and hotkey, air-gapped Polkadot Vault signing, and fully benchmarked extrinsic weights.',
  alternates: {canonical: '/releases/v435-upgrade'},
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

// Adversary cycle profit vs emissions farmed before ban.
//   bond curve:  π(E) = E + min(k·E, p·T) − T
//   pure burn:   π(E) = E − X
// Illustrative preset: X = 100, p = 83% ⟹ T = X/(1−p) ≈ 600, bond = p·T ≈ 500, k = 0.5.
// Break-even E* = T/(1+k) = 400 (bond) vs X = 100 (pure burn). Within E ∈ [0,1000]
// the min() cap (500) is not yet binding, so both curves are straight and converge at E=1000.
const AdversaryProfitGraph = () => {
  // Plot frame: E ∈ [0,1000] → x ∈ [70,730]; profit ∈ [+900,−600] → y ∈ [40,290].
  const xForE = (e: number) => 70 + (e / 1000) * 660;
  const yForP = (p: number) => 40 + ((900 - p) / 1500) * 250;
  return (
    <svg
      className={styles.graph}
      viewBox='0 0 760 340'
      role='img'
      aria-label='Net profit per register-farm-banned cycle against emissions farmed before the ban. The pure-burn strategy breaks even after farming 100 alpha; with the bond it takes 400 alpha, so cheating must clear a much higher bar before it pays.'
    >
      {/* Region between the two break-even points: extra farming the bond forces */}
      <rect
        x={xForE(100)}
        y={yForP(0)}
        width={xForE(400) - xForE(100)}
        height={290 - yForP(0)}
        fill='rgba(209, 81, 104, 0.06)'
      />

      {/* Axes */}
      <line x1='70' y1='40' x2='70' y2='290' stroke={INK} strokeWidth='1' />
      <line x1='70' y1='290' x2='730' y2='290' stroke={INK} strokeWidth='1' />
      <text {...GRAPH_TEXT} x='730' y='308' textAnchor='end'>
        EMISSIONS FARMED BEFORE BAN (α)
      </text>

      {/* y ticks */}
      <text {...GRAPH_TEXT} x='62' y='43' textAnchor='end'>
        +900
      </text>
      <text {...GRAPH_TEXT} x='62' y='193' textAnchor='end'>
        0
      </text>
      <text {...GRAPH_TEXT} x='62' y='293' textAnchor='end'>
        −600
      </text>
      {/* x ticks */}
      <text {...GRAPH_TEXT} x={xForE(0)} y='305' textAnchor='middle'>
        0
      </text>
      <text {...GRAPH_TEXT} x={xForE(500)} y='305' textAnchor='middle'>
        500
      </text>
      <text {...GRAPH_TEXT} x={xForE(1000)} y='305' textAnchor='middle'>
        1K
      </text>

      {/* Break-even (profit = 0) */}
      <line
        x1='70'
        y1={yForP(0)}
        x2='730'
        y2={yForP(0)}
        stroke='rgba(41, 41, 41, 0.5)'
        strokeWidth='1'
        strokeDasharray='4 4'
      />
      <text {...GRAPH_TEXT} x='726' y={yForP(0) - 6} textAnchor='end' fill='rgba(41, 41, 41, 0.55)'>
        BREAK-EVEN
      </text>

      {/* Pure burn: E − X, from (0,−100) to (1000,900) */}
      <path
        d={`M ${xForE(0)} ${yForP(-100)} L ${xForE(1000)} ${yForP(900)}`}
        fill='none'
        stroke='rgba(41, 41, 41, 0.35)'
        strokeWidth='1.5'
      />
      <text {...GRAPH_TEXT} x='330' y='132' fill='rgba(41, 41, 41, 0.55)'>
        PURE BURN ONLY
      </text>

      {/* With bond: 1.5E − 600, from (0,−600) to (1000,900) */}
      <path
        d={`M ${xForE(0)} ${yForP(-600)} L ${xForE(1000)} ${yForP(900)}`}
        fill='none'
        stroke={INK}
        strokeWidth='1.5'
      />
      <text {...GRAPH_TEXT} x='96' y='250'>
        WITH BOND (p = 83%, k = 0.5)
      </text>

      {/* Break-even crossings */}
      <circle cx={xForE(100)} cy={yForP(0)} r='3.5' fill='rgba(41, 41, 41, 0.55)' />
      <text {...GRAPH_TEXT} x={xForE(100)} y={yForP(0) + 18} textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
        100
      </text>
      <circle cx={xForE(400)} cy={yForP(0)} r='4' fill='#d15168' />
      <text {...GRAPH_TEXT} x={xForE(400) + 8} y={yForP(0) + 18} fill='#d15168'>
        E* = 400 = T ÷ (1 + k)
      </text>
    </svg>
  );
};

const VaultAirGapDiagram = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 340'
    role='img'
    aria-label='The btcli host constructs an unsigned transaction and shows it as a QR code; across an air gap, the offline Polkadot Vault phone decodes and clear-signs it; the signature QR returns through the webcam and the host submits it to the chain, which verifies the same metadata digest.'
  >
    {/* Air gap band */}
    <rect x='300' y='60' width='120' height='230' fill='rgba(41, 41, 41, 0.03)' />
    <line
      x1='300'
      y1='60'
      x2='300'
      y2='290'
      stroke='rgba(41, 41, 41, 0.4)'
      strokeWidth='1'
      strokeDasharray='4 4'
    />
    <line
      x1='420'
      y1='60'
      x2='420'
      y2='290'
      stroke='rgba(41, 41, 41, 0.4)'
      strokeWidth='1'
      strokeDasharray='4 4'
    />
    <text {...GRAPH_TEXT} x='360' y='78' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      AIR GAP
    </text>
    <text {...GRAPH_TEXT} x='360' y='282' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      QR ONLY
    </text>

    {/* btcli host node (online) */}
    <polygon points={hexPoints(180, 175, 44)} fill='none' stroke={INK} strokeWidth='1.5' />
    <rect x='172' y='167' width='8' height='8' fill='#e0a53f' />
    <text {...GRAPH_TEXT} x='180' y='240' textAnchor='middle'>
      BTCLI HOST
    </text>
    <text {...GRAPH_TEXT} x='180' y='254' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      ONLINE
    </text>

    {/* Vault phone node (offline) with lock glyph */}
    <polygon points={hexPoints(510, 175, 52)} fill='none' stroke={INK} strokeWidth='1.5' />
    {/* lock glyph */}
    <rect x='498' y='173' width='24' height='18' rx='2' fill='none' stroke={INK} strokeWidth='1.5' />
    <path
      d='M 502 173 v -6 a 8 8 0 0 1 16 0 v 6'
      fill='none'
      stroke={INK}
      strokeWidth='1.5'
    />
    <circle cx='510' cy='181' r='2' fill={INK} />
    <text {...GRAPH_TEXT} x='510' y='248' textAnchor='middle'>
      VAULT PHONE
    </text>
    <text {...GRAPH_TEXT} x='510' y='262' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      OFFLINE · CLEAR-SIGNS
    </text>

    {/* Unsigned tx QR, host -> phone (top, crossing the gap) */}
    <line x1='224' y1='150' x2='454' y2='150' stroke={INK} strokeWidth='1.5' />
    <polygon points='454,150 446,146 446,154' fill={INK} />
    <text {...GRAPH_TEXT} x='360' y='143' textAnchor='middle'>
      UNSIGNED TX · QR
    </text>

    {/* Signature QR, phone -> host (bottom, crossing back) */}
    <line x1='454' y1='205' x2='224' y2='205' stroke={INK} strokeWidth='1.5' />
    <polygon points='224,205 232,201 232,209' fill={INK} />
    <text {...GRAPH_TEXT} x='360' y='220' textAnchor='middle'>
      SIGNATURE · QR
    </text>

    {/* Submit to chain: host -> verify -> confirmed */}
    <line
      x1='562'
      y1='175'
      x2='628'
      y2='175'
      stroke='rgba(41, 41, 41, 0.5)'
      strokeWidth='1'
      strokeDasharray='4 4'
    />
    <text {...GRAPH_TEXT} x='595' y='168' textAnchor='middle'>
      SUBMIT
    </text>
    <polygon points={hexPoints(672, 175, 36)} fill='none' stroke={INK} strokeWidth='1.5' />
    {/* check glyph */}
    <path
      d='M 660 176 l 8 8 l 14 -16'
      fill='none'
      stroke='#5a8f5a'
      strokeWidth='2'
      strokeLinecap='round'
      strokeLinejoin='round'
    />
    <text {...GRAPH_TEXT} x='672' y='232' textAnchor='middle'>
      CHAIN VERIFIES
    </text>
    <text {...GRAPH_TEXT} x='672' y='246' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      SAME METADATA DIGEST
    </text>
  </svg>
);

const CollateralGraph = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 340'
    role='img'
    aria-label='Locked collateral declining as incentive is earned, fully released when earnings reach bond divided by k; if validators stop scoring the hotkey, the remaining collateral flatlines and strands.'
  >
    {/* Stranded mass region: between the flatlined lock and zero, after the blacklist */}
    <rect x='320' y='198' width='410' height='92' fill='rgba(209, 81, 104, 0.07)' />
    <text {...GRAPH_TEXT} x='525' y='236' textAnchor='middle' fill='#d15168'>
      STRANDED IF BLACKLISTED
    </text>
    <text {...GRAPH_TEXT} x='525' y='250' textAnchor='middle' fill='#d15168'>
      ZERO INCENTIVE, ZERO RELEASE
    </text>

    {/* Axes */}
    <line x1='70' y1='30' x2='70' y2='290' stroke='rgb(41, 41, 41)' strokeWidth='1' />
    <line x1='70' y1='290' x2='730' y2='290' stroke='rgb(41, 41, 41)' strokeWidth='1' />
    <text {...GRAPH_TEXT} x='730' y='310' textAnchor='end'>
      INCENTIVE EARNED
    </text>
    <text {...GRAPH_TEXT} x='62' y='293' textAnchor='end'>
      0α
    </text>
    <text {...GRAPH_TEXT} x='62' y='86' textAnchor='end'>
      9α
    </text>
    <text {...GRAPH_TEXT} x='62' y='63' textAnchor='end'>
      10α
    </text>

    {/* Registration price split */}
    <line
      x1='70'
      y1='60'
      x2='730'
      y2='60'
      stroke='rgba(41, 41, 41, 0.5)'
      strokeWidth='1'
      strokeDasharray='4 4'
    />
    <text {...GRAPH_TEXT} x='76' y='52'>
      REGISTRATION PRICE 10α: 1α BURNED + 9α LOCKED (p = 90%)
    </text>

    {/* Full release point */}
    <line
      x1='520'
      y1='40'
      x2='520'
      y2='290'
      stroke='rgba(41, 41, 41, 0.5)'
      strokeWidth='1'
      strokeDasharray='4 4'
    />
    <text {...GRAPH_TEXT} x='520' y='28' textAnchor='middle'>
      FULLY RELEASED: EARNED = BOND ÷ k
    </text>

    {/* Released-to-free-stake path (honest miner) */}
    <path
      d='M 70 290 L 520 83 L 730 83'
      fill='none'
      stroke='rgba(41, 41, 41, 0.35)'
      strokeWidth='1.5'
    />
    <text {...GRAPH_TEXT} x='236' y='240' fill='rgba(41, 41, 41, 0.55)'>
      RELEASED TO FREE STAKE
    </text>

    {/* Locked collateral path (honest miner) */}
    <path d='M 70 83 L 520 290' fill='none' stroke='rgb(41, 41, 41)' strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='110' y='110'>
      LOCKED COLLATERAL
    </text>
    <text {...GRAPH_TEXT} x='110' y='124' fill='rgba(41, 41, 41, 0.55)'>
      RELEASES k × INCENTIVE PER TEMPO
    </text>

    {/* Blacklist branch: validators stop scoring, the lock flatlines */}
    <line
      x1='320'
      y1='40'
      x2='320'
      y2='290'
      stroke='#d15168'
      strokeWidth='1'
      strokeDasharray='3 3'
    />
    <text {...GRAPH_TEXT} x='326' y='130' fill='#d15168'>
      VALIDATORS STOP SCORING
    </text>
    <path d='M 320 198 L 730 198' fill='none' stroke='#d15168' strokeWidth='1.5' />
    <circle cx='320' cy='198' r='4' fill='#d15168' />
  </svg>
);

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The V435 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            The Collateral Release · July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Spec <strong>435</strong> is the next mainnet runtime after{' '}
            <DocLink href='/releases/v431-upgrade'>v431</DocLink>, and it is a feature release
            headlined by one change to the network&apos;s economics:{' '}
            <DocLink href='/docs/guides/mining/collateral'>
              miner registration collateral
            </DocLink>
            . A subnet can now lock a share of its floating registration price as a bond on
            the registering hotkey instead of burning all of it — a bond the miner earns back
            only by mining. The release also carries the operational work shipped on the way
            here: pure proxies operable by a multisig from the CLI, one-call stake transfer
            to a new coldkey and hotkey, a dedicated stake-transfer minimum, air-gapped
            transaction signing with Polkadot Vault, and benchmark-measured weights for every
            extrinsic in the runtime.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Miner collateral</p>
          <p>
            Registration on a collateral subnet splits the floating price in two. The burned
            share is recycled exactly as before — the spam throttle is unchanged. The locked
            share is staked to the registering hotkey and held as collateral, and each tempo
            the chain releases <code>k × incentive</code> of it back to withdrawable stake.
            <strong> Mining is the only exit.</strong> The lock survives deregistration and is
            credited when the same hotkey re-registers, so a pruned miner returns for roughly
            the burned share alone; a hotkey that validators stop scoring keeps its remainder
            frozen indefinitely.
          </p>
          <CollateralGraph />
          <p className={styles.graph_caption}>
            A miner&apos;s bond over its career, at a 10α registration price with a 90% lock
            share. The lock declines as incentive is earned and is fully released once
            lifetime earnings reach bond ÷ k. If validators blacklist the hotkey — off chain,
            by simply not scoring it — incentive stops, the release stops with it, and the
            remainder strands.
          </p>
          <p>
            The design in one line: <strong>the burn prices the registration event; the
            collateral prices what a miner does afterward.</strong> A pure burn cannot punish
            post-registration behavior — by the time a score-gaming miner is caught, its
            payoff is banked and a fresh hotkey costs only the next burn. With collateral, an
            adversary who plans to farm emissions and abandon the hotkey must out-earn the
            full registration price against a work-gated refund. Sybils, UID squatters, and
            blacklisted hotkeys never release their locks at all. Honest miners are nearly
            unaffected: they recover the bond by doing exactly the work they registered to
            do, and their sunk cost stays the burned share. An interactive model of the bond
            — drag the lock share, drain ratio, and earning rate, and toggle the blacklist
            scenario — is in the{' '}
            <DocLink href='/docs/guides/mining/collateral'>collateral guide</DocLink>.
          </p>
          <p>
            Two miner extrinsics extend the mechanism to deposit-style policies.{' '}
            <code>add_collateral</code> voluntarily locks more on your own hotkey — for
            example to meet a validator-published per-machine requirement on resource
            subnets — and <code>set_min_collateral</code> sets a{' '}
            <strong>self-maintaining floor</strong>: the drain never releases below it, and
            while the lock is under it, earned incentive is captured into the lock until
            the floor is met, so miners don&apos;t re-lock drained funds every tempo. Collateral is a
            first-class metagraph field: every neuron row carries{' '}
            <code>collateral_locked</code>, <code>collateral_min</code>, and{' '}
            <code>collateral_earned</code> (lifetime incentive since the bond existed), so
            validator enforcement costs zero extra calls. The SDK surfaces the same
            path as <code>bt.AddCollateral</code> / <code>bt.SetMinCollateral</code>{' '}
            and <code>client.collateral.*</code> (
            <code>miner_collateral</code>, <code>subnet_collateral</code>,{' '}
            <code>collateral_policy</code>); the CLI mirrors it with{' '}
            <code>btcli collateral</code> (<code>show</code> / <code>list</code> /{' '}
            <code>add</code> / <code>set-min</code>) and the matching{' '}
            <code>btcli query</code> / <code>btcli tx</code> entries.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Using it in your subnet</p>
          <p>
            Collateral is <strong>off by default</strong> — the lock share ships at zero,
            which is byte-for-byte the previous chain behavior. Owners opt in with two
            hyperparameters, settable through <code>btcli sudo set</code> (owner-or-root,
            rate-limited, outside the weights window):
          </p>
          <pre className={styles.code_block}>
            {`btcli sudo set --netuid 42 --name collateral_lock_share --value 0.75   # p = 75%
btcli sudo set --netuid 42 --name collateral_drain_ratio --value 1.0    # k = 1`}
          </pre>
          <p>
            <DocLink href='/docs/hyperparameters/collateral-lock-share'>
              <code>collateral_lock_share</code>
            </DocLink>{' '}
            (p) decides how much of the entry price is accountability rather than toll;{' '}
            <DocLink href='/docs/hyperparameters/collateral-drain-ratio'>
              <code>collateral_drain_ratio</code>
            </DocLink>{' '}
            (k) decides how long a good miner takes to work it off. The deterrence math is one
            line: an adversary who plans to farm and abandon must collect roughly{' '}
            <code>price ÷ (1 + k)</code> in emissions before your validators stop scoring
            them, just to break even. Three starting points:
          </p>
          <ol className={styles.list}>
            <li>
              <strong>Standard bond (p = 75%, k = 1)</strong> — general-purpose deterrence:
              sybils and UID squatters forfeit, honest miners recover at par.
            </li>
            <li>
              <strong>Anti-tail-risk (p = 83–90%, k = 0.2–0.5)</strong> — for subnets whose
              scoring can be gamed on short horizons (trading, forecasting): a large lock with
              a slow drain keeps your fastest <i>apparent</i> earners collateralized through
              your detection window. The guide works this example against a Sortino-scored
              trading market.
            </li>
            <li>
              <strong>Fast release (p = 67%, k = 2)</strong> — a capital gate at entry with
              minimal ongoing lockup, for lower-stakes work.
            </li>
          </ol>
          <p>
            <strong>Worked example — a trading-signals subnet.</strong> Say miners submit
            equity trades and validators score a rolling Sortino ratio. The metric is blind
            to tail risk: a martingale that quietly sells crash insurance posts a
            top-percentile score for months, farms emissions, then blows up. Under pure burn
            (price τ10) the farmer nets ~τ70 a cycle and just re-registers a fresh hotkey. Set{' '}
            <code>collateral_lock_share = 90%</code> and{' '}
            <code>collateral_drain_ratio = 0.2</code>:
          </p>
          <pre className={styles.code_block}>
            {`btcli sudo set --netuid 42 --name collateral_lock_share --value 0.9    # p = 90%
btcli sudo set --netuid 42 --name collateral_drain_ratio --value 0.2    # k = 0.2`}
          </pre>
          <p>
            Registration still costs the same floating τ10 — now τ1 burned, τ9 locked. The
            farmer&apos;s math inverts: to break even they must farm{' '}
            <code>price ÷ (1 + k) ≈ τ8.3</code> in emissions <i>before</i> your validators
            stop scoring the hotkey, and the slow k&nbsp;=&nbsp;0.2 drain keeps ~τ9 at risk
            deep into the run — so the blow-up strands the bond. An honest miner with a real
            edge is barely affected: τ1 sunk, and the τ9 lock releases steadily as they earn.
            The subnet never had to fix Sortino; it just made the tail risk expensive to hide.
          </p>
          <p>
            The payoff curve makes the deflection concrete. Net profit per
            register-farm-banned cycle is <code>E + min(k·E, p·T) − T</code> with the bond
            versus <code>E − X</code> for a pure burn — where E is emissions farmed before the
            ban. Below the break-even line the strategy loses money, so the further right the
            curve crosses zero, the more a farmer must extract before detection just to
            recover their costs:
          </p>
          <AdversaryProfitGraph />
          <p className={styles.graph_caption}>
            Illustrative preset: equilibrium burned share X = 100α, lock share p = 83% (so the
            sticker price T = X ÷ (1 − p) ≈ 600α), drain k = 0.5. Pure burn breaks even after
            farming 100α; with the bond the break-even moves out to E* = T ÷ (1 + k) = 400α.
            Cheating has to clear four times the bar, and if the drain has not yet released
            the bond by the time the ban lands, the remainder is forfeit on top.
          </p>
          <p>
            Two properties make this safe to adopt. Both parameters are{' '}
            <strong>snapshot per miner at registration</strong>, so changing them never
            re-prices standing collateral — there is no way to retroactively lock or rug
            incumbents. And enforcement needs no new machinery: your validators&apos; existing
            power to stop evaluating a hotkey is the blacklist, stake-weighted by construction
            and reversible if they resume scoring. One requirement on the validator side: key
            scoring state by <strong>hotkey</strong>, not UID, and persist it across
            deregistrations, so a track record cannot be laundered by cycling registration.
            Full details, worked examples, and budgeting notes for miners are in the{' '}
            <DocLink href='/docs/guides/mining/collateral'>collateral guide</DocLink> and the{' '}
            <DocLink href='/docs/guides/mining'>mining guide</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Air-gapped signing with Polkadot Vault</p>
          <p>
            Every transaction can now be signed from a{' '}
            <DocLink href='/docs/guides/vault'>Polkadot Vault</DocLink> phone — a device that
            never goes online. Pass <code>--signer vault</code> and the CLI shows the
            transaction as a QR code; the phone decodes the call on its own screen, you
            approve it there, and the signature travels back through your webcam. No keyfile,
            password, or mnemonic ever exists on the machine running btcli. Like{' '}
            <DocLink href='/docs/guides/ledger'>Ledger signing</DocLink>, the flow is
            clear-signing through merkleized metadata, and setup is a single chain-specs scan
            with no metadata updates to sync — ever, including across runtime upgrades. The
            same signer is available in Python as <code>bt.VaultSigner</code>.
          </p>
          <VaultAirGapDiagram />
          <p className={styles.graph_caption}>
            The signing key never touches an online machine. btcli builds the unsigned
            transaction and renders it as a QR code; the offline Vault phone decodes and
            clear-signs it on its own screen; the signature QR returns through the webcam, and
            the chain verifies the same merkleized-metadata digest the phone displayed.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Pure proxies, multisig-operated</p>
          <p>
            The strongest key setup the chain supports is a{' '}
            <DocLink href='/docs/tx/create-pure-proxy'>pure proxy</DocLink> controlled by a{' '}
            <DocLink href='/docs/guides/multisig'>multisig</DocLink>: the operating account —
            a subnet owner key, a treasury — is keyless, so there is no seed to steal, its
            address never changes, and the signer set behind it can be rotated freely. The
            chain has always allowed the composition; the tooling now does too.{' '}
            <code>--proxy-for</code> composes with <code>--multisig</code> on{' '}
            <code>btcli call</code>, so a signer set can dispatch any call as the pure
            account:
          </p>
          <pre className={styles.code_block}>
            {`btcli call SubtensorModule.set_sn_owner_hotkey --args '{...}' \\
  --proxy-for my-subnet-owner --multisig my-team -w alice`}
          </pre>
          <p>
            Each signatory approves the same wrapped call — matched by hash, as with any
            multisig operation. The first approval prints ready-to-run commands for the
            co-signers, and <code>btcli multisig pending</code> reconstructs them on any
            machine by decoding the proxy wrapper from the on-chain call data, so co-signers
            need none of the submitter&apos;s local state. <code>--proxy-for</code> alone
            (without a multisig) now also works on <code>btcli call</code>, matching the
            flag every <code>btcli tx</code> command already carries.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Stake transfers, generalized</p>
          <p>
            <DocLink href='/docs/tx/transfer-stake'>
              <code>transfer_stake_and_hotkey</code>
            </DocLink>{' '}
            hands a stake position to another coldkey and lands it on a different hotkey —
            optionally on a different subnet — in one atomic extrinsic. Previously this took
            two calls (<code>transfer_stake</code> then <code>move_stake</code>), with the
            position exposed on the wrong validator between them and the second call left to
            the recipient. Pass <code>--dest-hotkey</code> to{' '}
            <code>btcli tx transfer-stake</code> (or <code>dest_hotkey_ss58</code> on the{' '}
            <code>TransferStake</code> intent) and the SDK dispatches the new call. Stake
            transfers also get their own minimum:{' '}
            <DocLink href='/code/runtime/src/lib.rs#L819'>
              <code>InitialMinTransfer</code>
            </DocLink>{' '}
            is 0.0001 TAO, where transfers previously had to clear the 0.002 TAO staking
            minimum.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Weights, measured</p>
          <p>
            Every dispatchable in the runtime now carries a{' '}
            <DocLink href='/code/pallets/subtensor/src/weights.rs'>benchmarked weight</DocLink>{' '}
            — including <code>proof_size</code>, which was previously ignored — replacing the
            hand-assigned constants used before. The benchmark suite was rebuilt around
            worst-case state, and a CI lint now fails any PR that adds an extrinsic without a
            plugged-in benchmark. Fees follow weights, so per-call fees shift slightly in both
            directions; nothing changes by an order of magnitude. Alongside: the chain&apos;s
            Rust source is browsable at <DocLink href='/code'>bittensor.com/code</DocLink>{' '}
            exactly as built into the running runtime, extension signing reuses remembered
            accounts without re-prompting, and URLs in CLI output are clickable in supporting
            terminals.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <p>
            Operators should wait for the on-chain <code>spec_version</code> to move to 435,
            then upgrade nodes and clients. SDK users should pull the matching bittensor
            release once the train publishes it — older clients keep working, they simply
            don&apos;t know the new calls. Nothing changes for miners on subnets that keep the
            default collateral settings; on subnets that opt in, check the split before
            registering with <code>btcli query burn</code> and{' '}
            <code>btcli sudo get --name collateral_lock_share</code>.
          </p>
          <p>
            Indexers should add <code>SubtensorModule.transfer_stake_and_hotkey</code> (call
            index 143), <code>SubtensorModule.add_collateral</code> (144),{' '}
            <code>SubtensorModule.set_min_collateral</code> (145),{' '}
            <code>AdminUtils.sudo_set_collateral_lock_share</code> (call index 98),{' '}
            <code>AdminUtils.sudo_set_collateral_drain_ratio</code> (call index 99), the{' '}
            <code>StakeAndHotkeyTransferred</code>, <code>CollateralLocked</code>, and{' '}
            <code>MinCollateralSet</code> events, and the <code>MinerCollateral</code>{' '}
            storage map.
          </p>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v435 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/docs/guides/mining/collateral'>Read the collateral guide</Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
