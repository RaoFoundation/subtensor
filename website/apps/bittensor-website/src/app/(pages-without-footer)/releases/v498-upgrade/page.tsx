import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V498 Upgrade: Runtime Version Bump',
  description: 'Runtime spec 498 and what the version bump means for transaction signing.',
  alternates: {canonical: '/releases/v498-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V498 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Runtime version bump · Proposed upgrade
          </p>
        </section>

        <section className={styles.section} id='compatibility'>
          <h2 className={styles.subtitle}>What changes</h2>
          <p>
            V498 raises the runtime spec version from <strong>497</strong> to <strong>498</strong>.
            The transaction version stays at <strong>2</strong>. Relative to V497, this release adds
            no calls, storage migrations, parameter changes, or Python SDK changes.
          </p>
          <p>
            Once the upgrade activates, refresh the runtime metadata and rebuild signing payloads
            prepared under earlier versions. Regenerate and review any pending root parameter
            proposals before continuing approvals.
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
