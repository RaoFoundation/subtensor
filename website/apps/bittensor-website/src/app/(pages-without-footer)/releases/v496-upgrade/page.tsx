import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V496 Upgrade — Short-Only Synthetic Lending',
  description:
    'V496 keeps short-only synthetic lending dark-launched: flat APR, whole-book coverage, ' +
    'TWAP health, paced buy-backs, and dereg settlement at a frozen post-basket rate.',
  alternates: {canonical: '/releases/v496-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V496 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Short-only lending · flat APR · whole-book coverage · TWAP health · dark launch
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Scope, versions, and compatibility</h2>
          <p>
            The runtime specification version is <strong>496</strong>. It sits above main tip 494
            and carries the short-only lending surface from the V495 dark launch. Lending remains
            pallet <strong>34</strong> with <code>Enabled</code> defaulting to <code>false</code>.
            Node operators, wallets, indexers, and SDK users must regenerate metadata for this
            runtime.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Short-only synthetic lending</h2>
          <p>
            Positions are synthetic shorts only. There is no long / lift path. Interest is a flat
            root-settable APR (default 20%), split 50/50 between burn and the residual reserve.
            Coverage uses whole-book collateral at{' '}
            <code>max(slow EMA, spot, 30-day ranking SMA)</code>. Health uses a median of interval
            means (TWAP ring). Forced closes spend the per-block pace budget across the queue.
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
