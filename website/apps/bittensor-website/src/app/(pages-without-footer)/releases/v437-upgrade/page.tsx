import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'The V437 Upgrade — Key Lineage',
  description:
    'On-chain hotkey and coldkey swap lineage: follow identity across renames without ' +
    'an archive node. Successor and root maps, plus bonded keep_stake and coldkey ' +
    'collateral migration hardening.',
  alternates: {canonical: '/releases/v437-upgrade'},
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
          <p className={styles.paper_title}>The V437 Upgrade</p>
          <p className={styles.subtitle}>Key lineage</p>
        </section>

        <section className={styles.section}>
          <p>
            Builds on the{' '}
            <DocLink href='/releases/v436-upgrade'>v436 collateral release</DocLink>. Runtime{' '}
            <code>spec_version</code> 437 adds on-chain <strong>swap lineage</strong> so
            validators and indexers can follow hotkey and coldkey identity across renames
            without replaying archives — and hardens bonded key swaps so collateral cannot
            detach from stake.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Hotkey lineage</p>
          <p>
            Each successful hotkey swap on a subnet writes two maps:
          </p>
          <ul className={styles.list}>
            <li>
              <code>HotkeySuccessor(netuid, old) → new</code> — the rename edge
            </li>
            <li>
              <code>HotkeyRoot(netuid, hotkey) → root</code> — the first key in the chain
              (absent means the key is its own root)
            </li>
          </ul>
          <p>
            Helpers on the pallet (callable from runtime APIs / off-chain workers that
            already query storage):
          </p>
          <pre className={styles.code_block}>
            {`hotkey_root(netuid, hotkey) -> AccountId
same_hotkey_lineage(netuid, a, b) -> bool
hotkey_lineage_tip(netuid, hotkey) -> AccountId   // best-effort; prefer root for bans`}
          </pre>
          <p>
            Maps are <strong>per-subnet</strong>: a swap may move a UID on one netuid while
            the old hotkey stays registered elsewhere. Dissolution clears them. Re-registration
            of a previously swapped-away SS58 clears a stale outgoing successor so tip walks
            do not follow the old rename.
          </p>
          <p>
            Bonded miners may still rename with <code>keep_stake=false</code> (the bond
            migrates with the UID). <code>keep_stake=true</code> while any{' '}
            <code>MinerCollateral</code> remains fails with{' '}
            <DocLink href='/docs/errors/chain/KeepStakeBlockedByCollateral'>
              <code>KeepStakeBlockedByCollateral</code>
            </DocLink>
            — there is no validator-permit escape.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Coldkey lineage</p>
          <p>
            Coldkey swaps are global (ownership moves everywhere at once), so the maps are
            not keyed by netuid:
          </p>
          <ul className={styles.list}>
            <li>
              <code>ColdkeySuccessor(old) → new</code>
            </li>
            <li>
              <code>ColdkeyRoot(coldkey) → root</code>
            </li>
          </ul>
          <pre className={styles.code_block}>
            {`coldkey_root(coldkey) -> AccountId
same_coldkey_lineage(a, b) -> bool
coldkey_lineage_tip(coldkey) -> AccountId   // best-effort; prefer root for attribution`}
          </pre>
          <p>
            Written at the end of a successful <code>do_swap_coldkey</code>, inside the same
            storage transaction that migrates stake, ownership, locks, and miner collateral.
            Use root for owner-keyed policy; tip walks are advisory under key reuse.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <p>
            Operators should wait for on-chain <code>spec_version</code> 437, then upgrade
            nodes. Indexers and validators that ban or attribute by SS58 should:
          </p>
          <ol className={styles.list}>
            <li>
              Prefer <code>hotkey_root</code> / <code>HotkeyRoot</code> (and{' '}
              <code>coldkey_root</code> / <code>ColdkeyRoot</code>) over raw addresses when
              tracking identity across renames.
            </li>
            <li>
              Ingest <code>HotkeySuccessor</code>, <code>HotkeyRoot</code>,{' '}
              <code>ColdkeySuccessor</code>, and <code>ColdkeyRoot</code> storage (and keep
              handling <code>HotkeySwapped</code> / <code>ColdkeySwapped</code> events).
            </li>
            <li>
              Treat <code>hotkey_lineage_tip</code> / <code>coldkey_lineage_tip</code> as
              advisory — root is the ban/attribution key.
            </li>
          </ol>
          <p>
            Full collateral mechanics remain in the{' '}
            <DocLink href='/releases/v436-upgrade'>v436 release notes</DocLink> and the{' '}
            <DocLink href='/docs/guides/mining/collateral'>collateral guide</DocLink>.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/docs/guides/mining/collateral'>Read the collateral guide</Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
