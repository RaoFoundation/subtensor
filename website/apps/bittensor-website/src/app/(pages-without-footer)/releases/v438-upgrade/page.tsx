import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V438 Upgrade — Interfaces & Reliability',
  description:
    'Bounded EVM staking views, Ledger-friendly limit-order signatures, exact mechanism ' +
    'emission splits, predictable epoch counters, testnet warp-sync repair, and a more ' +
    'recoverable release train.',
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
const MUTED = 'rgba(41, 41, 41, 0.5)';
const ACCENT = '#d15168';

const EvmStakeReadDiagram = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 330'
    role='img'
    aria-label='An EVM contract supplies a coldkey, subnet, and bounded list of candidate hotkeys to the staking precompile. The precompile reads only those positions and returns the non-zero stake entries, keeping gas proportional to at most 64 candidates.'
  >
    <rect x='60' y='90' width='170' height='130' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='145' y='120' textAnchor='middle'>
      EVM CONTRACT
    </text>
    <text {...GRAPH_TEXT} x='145' y='150' textAnchor='middle' fill={MUTED}>
      COLDKEY · NETUID
    </text>
    <text {...GRAPH_TEXT} x='145' y='168' textAnchor='middle' fill={MUTED}>
      ≤ 64 HOTKEYS
    </text>

    <line x1='230' y1='155' x2='315' y2='155' stroke={INK} strokeWidth='1.5' />
    <polygon points='315,155 305,150 305,160' fill={INK} />
    <text {...GRAPH_TEXT} x='272' y='145' textAnchor='middle'>
      STATICCALL
    </text>

    <rect
      x='315'
      y='70'
      width='190'
      height='170'
      fill='rgba(209, 81, 104, 0.05)'
      stroke={ACCENT}
      strokeWidth='1.5'
    />
    <text {...GRAPH_TEXT} x='410' y='102' textAnchor='middle' fill={ACCENT}>
      STAKING PRECOMPILE
    </text>
    <text {...GRAPH_TEXT} x='410' y='126' textAnchor='middle'>
      0x…0805
    </text>
    <line x1='344' y1='145' x2='476' y2='145' stroke={MUTED} strokeWidth='1' />
    <text {...GRAPH_TEXT} x='410' y='170' textAnchor='middle'>
      DISTINCT INPUTS
    </text>
    <text {...GRAPH_TEXT} x='410' y='189' textAnchor='middle'>
      BOUNDED READS
    </text>
    <text {...GRAPH_TEXT} x='410' y='208' textAnchor='middle'>
      NON-ZERO OUTPUTS
    </text>

    <line x1='505' y1='155' x2='590' y2='155' stroke={INK} strokeWidth='1.5' />
    <polygon points='590,155 580,150 580,160' fill={INK} />

    <rect x='590' y='90' width='120' height='130' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='650' y='120' textAnchor='middle'>
      STAKEINFO[]
    </text>
    <text {...GRAPH_TEXT} x='650' y='151' textAnchor='middle' fill={MUTED}>
      HOTKEY
    </text>
    <text {...GRAPH_TEXT} x='650' y='169' textAnchor='middle' fill={MUTED}>
      ALPHA STAKE
    </text>
    <text {...GRAPH_TEXT} x='650' y='198' textAnchor='middle' fill={ACCENT}>
      GAS-BOUNDED
    </text>

    <text {...GRAPH_TEXT} x='380' y='286' textAnchor='middle' fill={MUTED}>
      CALLER CHOOSES THE SEARCH SET · THE RUNTIME NEVER WALKS AN UNBOUNDED HISTORY
    </text>
  </svg>
);

const SignatureCompatibilityDiagram = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 340'
    role='img'
    aria-label='A limit order may be signed directly over its SCALE encoding or in the Ledger-compatible Bytes-wrapped hash form. Sr25519 and ed25519 signatures from either path are accepted, while ECDSA remains rejected.'
  >
    <text {...GRAPH_TEXT} x='100' y='55' textAnchor='middle'>
      LIMIT ORDER
    </text>
    <rect x='40' y='75' width='120' height='58' fill='none' stroke={INK} strokeWidth='1.5' />
    <text {...GRAPH_TEXT} x='100' y='109' textAnchor='middle'>
      SCALE BYTES
    </text>

    <path d='M 160 104 H 260 V 86 H 335' fill='none' stroke={INK} strokeWidth='1.5' />
    <polygon points='335,86 325,81 325,91' fill={INK} />
    <text {...GRAPH_TEXT} x='245' y='75' textAnchor='middle'>
      DIRECT
    </text>

    <path d='M 160 104 H 215 V 215 H 335' fill='none' stroke={INK} strokeWidth='1.5' />
    <polygon points='335,215 325,210 325,220' fill={INK} />
    <rect x='230' y='180' width='105' height='70' fill='none' stroke={MUTED} strokeWidth='1' />
    <text {...GRAPH_TEXT} x='282' y='204' textAnchor='middle'>
      BLAKE2-256
    </text>
    <text {...GRAPH_TEXT} x='282' y='225' textAnchor='middle'>
      &lt;BYTES&gt;…&lt;/BYTES&gt;
    </text>
    <text {...GRAPH_TEXT} x='282' y='270' textAnchor='middle' fill={ACCENT}>
      LEDGER SIGNRAW
    </text>

    <rect
      x='335'
      y='55'
      width='190'
      height='200'
      fill='rgba(209, 81, 104, 0.05)'
      stroke={ACCENT}
      strokeWidth='1.5'
    />
    <text {...GRAPH_TEXT} x='430' y='88' textAnchor='middle' fill={ACCENT}>
      RUNTIME VERIFIER
    </text>
    <line x1='365' y1='108' x2='495' y2='108' stroke={MUTED} strokeWidth='1' />
    <text {...GRAPH_TEXT} x='430' y='143' textAnchor='middle'>
      SR25519 ✓
    </text>
    <text {...GRAPH_TEXT} x='430' y='174' textAnchor='middle'>
      ED25519 ✓
    </text>
    <text {...GRAPH_TEXT} x='430' y='205' textAnchor='middle' fill={MUTED}>
      ECDSA ✕
    </text>
    <text {...GRAPH_TEXT} x='430' y='232' textAnchor='middle' fill={MUTED}>
      SAME ORDER ID
    </text>

    <line x1='525' y1='155' x2='625' y2='155' stroke={INK} strokeWidth='1.5' />
    <polygon points='625,155 615,150 615,160' fill={INK} />
    <circle cx='675' cy='155' r='40' fill='none' stroke={INK} strokeWidth='1.5' />
    <path d='M 654 155 L 669 170 L 697 137' fill='none' stroke={ACCENT} strokeWidth='3' />
    <text {...GRAPH_TEXT} x='675' y='220' textAnchor='middle'>
      EXECUTABLE
    </text>
  </svg>
);

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The V438 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Interfaces &amp; Reliability · July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Spec <strong>438</strong> is a focused compatibility and correctness release after{' '}
            <DocLink href='/releases/v436-upgrade'>v437</DocLink>. It gives smart contracts a
            bounded way to inspect staking positions, lets Ledger-style signatures authorize limit
            orders, closes two small but important gaps in mechanism and epoch accounting, repairs
            testnet warp sync, and makes the release pipeline recover cleanly from a partial Python
            publication. There is no new token model or migration for ordinary wallets: v438
            tightens the surfaces that applications, operators, and automation already use.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Staking data, directly from EVM</p>
          <p>
            Contracts can now read a coldkey&apos;s non-zero alpha positions on one subnet through
            the staking precompile at <code>0x…0805</code>.{' '}
            <DocLink href='/code/precompiles/src/solidity/stakingV2.sol'>
              <code>getStakeInfoForColdkeyAndNetuid</code>
            </DocLink>{' '}
            accepts a coldkey, netuid, and up to 64 distinct candidate hotkeys, then returns only
            the candidates with stake. The caller supplies the search set deliberately: the
            precompile never walks the coldkey&apos;s unbounded historical hotkey index, so its work
            and gas remain proportional to the request.
          </p>
          <Code
            language='rust'
            code={`IStaking.StakeInfo[] memory positions =
    staking.getStakeInfoForColdkeyAndNetuid(coldkey, netuid, hotkeys);

uint256 baseMinimum = staking.getDefaultMinStake();`}
          />
          <EvmStakeReadDiagram />
          <p className={styles.graph_caption}>
            The contract chooses at most 64 distinct candidate hotkeys. The precompile charges for
            the bounded reads and returns only non-zero <code>(hotkey, stake)</code> pairs.
          </p>
          <p>
            The companion <code>getDefaultMinStake()</code> view exposes the runtime&apos;s base
            staking minimum. It is intentionally a primitive, not a quote: operation fees, subnet
            price conversion, and full-unstake rules can still change what a particular transaction
            accepts. Both selectors are added to the Solidity interface, JSON ABI, Rust precompile,
            and gas-accounting tests, so Solidity and SDK callers share one contract.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Limit orders meet hardware wallets</p>
          <p>
            A signed limit order can now use either sr25519 or ed25519 and can cover either the raw
            SCALE-encoded order or the Ledger-compatible{' '}
            <code>&lt;Bytes&gt;blake2_256(order)&lt;/Bytes&gt;</code> payload produced by
            <code>signRaw</code>. The order ID is unchanged: it remains the blake2-256 hash of the
            versioned order, so cancellation, replay protection, partial fills, and relayer
            restrictions behave exactly as before. ECDSA remains rejected because its account
            recovery model does not map to the 32-byte signer identity used by the pallet.
          </p>
          <SignatureCompatibilityDiagram />
          <p className={styles.graph_caption}>
            Software keys can continue signing the order bytes directly. Ledger and compatible
            signers can sign the wrapped order hash; both paths verify against the same signer and
            derive the same on-chain order ID.
          </p>
          <Code
            language='rust'
            code={`raw       = SCALE(versioned_order)
order_id  = blake2_256(raw)
wrapped   = "<Bytes>" ++ order_id ++ "</Bytes>"

accepted  = sr25519(raw | wrapped) or ed25519(raw | wrapped)
rejected  = ecdsa`}
          />
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Mechanism emission splits are exact</p>
          <p>
            A custom mechanism split must now contain{' '}
            <strong>one entry for every active mechanism</strong>, and the entries must still sum to
            65,535. Previously, a shorter vector passed validation and was padded with zeroes, which
            could silently starve trailing mechanisms while rounding residue was assigned to
            mechanism zero. Owners that want an even split can continue clearing the custom value;
            owners that set one explicitly must describe the full mechanism set.
          </p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Configuration</th>
                <th>Before v438</th>
                <th>v438</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>No custom split</td>
                <td>Even distribution</td>
                <td>Even distribution</td>
              </tr>
              <tr>
                <td>Full vector, sum = 65,535</td>
                <td>Accepted</td>
                <td>Accepted</td>
              </tr>
              <tr>
                <td>Short vector</td>
                <td>Zero-padded</td>
                <td>Rejected</td>
              </tr>
              <tr>
                <td>Wrong sum</td>
                <td>Rejected</td>
                <td>Rejected</td>
              </tr>
            </tbody>
          </table>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Epoch observability stays bounded</p>
          <p>
            <DocLink href='/docs/query/blocks-since-last-step'>
              <code>BlocksSinceLastStep</code>
            </DocLink>{' '}
            is now capped at the subnet&apos;s <code>tempo + 1</code>. The scheduler&apos;s safety
            check uses that same subnet-specific tempo instead of a global maximum. Deferred epochs
            can still report the one-block-overdue state that triggers the fallback, but dashboards
            and clients no longer see a counter grow indefinitely during inconsistent or delayed
            state. The related epoch-status and next-epoch queries use the same model.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Testnet warp sync, repaired</p>
          <p>
            Testnet nodes now start GRANDPA warp sync from two trusted, genesis-scoped authority
            checkpoints. The checkpoints are enabled only when the node sees the testnet genesis
            hash; mainnet and development chains retain their existing initial set-ID behavior. This
            repairs fast synchronization across historical authority-set changes without weakening
            checkpoint selection on any other network. The release also aligns the Polkadot SDK
            revision needed to verify those proofs and corrects dependency features that affected
            node log decoding.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Release train, recoverable</p>
          <p>
            The mainnet watcher now tracks GitHub release completion and stable Python publication
            independently. A release can exist while a failed or partial PyPI upload is retried on
            the next watcher run; a provenance-bound completion marker is written only after every
            expected distribution is accepted. That removes the old failure mode where a successful
            GitHub release prevented the SDK publication from being retried.
          </p>
          <p>
            Every merge to <code>main</code> also publishes the moving development image{' '}
            <code>ghcr.io/raofoundation/subtensor:main</code>. The deployed <code>:latest</code> tag
            remains tied to an executed mainnet release, while
            <code>:devnet</code> and <code>:testnet</code> continue following their network mirrors.
            Developers can therefore test merged code without mistaking it for the version running
            on mainnet. The complete promotion sequence is documented in the{' '}
            <DocLink href='/docs/internals/release-process'>release process</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <ul className={styles.list}>
            <li>
              <strong>Node operators:</strong> wait for the on-chain <code>spec_version</code> to
              move to 438, then update to the matching release. Testnet operators should update
              promptly to pick up the warp-sync checkpoints.
            </li>
            <li>
              <strong>EVM integrators:</strong> refresh <code>stakingV2.abi</code> and use a
              bounded, duplicate-free hotkey list when calling the new stake-position view.
            </li>
            <li>
              <strong>Limit-order clients:</strong> raw sr25519 signatures remain valid.
              Ledger-style wrapped sr25519 and ed25519 signatures are now valid; do not emit ECDSA
              orders.
            </li>
            <li>
              <strong>Subnet owners:</strong> when setting a custom mechanism emission split, send
              exactly one value per active mechanism and keep the total at 65,535.
            </li>
            <li>
              <strong>Indexers:</strong> treat <code>BlocksSinceLastStep</code> as a bounded status
              counter, not an elapsed-time source beyond <code>tempo + 1</code>.
            </li>
          </ul>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v438 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/docs/internals/release-process'>Read the release process</Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
