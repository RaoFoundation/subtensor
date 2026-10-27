import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V439 Upgrade — Conviction for Contracts',
  description:
    'EVM access to stake locks and miner conviction, rolled lock views, bounded conviction ' +
    'queries, locked-alpha transfer policy, and subnet owner-cut auto-lock controls.',
  alternates: {canonical: '/releases/v439-upgrade'},
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
const MUTED = 'rgba(41, 41, 41, 0.5)';
const ACCENT = '#d15168';

const LockLifecycleDiagram = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 350'
    role='img'
    aria-label='A coldkey locks existing alpha to a hotkey. In the default decaying mode, locked mass returns gradually while conviction matures and later decays. In perpetual mode, locked mass stays fixed and conviction approaches the locked amount until perpetual mode is disabled.'
  >
    <text {...GRAPH_TEXT} x='95' y='42' textAnchor='middle'>
      EXISTING α STAKE
    </text>
    <rect x='35' y='62' width='120' height='58' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='95' y='96' textAnchor='middle'>
      COLDKEY
    </text>

    <line x1='155' y1='91' x2='235' y2='91' stroke={INK} strokeWidth='1.5' />
    <polygon points='235,91 225,86 225,96' fill={INK} />
    <text {...GRAPH_TEXT} x='195' y='78' textAnchor='middle'>
      LOCK
    </text>

    <rect
      x='235'
      y='45'
      width='190'
      height='94'
      fill='rgba(209, 81, 104, 0.05)'
      stroke={ACCENT}
      strokeWidth='1.5'
    />
    <text {...GRAPH_TEXT} x='330' y='75' textAnchor='middle' fill={ACCENT}>
      SUBNET-WIDE FLOOR
    </text>
    <text {...GRAPH_TEXT} x='330' y='99' textAnchor='middle'>
      LOCKED MASS
    </text>
    <text {...GRAPH_TEXT} x='330' y='120' textAnchor='middle'>
      → HOTKEY CONVICTION
    </text>

    <path d='M 425 91 H 475 V 205 H 520' fill='none' stroke={INK} strokeWidth='1.5' />
    <path d='M 475 91 V 91 H 520' fill='none' stroke={INK} strokeWidth='1.5' />
    <polygon points='520,91 510,86 510,96' fill={INK} />
    <polygon points='520,205 510,200 510,210' fill={INK} />

    <rect x='520' y='45' width='200' height='94' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='620' y='75' textAnchor='middle'>
      PERPETUAL MODE
    </text>
    <text {...GRAPH_TEXT} x='620' y='99' textAnchor='middle' fill={MUTED}>
      MASS STAYS LOCKED
    </text>
    <text {...GRAPH_TEXT} x='620' y='120' textAnchor='middle' fill={ACCENT}>
      CONVICTION → MASS
    </text>

    <rect x='520' y='159' width='200' height='94' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='620' y='189' textAnchor='middle'>
      DEFAULT · DECAYING
    </text>
    <text {...GRAPH_TEXT} x='620' y='213' textAnchor='middle' fill={MUTED}>
      MASS RETURNS OVER TIME
    </text>
    <text {...GRAPH_TEXT} x='620' y='234' textAnchor='middle' fill={ACCENT}>
      CONVICTION ROLLS FORWARD
    </text>

    <line x1='620' y1='253' x2='620' y2='292' stroke={INK} strokeWidth='1.5' />
    <polygon points='620,292 615,282 625,282' fill={INK} />
    <text {...GRAPH_TEXT} x='620' y='318' textAnchor='middle'>
      AVAILABLE TO UNSTAKE
    </text>

    <text {...GRAPH_TEXT} x='330' y='292' textAnchor='middle' fill={MUTED}>
      NO DIRECT UNLOCK CALL
    </text>
    <text {...GRAPH_TEXT} x='330' y='311' textAnchor='middle' fill={MUTED}>
      DISABLE PERPETUAL MODE TO RESUME DECAY
    </text>
  </svg>
);

const ContractReadDiagram = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 340'
    role='img'
    aria-label='A contract calls the staking V2 precompile. A coldkey query returns its rolled lock and target hotkey, a hotkey query returns aggregate locked mass and conviction, and a bounded batch returns conviction for up to 64 distinct hotkeys.'
  >
    <rect x='30' y='108' width='140' height='92' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='100' y='140' textAnchor='middle'>
      EVM CONTRACT
    </text>
    <text {...GRAPH_TEXT} x='100' y='167' textAnchor='middle' fill={MUTED}>
      STATICCALL
    </text>

    <line x1='170' y1='154' x2='260' y2='154' stroke={INK} strokeWidth='1.5' />
    <polygon points='260,154 250,149 250,159' fill={INK} />

    <rect
      x='260'
      y='73'
      width='210'
      height='162'
      fill='rgba(209, 81, 104, 0.05)'
      stroke={ACCENT}
      strokeWidth='1.5'
    />
    <text {...GRAPH_TEXT} x='365' y='106' textAnchor='middle' fill={ACCENT}>
      STAKING V2 · 0x…0805
    </text>
    <line x1='292' y1='124' x2='438' y2='124' stroke={MUTED} strokeWidth='1' />
    <text {...GRAPH_TEXT} x='365' y='153' textAnchor='middle'>
      ROLL TO CURRENT BLOCK
    </text>
    <text {...GRAPH_TEXT} x='365' y='178' textAnchor='middle'>
      CHARGE BOUNDED READS
    </text>
    <text {...GRAPH_TEXT} x='365' y='203' textAnchor='middle'>
      RETURN EXACT Q64.64
    </text>

    <path d='M 470 120 H 535 V 73 H 575' fill='none' stroke={INK} strokeWidth='1.5' />
    <polygon points='575,73 565,68 565,78' fill={INK} />
    <text {...GRAPH_TEXT} x='655' y='49' textAnchor='middle'>
      ONE COLDKEY
    </text>
    <rect x='575' y='58' width='155' height='58' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='652' y='92' textAnchor='middle'>
      LOCK + TARGET
    </text>

    <path d='M 470 154 H 575' fill='none' stroke={INK} strokeWidth='1.5' />
    <polygon points='575,154 565,149 565,159' fill={INK} />
    <rect x='575' y='125' width='155' height='58' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='652' y='159' textAnchor='middle'>
      HOTKEY AGGREGATE
    </text>

    <path d='M 470 188 H 535 V 231 H 575' fill='none' stroke={INK} strokeWidth='1.5' />
    <polygon points='575,231 565,226 565,236' fill={INK} />
    <rect x='575' y='202' width='155' height='58' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='652' y='226' textAnchor='middle'>
      ≤ 64 HOTKEYS
    </text>
    <text {...GRAPH_TEXT} x='652' y='246' textAnchor='middle' fill={MUTED}>
      ORDER PRESERVED
    </text>

    <text {...GRAPH_TEXT} x='380' y='300' textAnchor='middle' fill={MUTED}>
      EXPIRED STALE ROWS REPORT exists = false · VIEWS DO NOT MUTATE STORAGE
    </text>
  </svg>
);

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The V439 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Conviction for Contracts · July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Spec <strong>439</strong> brings the chain&apos;s stake-lock and miner-conviction system
            to EVM after <DocLink href='/releases/v438-upgrade'>v438</DocLink>. Contracts can lock
            existing alpha, direct conviction to a hotkey, move that lock deliberately, choose
            perpetual or decaying behavior, and read current coldkey or aggregate hotkey state
            without reconstructing runtime storage. Subnet owners also gain contract-facing control
            over whether owner-cut emission is locked automatically.
          </p>
          <p>
            This is an interface release: it exposes the runtime&apos;s existing lock model through
            the canonical Solidity interfaces and Python SDK ABIs. The important semantics stay
            on-chain — time-aware decay, conviction maturity, transfer policy, authorization, and
            cleanup — while contracts get bounded calls with explicit gas accounting.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Lock alpha, build conviction</p>
          <p>
            <DocLink href='/code/precompiles/src/solidity/stakingV2.sol'>
              <code>lockStake</code>
            </DocLink>{' '}
            turns part of the caller&apos;s existing alpha on a subnet into a subnet-wide unstaking
            floor. It does not move the stake and the locked alpha can live across several staking
            positions; the supplied hotkey is the target that receives conviction. One coldkey has
            one lock target per subnet. Repeating the call against that target tops the lock up;
            changing the target requires <code>moveLock</code>.
          </p>
          <Code
            language='solidity'
            code={`IStaking staking = IStaking(0x0000000000000000000000000000000000000805);

staking.lockStake(hotkey, 25_000_000_000, netuid);
staking.setPerpetualLock(netuid, true);

// Resume normal lock decay; there is no separate direct-unlock call.
staking.setPerpetualLock(netuid, false);`}
          />
          <LockLifecycleDiagram />
          <p className={styles.graph_caption}>
            Locks decay by default. Perpetual mode keeps the locked mass fixed while conviction
            matures; disabling it returns the lock to the runtime&apos;s ordinary decay curve.
          </p>
          <p>
            A move preserves the current rolled locked mass. Conviction is preserved when the source
            and destination hotkeys share an owner; moving to a differently owned hotkey resets
            conviction, so an established signal cannot be sold across owner boundaries. The global
            unlock and maturity timescales are available through <code>getLockRates()</code>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Current state, not stale rows</p>
          <p>
            Lock storage is lazy: it advances when touched rather than writing every block. The new
            views therefore roll state forward in memory before returning it.{' '}
            <code>getColdkeyLock</code> returns a coldkey&apos;s target hotkey, current locked mass,
            exact conviction, and perpetual flag. <code>getHotkeyLock</code> combines the relevant
            perpetual and decaying aggregate buckets; for the subnet owner hotkey it also includes
            the owner-specific buckets.
          </p>
          <ContractReadDiagram />
          <p className={styles.graph_caption}>
            Every response describes the current block. A fully expired lock reports
            <code>exists = false</code> even if a stale storage row has not yet been cleaned up.
          </p>
          <p>
            Contracts that compare candidates can use{' '}
            <code>getHotkeyConvictions(netuid, hotkeys)</code>. The input is capped at 64 distinct
            hotkeys, results stay aligned with the supplied order, and each conviction is returned
            as exact unsigned Q64.64 bits. Divide by <code>2^64</code> to express it in alpha rao.
            Duplicate candidates are rejected, and gas grows with the bounded candidate list rather
            than an unbounded metagraph scan.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Locked alpha moves only by consent</p>
          <p>
            Stake transfers and coldkey swaps can carry a proportional share of locked mass and
            conviction. Receiving coldkeys reject locked alpha by default, preventing an account
            from being handed stake it cannot immediately unstake. A recipient opts in with{' '}
            <code>setRejectLockedAlpha(false)</code>; contracts can inspect the policy first with{' '}
            <code>getRejectLockedAlpha(coldkey)</code>.
          </p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Recipient policy</th>
                <th>Unlocked alpha</th>
                <th>Locked alpha</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Default: reject</td>
                <td>Accepted</td>
                <td>Rejected</td>
              </tr>
              <tr>
                <td>Opted in</td>
                <td>Accepted</td>
                <td>Accepted with proportional lock state</td>
              </tr>
            </tbody>
          </table>
          <p>
            The policy belongs to the receiving coldkey, not the sending contract. Integrators
            should treat a rejection as a consent boundary and ask the recipient to opt in rather
            than retrying through a different transfer path.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Owner cut can compound into conviction</p>
          <p>
            The subnet precompile at <code>0x…0803</code> adds{' '}
            <code>getOwnerCutAutoLockEnabled</code> and <code>setOwnerCutAutoLockEnabled</code>.
            Auto-locking is off by default. When enabled, new owner-cut emission is added to the
            subnet owner coldkey&apos;s lock: an existing lock keeps its current target; otherwise
            the subnet owner hotkey becomes the target. The setter preserves the runtime&apos;s
            existing root-or-subnet-owner authorization.
          </p>
          <Code
            language='solidity'
            code={`ISubnet subnet = ISubnet(0x0000000000000000000000000000000000000803);

if (!subnet.getOwnerCutAutoLockEnabled(netuid)) {
    subnet.setOwnerCutAutoLockEnabled(netuid, true);
}`}
          />
          <p>
            This is an opt-in compounding policy, not a change to owner-cut distribution. Subnets
            that leave the flag false behave exactly as before.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Contract surface</p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Method</th>
                <th>Purpose</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>
                  <code>lockStake</code>
                </td>
                <td>Lock existing subnet alpha and target a hotkey</td>
              </tr>
              <tr>
                <td>
                  <code>moveLock</code>
                </td>
                <td>Move the lock target with owner-aware conviction handling</td>
              </tr>
              <tr>
                <td>
                  <code>setPerpetualLock</code>
                </td>
                <td>Switch between fixed-mass and decaying lock modes</td>
              </tr>
              <tr>
                <td>
                  <code>getColdkeyLock</code> / <code>getHotkeyLock</code>
                </td>
                <td>Read rolled individual or aggregate lock state</td>
              </tr>
              <tr>
                <td>
                  <code>getHotkeyConvictions</code>
                </td>
                <td>Read exact conviction for up to 64 distinct hotkeys</td>
              </tr>
              <tr>
                <td>
                  <code>setRejectLockedAlpha</code>
                </td>
                <td>Control whether the caller accepts transferred locked alpha</td>
              </tr>
              <tr>
                <td>
                  <code>setOwnerCutAutoLockEnabled</code>
                </td>
                <td>Opt subnet owner-cut emission into automatic locking</td>
              </tr>
            </tbody>
          </table>
          <p>
            The canonical <code>stakingV2.abi</code> and <code>subnet.abi</code>, their Solidity
            interfaces, and the Python SDK copies are synchronized in the release. Numeric inputs
            are range-checked before runtime dispatch, and state-changing calls execute as the
            precompile&apos;s mapped caller account.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <ul className={styles.list}>
            <li>
              <strong>Node operators:</strong> wait for the on-chain <code>spec_version</code> to
              move to 439, then update to the matching release.
            </li>
            <li>
              <strong>EVM integrators:</strong> refresh both canonical ABIs before using the new
              selectors; the staking methods live on V2 at <code>0x…0805</code>.
            </li>
            <li>
              <strong>Contract developers:</strong> decode conviction as unsigned Q64.64, treat
              query results as already rolled to the current block, and keep conviction batches
              distinct and at or below 64 hotkeys.
            </li>
            <li>
              <strong>Transfer applications:</strong> check the destination&apos;s locked-alpha
              policy before moving a position that may carry a lock.
            </li>
            <li>
              <strong>Subnet owners:</strong> owner-cut auto-lock remains disabled until an owner or
              root enables it explicitly.
            </li>
          </ul>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v439 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/code/precompiles/src/solidity/stakingV2.sol'>
            Read the staking V2 interface
          </Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
