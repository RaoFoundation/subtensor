import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V495 Upgrade — Synthetic Short Lending (Dark Launch)',
  description:
    'V495 adds the Lending pallet as a dark launch: synthetic short positions against subnet pools, ' +
    'root-settable SynthParams, dissolution settlement, and ProxyType::Lending.',
  alternates: {canonical: '/releases/v495-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V495 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Synthetic short lending · dark launch · dissolution hooks · ProxyType::Lending
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Scope, versions, and compatibility</h2>
          <p>
            The runtime specification version is <strong>495</strong>. Spec 495 is chosen so this
            lending ship does not collide with the separate release-494 candidate. Node operators
            should build the final tagged release when governance schedules it. Wallets, indexers,
            and SDK users must regenerate metadata for this runtime.
          </p>
          <p>
            Lending is pallet <strong>34</strong>. It is a <strong>dark launch</strong>:{' '}
            <code>Enabled</code> defaults to <code>false</code>. Opens and related extrinsics stay
            refused until root calls <code>set_enabled(true)</code> in a separate action.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Synthetic short lending</h2>
          <p>
            Subnet participants can open <strong>short-only</strong> synthetic positions: borrowed
            alpha is closed through the AMM without lifting pool liquidity. Economic knobs live in
            root-settable <code>SynthParams</code> (LTV, APR, stress curve, book share caps, TWAP
            samples for deleverage). Custody accounts hold collateral and proceeds off the public
            stake ledger until close, repayment, or subnet dissolution.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>ProxyType::Lending</h2>
          <p>
            Spec 495 appends <code>ProxyType::Lending</code> at index <strong>23</strong>. A Lending
            delegate may call only lending user extrinsics (<code>open_short</code>,{' '}
            <code>close_short</code>, <code>repay_short</code>, and related reads) plus Utility batch
            wrappers. Transfers and ordinary staking stay denied.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What operators and integrators should do</h2>
          <ol className={styles.list}>
            <li>Regenerate chain metadata and signing payloads for spec 495.</li>
            <li>
              Treat lending as off until root enables it. Read live <code>SynthParamsStore</code>{' '}
              rather than hard-coding defaults.
            </li>
          </ol>
          <p>
            Related:{' '}
            <Link href="/releases/v496-upgrade" className={styles.inline_link}>
              V496
            </Link>
            .
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}