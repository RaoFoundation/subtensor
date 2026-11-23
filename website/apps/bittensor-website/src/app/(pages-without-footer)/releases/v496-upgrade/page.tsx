import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V496 Upgrade — Lift Lending Book Caps and Utilization APR',
  description:
    'V496 keeps Lift lending dark-launched and adds short/long book pool shares, ' +
    'utilization-based short APR, Cap_short 1×, add_collateral, roll adequacy, and forced buyback progress.',
  alternates: {canonical: '/releases/v496-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V496 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Lift lending · book shares · utilization short APR · Cap_short 1× · dark launch
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Scope, versions, and compatibility</h2>
          <p>
            The runtime specification version is <strong>496</strong>. It sits above main tip 494
            and supersedes the lending dark-launch surface introduced for 495. Lending remains
            pallet <strong>34</strong> with <code>Enabled</code> defaulting to <code>false</code>.
            Opens stay refused until root enables the pallet in a separate action. Node operators,
            wallets, indexers, and SDK users must regenerate metadata for this runtime.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Book caps (Jacob)</h2>
          <p>
            Outstanding short borrowed alpha must stay at or below{' '}
            <code>short_book_max_pool_alpha_bps</code> (default <strong>15%</strong>, sudo 1–50%) of
            pool alpha (<code>SubnetAlphaIn</code>). Outstanding long borrowed TAO must stay at or
            below <code>long_book_max_pool_tao_bps</code> (same default and bounds) of{' '}
            <code>SubnetTAO</code>. Both gates run only on open, increase, and rolls that grow debt;
            they never force-close live loans. Cap_short returns to a <strong>1×</strong> TAO bound
            (<code>split_short_bps = 5000</code>). Cap_long remains an additional long-side bound.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Utilization short APR</h2>
          <p>
            Short interest tracks book utilization{' '}
            <code>U = short_sold_alpha / (short_book_max_pool_alpha_bps · pool_alpha)</code>. The
            default kinked curve is <strong>20% APR</strong> flat to U = 0.5, then linear to{' '}
            <strong>100% APR</strong> at U = 1. Root can set base, kink, kink utilization, and max
            (max ≤ 50_000 bps) with a positive monotonic curve. The live rate applies at every
            accrual checkpoint for all open shorts (O(1) from book counters). Interest is still
            burned.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Also in this tip</h2>
          <p>
            Owner <code>add_collateral</code>, roll collateral adequacy, spot-relative forced
            buyback progress with a minimum floor, and the ProxyType::Lending / force-at-market
            surface from the lending tip remain part of this runtime. Read live{' '}
            <code>LiftParamsStore</code> (for example via <code>btcli lending params</code>) rather
            than hard-coding defaults.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What operators and integrators should do</h2>
          <ol className={styles.list}>
            <li>Regenerate chain metadata and signing payloads for spec 496.</li>
            <li>
              Treat lending as off until root enables it. Do not assume live capacity, LTV, or APR
              until then.
            </li>
            <li>
              Expect short APR to move with utilization after enablement; quotes that show the base
              coupon are only the U = 0 reference.
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
