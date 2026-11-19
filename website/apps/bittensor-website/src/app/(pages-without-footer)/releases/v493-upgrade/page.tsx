import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V493 Upgrade — Gamma, Commit Burns, Delegation, Basket Controls, and Etao',
  description:
    'V493 changes gamma spending to the deposit hotkey, adds optional commitment burns and ' +
    'mechanism weight delegation, preserves basket defaults, and makes Etao updates per-tempo.',
  alternates: {canonical: '/releases/v493-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V493 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Gamma hotkey spending, commit burns, weight delegation, basket controls, and Etao
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Transaction and call compatibility</h2>
          <p>
            The runtime specification version is 493 and the transaction version is 2. The
            transaction-version bump is required because gamma removes and renumbers existing
            calls. Signers, wallets, indexers, and other integration partners must regenerate their
            metadata and use transaction version 2 when signing for this runtime. The existing
            gamma deposit, service, and runtime API record formats change as described below.
          </p>
          <p>
            SubtensorModule calls at 166–169 are <code>mint_gamma</code>,{' '}
            <code>mint_gamma_with_stake</code>, <code>spend_gamma</code>, and <code>set_service</code>.
            Calls at 170–174 are <code>set_mech_weight_delegate</code>,{' '}
            <code>set_mechanism_weights_for</code>, <code>commit_mechanism_weights_for</code>,{' '}
            <code>reveal_mechanism_weights_for</code>, and{' '}
            <code>commit_timelocked_mechanism_weights_for</code>. Indices 158, 159, 162, 164, and 165
            are retired. The old <code>set_gamma_deposit_spender</code> call and the batched spender
            call are removed.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Gamma hotkey spending and migration</h2>
          <p>
            A deposit&apos;s hotkey now signs a fee-free spend at a service on its subnet. The amount
            is burned and recorded in <code>GammaSpent(netuid, service, hotkey)</code>, which starts
            at zero. Services read that total to credit spending and cannot spend the deposit.
            The coldkey keeps ownership of the balance, daily cap, and closing. A daily cap is zero,
            which pauses spending, or at least 0.1 gamma. A <code>GammaSpend</code> proxy is
            deprecated and cannot spend.
          </p>
          <p>
            The <code>migrate_drop_gamma_spenders</code> migration removes the spender from existing
            deposits and services. It pauses the subnet owner&apos;s uncapped deposit at the owner
            hotkey and any deposit with a nonzero cap below 0.1 gamma, so the owner coldkey must set
            a new cap before those hotkeys can spend. Existing deposits move with a coldkey swap&apos;s
            hotkey change. Gamma emission opens the owner hotkey&apos;s deposit with a zero cap.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Optional commitment burns and weight delegation</h2>
          <p>
            Commitments adds <code>set_commitment_to</code> at call index 3. It publishes a
            per-channel commitment with the coldkey paying an optional burn. Missing configuration
            means a free channel by default. AdminUtils burn setters are at 123–127: enabled, minimum
            burn, maximum burn, half-life, and increase multiplier. The default channel
            configuration is disabled with zero burn, floor, and ceiling, a one-block half-life,
            and a multiplier of one.
          </p>
          <p>
            Validators can delegate direct weights, hash commit/reveal, and timelocked commits for
            an exact subnet and mechanism. The delegate pays transaction fees, while authorization,
            UID state, permits, and rate limits remain those of the validator. Missing delegation
            records authorize nobody.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Basket governance and Etao rate limit</h2>
          <p>
            Basket controls are sudo-settable through AdminUtils 128–130: trade band, refill
            blocks, and price reference. The upgrade preserves live behavior with{' '}
            <code>StrictestEmaAndSpot</code>, a 200-bps band, and a 7200-block refill. A separate
            governance choice can select spot-only execution.
          </p>
          <p>
            AdminUtils 131 sets the Etao owner-update interval in blocks. The new{' '}
            <code>SubnetEmissionWeightsRateLimit</code> storage defaults to 360 blocks, once per
            default tempo, and zero disables the limit. The limit remains per subnet; root is
            exempt and a subnet&apos;s first owner call is always allowed.
          </p>
          <p>
            AdminUtils 122 sets the Yuma timelocked commit cap. Its default remains 10 per hotkey,
            mechanism, and commit epoch; zero pauses new commits while existing commitments can
            reveal.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Post-merge sudo calls</h2>
          <p>
            No basket setter is needed to preserve existing behavior. To select the proposed
            configuration, submit these separate calls in this order:
          </p>
          <ol>
            <li><code>AdminUtils::sudo_set_basket_trade_max_slippage_bps(500)</code>.</li>
            <li><code>AdminUtils::sudo_set_basket_trade_price_reference(SpotOnly)</code>.</li>
            <li><code>AdminUtils::sudo_set_basket_concentration_cap(9830)</code>.</li>
          </ol>
          <p>
            <code>SpotOnly</code> drops the slow and fast EMA anchors, so a manipulated spot price
            within the configured band can be traded against. This is a deliberate sudo step, not
            the upgrade default.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Release operations and build</h2>
          <p>
            Release workflows announce proposed and live mainnet upgrades and stable Python
            releases on Discord. Stable Python packages publish through the mainnet release
            watcher; there is no separate standalone stable-publish workflow. Structured notes and
            a release page are required for future runtime and Python version bumps.
          </p>
          <p>
            Once the release train creates the v493 tag, run these commands from an existing
            Subtensor checkout to build that revision. The{' '}
            <Link
              href='https://github.com/RaoFoundation/subtensor/releases/tag/v493'
              className={styles.inline_link}
            >
              GitHub release
            </Link>{' '}
            identifies whether the upgrade is proposed or final.
          </p>
          <pre className={styles.code_block}>
            <code>{`git fetch https://github.com/RaoFoundation/subtensor.git tag v493
git checkout --detach v493
cargo build --release --locked -p node-subtensor`}</code>
          </pre>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
