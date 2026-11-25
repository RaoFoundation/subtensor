import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V496 Upgrade: Root Parameter Proposals',
  description:
    'Runtime spec 496 and a reviewed, read-only workflow for proposing chain-wide root parameter changes.',
  alternates: {canonical: '/releases/v496-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V496 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Root parameter proposals · Proposed upgrade
          </p>
        </section>

        <section className={styles.section} id='compatibility'>
          <h2 className={styles.subtitle}>What changes on chain</h2>
          <p>
            This PR raises the runtime spec version to <strong>496</strong>. The transaction version
            stays at <strong>2</strong>. The PR adds no runtime calls or storage migrations and
            does not itself change any parameter values. No activation date or separate Python
            package release is announced here.
          </p>
          <p>
            Wallets and indexers should refresh their runtime metadata at activation. Signers should
            rebuild any proposals and signing payloads prepared under an older runtime.
          </p>
        </section>

        <section className={styles.section} id='root-parameters'>
          <h2 className={styles.subtitle}>Root parameter proposals</h2>
          <p>
            The repository tracks 51 chain-wide root settings for devnet, testnet, and mainnet. CI
            compares reviewed file edits with finalized on-chain values and reports unrelated drift
            separately. Pull requests show a preview, not an approval command. Merging the files
            does not sign, submit, or apply a proposal. The nomination minimum factor is tracked
            for reporting only and cannot enter a generated batch.
          </p>
          <p>
            On mainnet, use the checked-in verifier and updated SDK from a trusted checkout.
            Independently reproduce the proposal and compare its call hash before approving. Only
            the designated initiator opens the operation; later signers approve its exact opening
            timepoint. The verifier checks live values before approval, but the existing sudo
            setters still use last-successful-write semantics at execution.
          </p>
          <p>
            See the{' '}
            <Link
              className={styles.inline_link}
              href='https://github.com/RaoFoundation/subtensor/tree/main/sudo#mainnet-triumvirate-review'
            >
              root parameter guide
            </Link>{' '}
            for operator instructions and the{' '}
            <Link className={styles.inline_link} href='/releases/v495-upgrade'>
              V495 release notes
            </Link>{' '}
            for the previous runtime changes.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
