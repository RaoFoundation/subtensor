import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V446 Upgrade — Accounting, Liquid Alpha, and Timelock Recovery',
  description:
    'V446 repairs alpha accounting, refines the conviction ownership gate, adds Liquid Alpha ' +
    'consensus modes, preserves failed timelock reveals, and fixes GRANDPA warp sync.',
  alternates: {canonical: '/releases/v446-upgrade'},
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
          <h1 className={styles.paper_title}>The V446 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Accounting, Liquid Alpha, and Timelock Recovery · August 2026
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            Spec <strong>{'446 '}</strong>makes three pieces of economic state more faithful to what
            actually happened on chain. Historical alpha issuance and burn counters are repaired,
            the conviction ownership quorum excludes alpha that cannot support a challenger, and a
            timelock reveal that cannot succeed becomes a visible terminal state instead of
            disappearing into repeated work. The release also lets subnet owners choose which
            epoch&apos;s consensus drives Liquid Alpha and corrects the initial-set offset used by
            Finney GRANDPA warp sync.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>The ownership gate counts eligible alpha</h2>
          <p>
            Conviction-based ownership still requires a subnet to be at least one year old and
            selects the hotkey with the most rolled aggregate conviction. What changes is the quorum
            denominator. Protocol-owned and burned alpha cannot express support for a challenger, so
            v446 excludes both:
          </p>
          <Code
            language='text'
            code={`eligible alpha = saturating(
  SubnetAlphaOut - SubnetProtocolAlpha - AlphaBurned
)

ownership quorum = 10% × eligible alpha`}
          />
          <p>
            If eligible alpha is zero, ownership does not transfer. Existing locks and conviction
            continue to count; the release does not reset them. Operators and indexers should stop
            deriving the threshold from <code>SubnetAlphaOut</code> alone. The{' '}
            <DocLink href='/docs/guides/conviction'>conviction guide</DocLink> and{' '}
            <DocLink href='/docs/query/subnet-convictions'>subnet-convictions read</DocLink> expose
            the new accounting fields and threshold.
          </p>
          <p>
            The upgrade migrations also repair historical <code>SubnetAlphaOut</code> undercounts,
            backfill alpha burned before the counter existed, and remove issuance, burn, and recycle
            offsets inherited by reused subnet slots. Every correction is scoped to the expected
            mainnet subnet generation; a slot whose registration block does not match is left
            untouched.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Liquid Alpha can use current or previous consensus</h2>
          <p>
            Liquid Alpha&apos;s per-bond EMA can now be driven by one of three consensus modes. The
            new storage value defaults to <code>Auto</code>:
          </p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Mode</th>
                <th>Consensus used for Liquid Alpha</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>
                  <code>Current</code>
                </td>
                <td>The consensus calculated in the current epoch.</td>
              </tr>
              <tr>
                <td>
                  <code>Previous</code>
                </td>
                <td>The consensus persisted by the previous epoch, falling back if absent.</td>
              </tr>
              <tr>
                <td>
                  <code>Auto</code>
                </td>
                <td>
                  Previous consensus at the maximum bonds penalty; current consensus otherwise.
                </td>
              </tr>
            </tbody>
          </table>
          <p>
            A subnet owner can change the mode during the admin window, subject to the normal
            per-hyperparameter rate limit. Root can make the same change. Until a first-class
            transaction wrapper is added, use the raw call:
          </p>
          <Code
            language='bash'
            code={`btcli call AdminUtils.sudo_set_liquid_alpha_consensus_mode \\
  --args '{"netuid": 42, "mode": "Auto"}' -w my_owner_wallet`}
          />
          <p>
            This setting only chooses the consensus source;{' '}
            <DocLink href='/docs/hyperparameters/liquid-alpha-enabled'>
              liquid_alpha_enabled
            </DocLink>{' '}
            still controls whether the per-weight Liquid Alpha calculation runs at all.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Failed timelock reveals remain auditable</h2>
          <p>
            The chain now distinguishes a commitment still waiting for a pulse from one that can
            never reveal. A current or future missing pulse remains <code>TimelockEncrypted</code>{' '}
            and stays in the retry index. Corrupt ciphertext, a mismatched round, an invalid
            quicknet point, or an expired pulse becomes <code>TimelockRevealFailed</code>.
          </p>
          <p>
            A terminal failure emits <code>CommitmentRevealFailed</code> once, remains in{' '}
            <code>CommitmentOf</code> for audit, and is removed from the reveal index so the hook
            does not retry it forever. Users cannot submit the failure variant directly. The SDK now
            accepts either its portable timelock envelope or the pallet-native inner ciphertext when
            publishing, and exposes <code>Timelocked.encrypted</code> for the canonical inner form.
            See the <DocLink href='/docs/guides/timelock'>timelock guide</DocLink> for an end-to-end
            commitment example.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>GRANDPA warp sync applies the offset once</h2>
          <p>
            Finney&apos;s historical GRANDPA set-ID offset is now applied only when warp sync starts
            from set zero. Later proof fragments use the set ID returned by the previous fragment,
            rather than applying the initial correction repeatedly across authority rotations. This
            is a node synchronization fix; it does not change runtime consensus rules or require
            application-level changes.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Upgrade checklist</h2>
          <ul>
            <li>Upgrade nodes and metadata-generated clients to runtime spec 446.</li>
            <li>Recalculate conviction ownership thresholds from eligible alpha.</li>
            <li>
              Leave Liquid Alpha on <code>Auto</code> unless the subnet deliberately needs a fixed
              current- or previous-consensus policy.
            </li>
            <li>
              Teach commitment indexers that <code>TimelockRevealFailed</code> is terminal and
              retained for audit, while <code>TimelockEncrypted</code> is pending.
            </li>
          </ul>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
