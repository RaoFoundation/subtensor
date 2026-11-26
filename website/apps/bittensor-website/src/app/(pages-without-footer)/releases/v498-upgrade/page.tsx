import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V498 Upgrade: Gamma Balances and Transfers',
  description: 'Runtime spec 498 lets coldkeys hold gamma outside a deposit and send it to each other.',
  alternates: {canonical: '/releases/v498-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V498 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Gamma balances and transfers · Proposed upgrade
          </p>
        </section>

        <section className={styles.section} id='compatibility'>
          <h2 className={styles.subtitle}>What changes</h2>
          <p>
            V498 lets a coldkey hold gamma outside a deposit and send it to other coldkeys. A mint
            with no hotkey credits the signer&apos;s gamma balance on the subnet, which no key can
            spend until it is moved into a deposit. <code>deposit_gamma</code> and{' '}
            <code>withdraw_gamma</code> move gamma between a balance and a deposit, and{' '}
            <code>transfer_gamma</code> sends an exact amount from a balance or deposit to another
            coldkey&apos;s balance or deposit. Another coldkey can open a deposit for a coldkey only
            while it holds fewer than 32 across subnets and has no coldkey swap underway.
          </p>
          <p>
            <code>mint_gamma</code> and <code>mint_gamma_with_stake</code> take an optional hotkey
            and move from call indices 166 and 167 to 178 and 179. The old indices are retired, so
            update the Python SDK before minting. The release adds the <code>GammaBalances</code>{' '}
            storage item and moves <code>GammaRuntimeApi</code> to version 3 with{' '}
            <code>get_gamma_balances</code>. It has no storage migrations or parameter changes.
          </p>
          <p>
            V498 raises the runtime spec version from <strong>497</strong> to <strong>498</strong>.
            The transaction version stays at <strong>2</strong>. Once the upgrade activates, refresh
            the runtime metadata and rebuild signing payloads prepared under earlier versions.
            Regenerate and review any pending root parameter proposals before continuing approvals.
          </p>
          <p>
            See the{' '}
            <Link className={styles.inline_link} href='/releases/v497-upgrade'>
              V497 release notes
            </Link>{' '}
            for the Python SDK release and the sudo signing safeguards carried forward.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
