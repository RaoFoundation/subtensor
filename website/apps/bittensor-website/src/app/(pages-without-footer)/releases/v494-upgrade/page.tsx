import FadeInWrapper from '@/app/components/FadeInWrapper';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V494 Upgrade',
  description: 'The next Bittensor runtime release, currently in preparation.',
  alternates: {canonical: '/releases/v494-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V494 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Release in preparation
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Release notes</h2>
          <p>
            V494 is the next Bittensor runtime release. Its final feature, integration, and upgrade
            notes will be completed on the release branch before the upgrade is proposed.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
