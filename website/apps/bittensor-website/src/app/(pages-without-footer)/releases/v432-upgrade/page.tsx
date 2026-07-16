import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'The V432 Upgrade',
  description:
    'Follow-on runtime ship after v431: privilege-aware SDK intents and docs, root emission ' +
    'controls, nested sudo/proxy/multisig failure reporting, and the localnet publication fix.',
  alternates: {canonical: '/releases/v432-upgrade'},
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
          <p className={styles.paper_title}>The V432 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Spec <strong>432</strong> is the follow-on ship after{' '}
            <DocLink href='/releases/v431-upgrade'>v431</DocLink>. Devnet and testnet already
            run 431; this bump moves the same economic surface forward so the release train can
            redeploy and propose the remaining SDK, documentation, and CI fixes onto every
            network — including mainnet, which is still catching up from earlier specs.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What changed</p>
          <ul className={styles.list}>
            <li>
              Intent privilege is first-class: docs and the CLI surface{' '}
              <code>signed</code> / <code>subnet_owner</code> / <code>root</code> origins, and
              root calls are wrapped in <code>Sudo.sudo</code> at execute time.
            </li>
            <li>
              Nested dispatch failures inside Sudo, Proxy, and Multisig wrappers are reported as
              failures (no more false <code>is_success</code> when only the outer extrinsic
              succeeded).
            </li>
            <li>
              Sync client teardown and docs search focus fixes; localnet publication now pins
              smoke-tested multi-arch digests.
            </li>
            <li>
              Regenerated transaction docs and catalogs match the privilege labels agents and
              operators see in help output.
            </li>
          </ul>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <p>
            Operators should treat this like any other runtime upgrade: wait for the on-chain{' '}
            <code>spec_version</code> to move to 432, then upgrade nodes and clients. SDK users
            should pull the matching bittensor release once the train publishes it. Economic
            rules from v431 (conviction ownership, price-driven emissions) are unchanged —
            see the <DocLink href='/releases/v431-upgrade'>v431 notes</DocLink> for those.
          </p>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v432 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
