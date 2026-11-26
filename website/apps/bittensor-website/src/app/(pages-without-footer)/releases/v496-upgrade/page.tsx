import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V496 Upgrade — Root Parameters and Short-Only Lending',
  description:
    'Runtime spec 496: reviewed root parameter proposals, plus short-only synthetic lending ' +
    'dark-launched with flat APR, whole-book coverage, TWAP health, and paced buy-backs.',
  alternates: {canonical: '/releases/v496-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V496 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Root parameter proposals · short-only lending · dark launch
          </p>
        </section>

        <section className={styles.section} id="compatibility">
          <h2 className={styles.subtitle}>What changes on chain</h2>
          <p>
            This PR raises the runtime spec version to <strong>496</strong>. The transaction version
            stays at <strong>2</strong>. Wallets and indexers should refresh their runtime metadata
            at activation. Signers should rebuild any proposals and signing payloads prepared under
            an older runtime.
          </p>
        </section>

        <section className={styles.section} id="root-parameters">
          <h2 className={styles.subtitle}>Root parameter proposals</h2>
          <p>
            The repository tracks 51 chain-wide root settings for devnet, testnet, and mainnet. CI
            compares reviewed file edits with finalized on-chain values and reports unrelated drift
            separately. Pull requests show a preview, and manual baseline comparisons are
            report-only. Only pushes to main can produce an approval command. Merging the files
            does not sign, submit, or apply a proposal. The nomination minimum factor is tracked
            for reporting only and cannot enter a generated batch.
          </p>
          <p>
            On mainnet, use the checked-in verifier and updated SDK from a trusted checkout.
            Independently reproduce the proposal and compare its call hash and runtime spec version
            before approving. The signer refuses to sign under a different runtime version. Only the
            designated initiator opens the operation; later signers approve its exact opening
            timepoint. The verifier checks live values before approval, but the existing sudo
            setters still use last-successful-write semantics at execution.
          </p>
          <p>
            See the{' '}
            <Link
              className={styles.inline_link}
              href="https://github.com/RaoFoundation/subtensor/tree/main/sudo#mainnet-triumvirate-review"
            >
              root parameter guide
            </Link>{' '}
            for operator instructions.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Short-only synthetic lending</h2>
          <p>
            Lending remains pallet <strong>34</strong> with <code>Enabled</code> defaulting to{' '}
            <code>false</code>. Positions are synthetic shorts only. Interest is a flat root-settable
            APR (default 20%), split 50/50 between burn and the residual reserve. Coverage uses
            whole-book collateral at <code>max(slow EMA, spot, 30-day ranking SMA)</code>. Health
            uses a median of interval means (TWAP ring). Forced closes spend the per-block pace
            budget across the queue.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Dissolution and dereg</h2>
          <p>
            On subnet dissolve, basket holdings convert first, then the settlement rate{' '}
            <code>r = min(T, T_ema) / all_alpha</code> freezes. Shorts settle at <code>r</code>{' '}
            (zone alpha at its own proceeds). Stakers are paid at most <code>r</code> per alpha,
            capped by the remaining pot; leftover pot TAO is recycled. Ranking stays dark until
            lending is enabled and each subnet has a full window of real daily samples.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What operators and integrators should do</h2>
          <ol className={styles.list}>
            <li>Regenerate chain metadata and signing payloads for spec 496.</li>
            <li>
              Treat lending as off until root enables it. Read live <code>SynthParamsStore</code>{' '}
              for APR, stress, pace, and TWAP knobs rather than hard-coding defaults.
            </li>
            <li>
              For root parameter proposals, follow the{' '}
              <Link
                className={styles.inline_link}
                href="https://github.com/RaoFoundation/subtensor/tree/main/sudo#mainnet-triumvirate-review"
              >
                mainnet triumvirate review
              </Link>{' '}
              guide.
            </li>
          </ol>
          <p>
            Related:{' '}
            <Link href="/releases/v495-upgrade" className={styles.inline_link}>
              V495
            </Link>
            .
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
