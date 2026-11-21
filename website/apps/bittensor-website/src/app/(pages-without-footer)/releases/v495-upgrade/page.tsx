import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V495 Upgrade — Null Consensus with up to 16 Mechanisms',
  description:
    'V495 proposes Null UID capacity independent of mechanism count, a shared per-mechanism ' +
    'weight-row budget, and root-configurable support for up to 16 mechanisms.',
  alternates: {canonical: '/releases/v495-upgrade'},
};

const DocLink = ({href, children}: {href: string; children: React.ReactNode}) => (
  <Link href={href} className={styles.inline_link}>
    {children}
  </Link>
);

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V495 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Null consensus · up to 16 mechanisms · shared weight-row budget · Proposed upgrade
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Scope, versions, and compatibility</h2>
          <p>
            The proposed runtime specification version is <strong>495</strong> and the transaction
            version remains <strong>2</strong>. This page describes a proposed upgrade, not an
            activation time or deployment. Node operators should build the final tag only after the
            release process and governance select it. Wallets, indexers, SDK users, and other
            signers should refresh runtime metadata and must not reuse a signed payload prepared for
            a different runtime version.
          </p>
          <p>
            V495 adds the SubtensorModule error <code>TooManyWeightsForNullMechanism</code> for a
            Null weight row that exceeds its derived limit. It adds no dispatchable, AdminUtils call
            index, sudo storage value, cap event, or migration. Existing mechanism storage continues
            to use <code>netuid + mecid * 4096</code>.
          </p>
        </section>

        <section className={styles.section} id='capacity'>
          <h2 className={styles.subtitle}>Null UID capacity independent of mechanism count</h2>
          <p>
            Null consensus allows <code>MaxAllowedUids</code> up to <strong>2,500</strong>,
            independently of the subnet&apos;s mechanism count. Entering or remaining on Yuma keeps
            the existing product rule: <code>MaxAllowedUids * mechanism_count &lt;= 256</code>.
            Switching from Yuma to Null therefore does not clamp the UID maximum merely because a
            subnet has multiple mechanisms.
          </p>
          <p>
            The global maximum mechanism count still defaults to <strong>2</strong>. Root can raise
            it through the existing <code>sudo_set_max_mechanism_count</code> call to the existing
            hard maximum of <strong>16</strong>, then configure a subnet&apos;s mechanism count
            through the existing subnet-level call. This upgrade does not itself change those
            settings or enable 16 mechanisms on a live subnet; those remain separate governance
            actions.
          </p>
        </section>

        <section className={styles.section} id='weight-budget'>
          <h2 className={styles.subtitle}>A shared 2,500-entry Null weight-row budget</h2>
          <p>
            Each Null mechanism&apos;s new weight row is limited to{' '}
            <code>max(1, floor(2500 / current_mechanism_count))</code> entries. The resulting cap is
            2,500 with one mechanism, 1,250 with two, and 156 with sixteen. One-mechanism Null
            consequently retains its existing effective full-row behavior. Yuma receives no
            additional row cap.
          </p>
          <table>
            <thead>
              <tr>
                <th>Mechanisms</th>
                <th>Maximum new Null weight-row entries</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>1</td>
                <td>2,500</td>
              </tr>
              <tr>
                <td>2</td>
                <td>1,250</td>
              </tr>
              <tr>
                <td>16</td>
                <td>156</td>
              </tr>
            </tbody>
          </table>
          <p>
            Direct and per-mechanism weight setting, delegated <code>*_for</code> calls, legacy hash
            commits and reveals, timelocked commits and reveals, and batch operations all enforce
            the derived limit. Opaque commits cannot be checked by row length until reveal. An
            oversized automatic timelocked reveal is skipped without panicking or stalling the block
            hook. Existing oversized stored rows remain intact when mechanism counts change; the
            derived limit applies to new writes. A non-self Null row&apos;s effective minimum length
            is bounded by the derived cap and registered UID count, so an existing{' '}
            <code>MinAllowedWeights</code> value above the cap cannot make every row invalid. The
            stored setting is not rewritten, and Yuma retains its existing minimum-row rule.
          </p>
          <p>
            There is no configurable fixed cap and no new sudo setter. Validators, delegates, and
            SDK integrations should derive the current row limit from the subnet&apos;s live
            mechanism count. See the{' '}
            <DocLink href='/docs/guides/null-consensus'>Null consensus guide</DocLink> and the{' '}
            <DocLink href='/docs/guides/subnets'>subnet guide</DocLink> for the capacity rules.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Upgrade checklist</h2>
          <ul>
            <li>
              <strong>Node operators:</strong> wait for the final governance-selected v495 tag; this
              proposal is not an activation or deployment instruction.
            </li>
            <li>
              <strong>Validators and delegates:</strong> use current metadata and keep each new Null
              weight row within the live mechanism-derived limit.
            </li>
            <li>
              <strong>Miners:</strong> no state migration is required; existing stored weight rows
              are not rewritten by this change.
            </li>
            <li>
              <strong>Subnet owners and governance:</strong> the default mechanism maximum remains
              two; raising it to 16 is a separate root decision.
            </li>
            <li>
              <strong>SDK and indexer operators:</strong> regenerate errors and transaction
              metadata, including the new Null row-limit error.
            </li>
          </ul>
          <p>Once the release train creates a final tag, build it from a clean checkout:</p>
          <pre className={styles.code_block}>
            <code>{`git fetch https://github.com/RaoFoundation/subtensor.git tag v495
git checkout --detach v495
cargo build --release --locked -p node-subtensor`}</code>
          </pre>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
