import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V448 Upgrade — Root Claims, Safer Staking, and Linked Orders',
  description:
    'V448 makes root claims predictable, protects cross-subnet stake moves, adds live staking ' +
    'indexes and multi-hotkey exits, and introduces composable linked limit orders.',
  alternates: {canonical: '/releases/v448-upgrade'},
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
          <h1 className={styles.paper_title}>The V448 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Root claims, safer staking, and linked orders · August 2026
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            Spec <strong>{'448 '}</strong>makes expensive operations easier to predict and
            multi-step operations safer to express. The SDK now blocks root claims that cannot fit
            the runtime envelope during planning, while the runtime rejects oversized raw
            submissions at dispatch. btcli protects cross-subnet moves and supports multi-hotkey
            exits, and linked orders can size a later trade from an earlier trade&apos;s output. Two
            live staking indexes replace scans that become more expensive as the network grows.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Root claims fail safely and quote real costs</h2>
          <p>
            Both root-claim paths reserve the same conservative 256-unit work envelope, independent
            of the number of live networks. The runtime rejects a claim with{' '}
            <code>RootClaimTooHeavy</code> when its required work cannot fit that envelope, then
            refunds the fee for unused declared weight after a successful claim. The SDK mirrors
            that check during planning and evaluates the minimum payout threshold per validator,
            matching runtime enforcement rather than combining a coldkey&apos;s positions into one
            aggregate result.
          </p>
          <p>
            Fee previews distinguish the signer that pays from the proxy or multisig account whose
            state the call changes. Shielded submissions run the same hard intent checks as ordinary
            submissions, so wrapping a call no longer bypasses the reserve or work-limit preflight.
            See the <DocLink href='/docs/guides/root-reborn'>Root Reborn guide</DocLink> for claim
            fees, thresholds, and basket semantics.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Stake moves and multi-hotkey exits are safer</h2>
          <p>
            Runtime v448 adds <code>move_stake_limit</code>: one call can change both hotkey and
            subnet while enforcing a minimum destination-alpha per origin-alpha ratio. SDK and btcli
            cross-subnet moves apply a 5% protection by default; callers can set an explicit
            tolerance or deliberately disable the guard. SDK and btcli submit protected moves as
            fill-or-kill; direct runtime, EVM, and WASM callers can opt into partial execution.
          </p>
          <p>
            Root <code>--claim --all</code> exits now claim first and resolve the full post-claim
            stake inside the atomic batch, so freshly claimed yield does not remain staked. This
            atomic form is available only while <code>RootStakeUnlockInterval</code> is zero. When a
            hold is configured, claim separately, wait out the interval, then unstake. The new{' '}
            <code>btcli stake unstake-all --all-hotkeys</code> path discovers every staking hotkey
            for the coldkey and attempts to exit positions across them. The runtime can skip
            disabled or otherwise invalid positions, so automation should verify the remaining
            stake. Review the <DocLink href='/docs/guides/staking'>staking guide</DocLink> and{' '}
            <DocLink href='/docs/tx/unstake-all'>unstake-all reference</DocLink> before automating a
            bulk wallet exit.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Live staking indexes replace expensive scans</h2>
          <p>
            <code>TotalAlphaStaked</code> maintains each subnet&apos;s aggregate stake in
            constant-time storage, while <code>StakingHotkeys</code> exposes the hotkeys relevant to
            a coldkey&apos;s staking and root-basket state. The latter is also available through a
            paged EVM view. Applications can use these indexes instead of walking every stake
            position.
          </p>
          <p>
            Historical totals are backfilled and stale hotkey relationships are cleaned in bounded
            passes using otherwise-unused <code>on_idle</code> weight. Cleanup preserves a
            relationship when root-basket claim state still needs it for discovery. Normal staking
            remains enabled throughout and operators do not need to run a separate migration. Until
            the total-stake backfill reaches a key, that key can be omitted from the aggregate; live
            mutations are reconciled by migration completion, not necessarily visible immediately.
            Unrelated stale hotkey relationships can likewise remain until cleanup finishes.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Linked orders compose multi-leg strategies</h2>
          <p>
            A V2 limit order can record its post-fee output, and a later order can spend a signed
            percentage of that amount. Provider and consumer must have the same signer, the output
            asset must match the consumer&apos;s input, and neither side may use partial fills. The
            output record is accounting, not custody: funds remain in the signer&apos;s balance, and
            the first consumer removes the single-use record. Unused output remains ordinary
            balance.
          </p>
          <p>
            Records stay drawable for seven days. <code>execute_orders</code> can place a provider
            before its consumer in one call; <code>execute_batched_orders</code> resolves amounts
            before its netted swap, so the two legs must be submitted separately there. Existing V1
            signed orders remain valid. The{' '}
            <DocLink href='/code/pallets/limit-orders/README.md'>limit-orders reference</DocLink>
            documents the signed payload, validation rules, and pruning behavior.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Wrapped and shielded submissions preserve intent</h2>
          <p>
            Planning now keeps dispatch origin and fee payer separate through proxies and multisigs,
            so warnings, effects, and hard blocks inspect the account the call actually changes.
            Shielded raw calls and multisig approvals honor <code>wait_for_finalization</code> for
            the decrypted inner extrinsic. The SDK waits for canonical finality, resumes scanning if
            a reorganization removed the observed inner call, and raises <code>ChainError</code>{' '}
            after bounded finality RPC failures or stalled-finality polling instead of hanging
            forever. See <DocLink href='/docs/concepts/advanced'>advanced submission</DocLink> for
            the full composition model.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Upgrade checklist</h2>
          <ul>
            <li>Upgrade nodes and metadata-generated clients to runtime spec 448.</li>
            <li>
              Let the bounded staking-index migrations finish before treating historical{' '}
              <code>TotalAlphaStaked</code> values as complete. Operators can confirm the backfill
              through <code>HasMigrationRun[b&quot;migrate_total_alpha_staked&quot;]</code>.
            </li>
            <li>
              Update cross-subnet stake automation for <code>move_stake_limit</code> and explicit
              partial-fill policy.
            </li>
            <li>
              Budget the 256-unit root-claim reserve even though unused work is refunded after
              dispatch.
            </li>
            <li>
              Treat linked-output records as single-use accounting with a seven-day lifetime, not as
              escrowed funds.
            </li>
          </ul>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
