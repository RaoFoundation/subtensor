import FadeInWrapper from '@/app/components/FadeInWrapper';
import {ExplorerGrid} from '@/app/components/ExplorerGrid/ExplorerGrid';
import {learnResources} from '@/data/learn';
import type {Metadata} from 'next';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'Explore — block explorers and learning resources',
  description:
    'Commonly used Bittensor block explorers, subnet explorers, and educational resources ' +
    'for the TAO ecosystem.',
  alternates: {canonical: '/explore'},
};

export default function ExplorePage() {
  return (
    <FadeInWrapper className={styles.page}>
      <div className={styles.content}>
        <section className={styles.section} aria-labelledby="explore-heading">
          <div className={styles.titleBlock}>
            <p id="explore-heading" className={styles.title}>
              Bittensor Explorers
            </p>
            <p className={styles.subtitle}>
              THESE ARE COMMONLY USED THIRD-PARTY BLOCKCHAIN AND SUBNET EXPLORERS FOR THE BITTENSOR
              ECOSYSTEM. WE DO NOT CONTROL, AUDIT, OR GUARANTEE THEIR SECURITY, COMPATIBILITY, OR
              SUITABILITY FOR YOUR USE CASE.
            </p>
          </div>
          <ExplorerGrid />
        </section>
        <section className={styles.section} aria-labelledby="learn-heading">
          <div className={styles.titleBlock}>
            <p id="learn-heading" className={styles.title}>
              Learn
            </p>
            <p className={styles.subtitle}>
              BITTENSOR DOCS IS THE OFFICIAL DOCUMENTATION FOR THE BITTENSOR ECOSYSTEM. THE OTHER
              RESOURCES LISTED ARE COMMONLY USED THIRD-PARTY EDUCATIONAL MATERIALS. WE DO NOT
              CONTROL, AUDIT, OR GUARANTEE THEIR ACCURACY, COMPLETENESS, OR SUITABILITY FOR YOUR
              USE CASE.
            </p>
          </div>
          <ExplorerGrid cards={learnResources} />
        </section>
      </div>
    </FadeInWrapper>
  );
}
