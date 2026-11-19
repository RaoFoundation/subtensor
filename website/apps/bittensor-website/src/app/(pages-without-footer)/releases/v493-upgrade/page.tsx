import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V493 Upgrade — Yuma Commit Limits and Release Announcements',
  description:
    'V493 adds sudo-configurable Yuma timelocked commit limits, Discord release announcements, ' +
    'and standalone stable Python publishing.',
  alternates: {canonical: '/releases/v493-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V493 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Yuma commit limits and release announcements
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Configurable Yuma commit limits</h2>
          <p>
            Sudo can set <code>MaxTimelockedCommitsPerHotkey</code> for each subnet through{' '}
            <code>sudo_set_max_timelocked_commits_per_hotkey</code>. The limit applies to each
            hotkey, mechanism, and commit epoch on Yuma subnets. The default remains 10. Setting the
            limit to zero pauses new timelocked commits; commitments already queued remain eligible
            to reveal. Legacy hash commits retain their fixed limit of 10, and Null consensus keeps
            its existing queue limits.
          </p>
          <p>
            Existing subnets use the default without a storage migration. See{' '}
            <Link href='/docs/hyperparameters/commit-reveal-period' className={styles.inline_link}>
              commit and reveal timing
            </Link>{' '}
            and{' '}
            <Link
              href='https://github.com/RaoFoundation/subtensor/pull/3340'
              className={styles.inline_link}
            >
              PR #3340
            </Link>
            .
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Release announcements and Python publication</h2>
          <p>
            Release workflows announce proposed and live mainnet upgrades and stable Python releases
            on Discord. Successful posts are recorded as commit statuses so retries can skip them. A
            separate workflow publishes stable Python packages without a runtime upgrade. Future
            runtime and Python version bumps require structured notes linked to a release page. See{' '}
            <Link
              href='https://github.com/RaoFoundation/subtensor/pull/3342'
              className={styles.inline_link}
            >
              PR #3342
            </Link>
            .
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Build the release</h2>
          <p>
            Once the release train creates the v493 tag, run these commands from an existing
            Subtensor checkout to build that revision. The{' '}
            <Link
              href='https://github.com/RaoFoundation/subtensor/releases/tag/v493'
              className={styles.inline_link}
            >
              GitHub release
            </Link>{' '}
            identifies whether the upgrade is proposed or final.
          </p>
          <pre className={styles.code_block}>
            <code>{`git fetch https://github.com/RaoFoundation/subtensor.git tag v493
git checkout --detach v493
cargo build --release --locked -p node-subtensor`}</code>
          </pre>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
