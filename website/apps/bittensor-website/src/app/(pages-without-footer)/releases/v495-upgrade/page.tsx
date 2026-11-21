import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V495 Upgrade — Lift Lending (Dark Launch)',
  description:
    'V495 adds Lift lending as a dark-launch pallet: escrowed short and long loans, ' +
    'root-settable LiftParams, force-at-market after max deferral, ProxyType::Lending, and a continuous thin-depth blend.',
  alternates: {canonical: '/releases/v495-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V495 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Lift lending · dark launch · bounded unwind · ProxyType::Lending · thin-depth blend
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Scope, versions, and compatibility</h2>
          <p>
            The runtime specification version is <strong>495</strong>. Spec 495 is chosen so this
            lending ship does not collide with the separate release-494 candidate (commit burns,
            weight delegation, basket controls, Etao). Remmerge after 494 lands on main and keep
            this tip strictly above main. Node operators should build the final tagged release when
            governance schedules it. Wallets, indexers, and SDK users must regenerate metadata for
            this runtime.
          </p>
          <p>
            Lending is pallet <strong>34</strong>. It is a <strong>dark launch</strong>:{' '}
            <code>Enabled</code> defaults to <code>false</code>. Opens and related extrinsics stay
            refused until root calls <code>set_enabled(true)</code> in a separate action. No
            activation time is implied here; any deployment or sudo action must be scheduled
            separately, in UTC.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Lift lending</h2>
          <p>
            Subnet participants can open escrowed short and long loans against pool depth under
            root-settable <code>LiftParams</code> (LTV, APR, capacity floor/ceiling, EMA band,
            duration, settlement drain). New opens and rolls use a hard{' '}
            <code>max_loan_duration</code> (default ~30 days). Increases do not extend{' '}
            <code>expires_at</code>; use <code>roll(netuid)</code> to renew the term. Economic knobs
            are live in <code>LiftParamsStore</code>, not compile-time Config seeds.
          </p>
          <p>
            Capacity floor and long LTV use a continuous thin-depth blend:{' '}
            <code>thin_tao</code> plus <code>thin_blend_tao</code> (default 3000 τ each). At or below
            thin depth the thin values apply; at or above thin + blend the full values apply; in
            between they interpolate. That removes the old step cliff at 3000 τ.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Expiry settlement and MEV bounds</h2>
          <p>
            Protocol-forced AMM settles (expiry, forfeit buybacks, paced interest sells) check
            realized cumulative TAO against the lending EMA within{' '}
            <code>expiry_max_slippage_bps</code> (default 2%). A breach rolls the settle back and
            defers. After <code>max_settlement_deferral</code> (default ~7 days from the first band
            deferral) the next expiry pass <strong>force-settles at market</strong>, bypassing that
            per-trade band, but still subject to the per-subnet per-block{' '}
            <code>ProtocolForcedImpact</code> cap — surplus claims wait for later blocks. Increases
            and rolls stay frozen until settled or the owner closes. Wallet repay remains
            unconstrained. With opens and rolls disabled, every position closes within the 30-day
            term plus max deferral plus a few drain blocks.
          </p>
          <p>
            Per-block expiry work splits the settle budget fairly between scheduled Expiry and
            ExpiryRetry so retries cannot starve under a sustained backlog. Settlement age is
            measured from <code>expires_at</code>, not from the first attempt block. Shorts that
            cannot buy back principal forfeit to the escrow/collateral limit (same interest-cap
            forfeit path).
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>ProxyType::Lending</h2>
          <p>
            Spec 495 appends <code>ProxyType::Lending</code> at index <strong>23</strong> so existing
            proxy indices stay stable. A Lending delegate may call only Lift lending user
            extrinsics (<code>open</code>, <code>open_exact</code>, <code>close</code>,{' '}
            <code>repay</code>, <code>roll</code>, <code>open_long_and_sell</code>,{' '}
            <code>warm_reference</code>) and Utility batch wrappers of those calls. Transfers and
            staking stay denied. <code>Any</code> still covers lending; <code>Staking</code> and{' '}
            <code>NonTransfer</code> do not.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What operators and integrators should do</h2>
          <ol className={styles.list}>
            <li>Regenerate chain metadata and signing payloads for spec 495.</li>
            <li>
              Treat lending as off until root enables it. Do not assume live capacity or LTV until
              then.
            </li>
            <li>
              Read live <code>LiftParamsStore</code> (for example via{' '}
              <code>btcli lending params</code>) rather than hard-coding defaults.
            </li>
            <li>
              After release-494 merges, expect a remmerge of this branch onto the new main tip
              before any joint activation plan.
            </li>
          </ol>
          <p>
            Spec and CLI notes live in the lending pallet docs. For website catalogue and SDK
            error maps, regenerate from this tree after the upgrade lands.
          </p>
          <p>
            Related:{' '}
            <Link href="/releases/v493-upgrade" className={styles.inline_link}>
              V493
            </Link>
            .
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
