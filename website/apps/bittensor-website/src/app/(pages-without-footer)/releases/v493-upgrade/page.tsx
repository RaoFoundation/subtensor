import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V493 Upgrade — Hotkey Gamma Spends, Yuma Commit Limits and Release Announcements',
  description:
    'V493 lets gamma deposits be spent by their own hotkey, adds sudo-configurable Yuma ' +
    'timelocked commit limits, Discord release announcements, and standalone stable Python ' +
    'publishing.',
  alternates: {canonical: '/releases/v493-upgrade'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V493 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Hotkey gamma spends, Yuma commit limits and release announcements
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Gamma spent by the deposit&apos;s hotkey</h2>
          <p>
            A gamma deposit is now spent by its own hotkey. The hotkey signs{' '}
            <code>spend_gamma(service, netuid, amount)</code> at a service listed on the subnet and
            pays no fee. The amount is burned and added to{' '}
            <code>GammaSpent(netuid, service, hotkey)</code>. A service credits the hotkey with what
            that total has grown by, read at a finalized block, so it sends no transaction and holds
            no spending authority over its customers&apos; deposits. The deposit&apos;s coldkey
            keeps control of the balance, the daily spend cap, and closing the deposit.
          </p>
          <p>
            A spend takes at least 0.1 gamma unless it empties the deposit, and stays within the
            balance and what the daily spend cap allows. A daily spend cap is 0, which pauses the
            deposit, or at least 0.1 gamma; the upgrade pauses existing caps below that. A move
            takes the whole balance, or at least 0.1 gamma leaving at least as much. A hotkey swap
            moves the coldkey&apos;s deposit at the old hotkey to the new hotkey, so the old hotkey
            can no longer spend it. Gamma emission opens the subnet owner hotkey&apos;s deposit with
            a cap of 0, so the owner coldkey sets a cap before that hotkey can spend it.
          </p>
          <p>This breaks existing gamma integrations:</p>
          <ol className={styles.list}>
            <li>
              <code>spend_gamma</code> is signed by the deposit&apos;s hotkey and takes a service, a
              netuid, and an amount. The batched spender call is gone.
            </li>
            <li>
              <code>mint_gamma</code>, <code>mint_gamma_with_stake</code>, and{' '}
              <code>set_service</code> no longer take a spender and move to call indices 166, 167,
              and 169; <code>spend_gamma</code> is 168. Indices 158, 159, 162, 164, and 165 are
              retired, and <code>set_gamma_deposit_spender</code> is removed.
            </li>
            <li>
              <code>GammaDeposit</code>, <code>ServiceInfo</code>, and the <code>GammaMinted</code>,{' '}
              <code>GammaEmissionMinted</code>, and <code>ServiceSet</code> events no longer carry a
              spender. The <code>GammaSpent</code> event is{' '}
              <code>{'{ netuid, hotkey, service, amount }'}</code>.
            </li>
            <li>
              The <code>GammaSpend</code> proxy type is deprecated and denies all calls. Only an{' '}
              <code>Any</code> proxy can spend for a hotkey.
            </li>
            <li>
              <code>GammaRuntimeApi</code> is version 2. <code>GammaDepositInfo</code> drops the
              spender, and <code>spendable</code> is what a spend can burn now under the 0.1 gamma
              minimum.
            </li>
            <li>
              In the Python SDK, <code>SpendGamma</code> takes a netuid, a service hotkey, and an
              amount and is signed by the deposit&apos;s hotkey, as in{' '}
              <code>btcli gamma spend --service-hotkey SERVICE_HOTKEY -H API_KEY</code>. The new{' '}
              <code>gamma_spent</code> read returns a hotkey&apos;s total at a service.{' '}
              <code>SetGammaDepositSpender</code>, <code>btcli gamma set-spender</code>, and the
              mint <code>--spender</code> and <code>--service-hotkey</code> options are removed.
            </li>
          </ol>
          <p>
            Services that spent their customers&apos; deposits as spenders now credit customers from{' '}
            <code>GammaSpent</code> instead. See{' '}
            <Link href='/docs/guides/gamma' className={styles.inline_link}>
              Gamma
            </Link>
            ,{' '}
            <Link href='/docs/guides/services' className={styles.inline_link}>
              Subnet services
            </Link>
            , and{' '}
            <Link
              href='https://github.com/RaoFoundation/subtensor/pull/3343'
              className={styles.inline_link}
            >
              PR #3343
            </Link>
            .
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
