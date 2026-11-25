import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V497 Upgrade: Runtime and Python SDK',
  description: 'Runtime spec 497 and Python SDK 11.3.7 with pinned sudo approvals.',
  alternates: {canonical: '/releases/v497-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V497 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Runtime spec 497 · Python SDK 11.3.7
          </p>
        </section>

        <section className={styles.section} id='compatibility'>
          <h2 className={styles.subtitle}>What changes</h2>
          <p>
            V497 raises the runtime spec version from <strong>496</strong> to <strong>497</strong>.
            The transaction version stays at <strong>2</strong>. Relative to V496, this release adds
            no calls, storage migrations, or parameter changes.
          </p>
          <p>
            Once the upgrade activates, refresh the runtime metadata and rebuild signing payloads
            prepared under earlier versions. Regenerate and review any pending root parameter
            proposals before continuing approvals.
          </p>
          <p>
            See the{' '}
            <Link className={styles.inline_link} href='/releases/v496-upgrade'>
              V496 release notes
            </Link>{' '}
            for the root parameter proposal workflow.
          </p>
        </section>

        <section className={styles.section} id='python-sdk'>
          <h2 className={styles.subtitle}>Python SDK 11.3.7</h2>
          <p>
            This package release includes the V496 signing safeguards. Raw multisig approvals can
            pin the expected call hash, runtime spec version, and operation timepoint. The sudo
            parameter countersigner uses these checks to refuse mismatched approvals. Sync and async
            snapshots also expose the runtime spec version at their pinned block.
          </p>
          <p>
            After the stable packages are published, use <code>bittensor==11.3.7</code> for the
            updated countersigner. <code>bittensor-core</code> 0.1.14 accompanies this release with
            no native code changes.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
