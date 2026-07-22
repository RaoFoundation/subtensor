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
    tag: 'v437',
    date: 'July 2026',
    title: 'Key Lineage',
    summary:
      'On-chain hotkey and coldkey swap lineage (Successor + Root maps and helpers) so ' +
      'validators and indexers can follow identity across renames without archives — plus ' +
      'bonded keep_stake and coldkey collateral migration hardening on top of v436.',
    href: '/releases/v437-upgrade',
  },
  {
    tag: 'v436',
    date: 'July 2026',
    title: 'The Collateral Release',
    summary:
      'Miner registration collateral: subnets can lock a share of the registration price as ' +
      'a bond hotkeys earn back through emission — sunk for sybils and cheaters, nearly free ' +
      'for honest operators. Plus one-call stake transfer to a new coldkey and hotkey, ' +
      'air-gapped Polkadot Vault signing, and fully benchmarked extrinsic weights.',
    href: '/releases/v436-upgrade',
  },
  {
    tag: 'v431',
    date: 'July 2026',
    title: 'The Monorepo Release',
    summary:
      'Conviction-based subnet ownership, price-driven emissions, the bittensor v11 SDK ' +
      'with a Rust core, Ledger and browser-extension signing, and a verifiable upgrade ' +
      'pipeline — the chain, SDK, CLI, and docs developed and released together.',
    href: '/releases/v431-upgrade',
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
