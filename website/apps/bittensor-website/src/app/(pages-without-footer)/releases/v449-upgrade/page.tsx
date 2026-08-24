import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V449 Upgrade — Root Weights Open',
  description:
    'V449 enables set_root_weights network-wide: root validators can now curate their ' +
    'dividend baskets, bounded by a new concentration cap that keeps every basket spread ' +
    'across at least 16 destinations.',
  alternates: {canonical: '/releases/v449-upgrade'},
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
          <h1 className={styles.paper_title}>The V449 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Root weights open · August 2026
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            <DocLink href='/releases/v441-upgrade'>Root Reborn</DocLink> shipped validator-curated
            baskets with the weight-setting switch off: every fund launched on the null strategy,
            dividends accumulating in place on the subnet they arrive on. Spec{' '}
            <strong>449</strong> flips that switch. From the upgrade block, every root validator
            can call <code>set_root_weights</code> and decide how its dividend stream is deployed
            across subnet alpha.
          </p>
          <p>
            Curation opens with a guardrail. A new <code>RootWeightsCap</code> hyperparameter
            bounds how concentrated a basket vector may be: no single destination may take more
            than 1/16 of the vector at launch, so an active basket spreads across at least 16
            destinations. Root validators steer allocation; no fund becomes a leveraged bet on one
            subnet on day one.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What changed on chain</h2>
          <p>
            The upgrade migration sets <code>RootWeightSettingEnabled</code> to true and writes{' '}
            <code>RootWeightsCap</code> at 4096/65535 (1/16). The cap is checked inside{' '}
            <code>set_root_weights</code> on the submitted values: a vector where any
            destination&apos;s share of the summed weights exceeds the cap is rejected with the
            new <code>RootWeightCapExceeded</code> error. Vectors stored before the upgrade are
            untouched; the cap applies when a validator next sets weights.
          </p>
          <p>
            The cap is governance-adjustable through a new root-only extrinsic,{' '}
            <code>AdminUtils::sudo_set_root_weights_cap</code>, and the enable switch remains
            reversible through <code>sudo_set_root_weight_setting_enabled</code>. On chains with
            fewer destinations than the cap demands (a fresh localnet, for example) the check is
            skipped, mirroring how the existing 8-destination diversity floor softens.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Registering into the root network</h2>
          <p>
            Basket curation is for root validators: hotkeys registered on netuid 0. Registration
            is burn-based — the coldkey pays the current root registration price, no prior stake
            is required — but root seats are limited: a full root network evicts the lowest-staked
            member, so a seat is held by keeping stake behind the hotkey.
          </p>
          <pre className={styles.code_block}>
            {`btcli subnets burn-cost 0                          # current root registration price
btcli subnets register --netuid 0 -w my_wallet     # register the wallet hotkey on root
btcli stake add --netuid 0 --amount 1000 -w my_wallet   # stake TAO behind the seat`}
          </pre>
          <p>
            The hotkey must also clear the minimum stake to set weights before its first vector is
            accepted. See the <DocLink href='/docs/guides/root-reborn'>Root Reborn guide</DocLink>{' '}
            for how seats, dividends, and baskets fit together.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Setting basket weights with btcli</h2>
          <p>
            Weights are relative <code>netuid:weight</code> pairs. Netuid 0 is a valid destination
            — that share is held as TAO (root stake) instead of subnet alpha. With the launch cap,
            a vector needs at least 16 positive destinations and no destination above 1/16 of the
            total. An equal 16-way split sits exactly at the cap.
          </p>
          <pre className={styles.code_block}>
            {`# 16 destinations, equal weight each (share 1/16 = at the cap):
btcli root set-weights -w my_wallet \\
  --weights "0:1,1:1,3:1,4:1,5:1,8:1,9:1,11:1,13:1,19:1,21:1,23:1,34:1,51:1,64:1,77:1"

btcli root get-weights --hotkey 5F...              # read a validator's stored vector`}
          </pre>
          <p>
            A too-concentrated vector fails during planning with the exact rule the chain
            enforces, before anything is signed; a raw submission that bypasses the SDK fails at
            dispatch with <code>RootWeightCapExceeded</code>. The existing checks still apply:
            at least 8 positive destinations, every destination netuid 0 or an existing subnet,
            the root weights rate limit, and the minimum stake to set weights. Validators that
            never set a vector stay on the null strategy — dividends keep accumulating in place,
            trade-free.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>SDK and tooling</h2>
          <p>
            SDK 11.3.0 ships alongside the runtime: regenerated metadata bindings expose{' '}
            <code>RootWeightsCap</code> and the new admin extrinsic, <code>SetRootWeights</code>{' '}
            preflights the concentration cap client-side on the exact quantized values it will
            submit, and the error taxonomy classifies <code>RootWeightCapExceeded</code> with an
            actionable description. <code>RootWeightSettingDisabled</code> now indicates
            governance switched curation back off, not the launch gate.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
