import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V496 Upgrade — Synth Short Book Caps and Stress APR',
  description:
    'V496 keeps synthetic short lending dark-launched and adds pool share caps, stress-based short APR, ' +
    'TWAP deleverage, and dereg-ranking hooks for dissolution.',
  alternates: {canonical: '/releases/v496-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V496 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Synth short lending · book share caps · stress APR · TWAP deleverage · dark launch
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
          <h2 className={styles.subtitle}>Book share caps</h2>
          <p>
            Outstanding synthetic short alpha must stay at or below the configured share of pool
            alpha (<code>SubnetAlphaIn</code>). The gate runs on open and increases; it does not
            force-close live shorts. Root sets bounds via <code>SynthParams</code>.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Stress short APR</h2>
          <p>
            Short interest scales with utilization and a stress multiplier derived from TWAP vs spot.
            Root can tune base APR, stress slope, and book caps. Interest accrual checkpoints use
            the live curve for all open shorts.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Dissolution and dereg hooks</h2>
          <p>
            Lending exposes <code>start_dissolution</code>, metered short settlement into the staker
            pot, and <code>finish_dissolution</code> for subnet teardown. A 30-day SMA of spot feeds
            dereg ranking; pool-TAO EMA supports dissolution payout math.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What operators and integrators should do</h2>
          <ol className={styles.list}>
            <li>Regenerate chain metadata and signing payloads for spec 496.</li>
            <li>
              Treat lending as off until root enables it. Expect short APR to move with utilization
              and stress after enablement.
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