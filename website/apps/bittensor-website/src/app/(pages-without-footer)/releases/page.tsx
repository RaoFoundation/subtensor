import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'Releases',
  description:
    'Bittensor network releases: every runtime upgrade with what changed, why it matters, ' +
    'and what to do about it.',
  alternates: {canonical: '/releases'},
};

type Release = {
  tag: string;
  date: string;
  title: string;
  summary: string;
  href: string;
};

// Newest first. Add new releases to the top.
const releases: Release[] = [
  {
    tag: 'v430',
    date: 'July 2026',
    title: 'The V430 Upgrade',
    summary:
      'Conviction-based subnet ownership, price-driven emissions, the bittensor v11 SDK ' +
      'with a Rust core, Ledger and browser-extension signing, and a verifiable upgrade ' +
      'pipeline — the monorepo era.',
    href: '/releases/v430-upgrade',
  },
];

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>Releases</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Network upgrades, in order
          </p>
        </section>
        <section className={styles.section} style={{width: '100%'}}>
          <div className={styles.release_list}>
            {releases.map((release) => (
              <Link key={release.tag} href={release.href} className={styles.release_item}>
                <span className={styles.release_meta}>
                  <span className={styles.release_tag}>{release.tag}</span>
                  <span className={styles.release_date}>{release.date}</span>
                </span>
                <span className={styles.release_title}>{release.title}</span>
                <p className={styles.release_summary}>{release.summary}</p>
              </Link>
            ))}
          </div>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
