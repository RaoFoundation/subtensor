import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V497 Upgrade: Runtime Version Bump',
  description: 'Runtime spec 497 and what the version bump means for transaction signing.',
  alternates: {canonical: '/releases/v497-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V497 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Runtime version bump · Proposed upgrade
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
      </FadeInWrapper>
    </Suspense>
  );
}
