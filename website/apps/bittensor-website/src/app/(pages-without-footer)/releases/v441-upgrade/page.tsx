import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V441 Upgrade — Two-Tempo Child Keys',
  description:
    'Child-key updates now cool down for two subnet tempos instead of a fixed 24 hours. ' +
    'The delay follows each subnet cadence and is configurable by root sudo.',
  alternates: {canonical: '/releases/v441-upgrade'},
};

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The V441 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Two-Tempo Child Keys · July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Child keys let a hotkey delegate a share of its stake weight to other hotkeys. Changes
            are delayed before they take effect so parent-child relationships cannot be rewired
            instantly.
          </p>
          <p>
            Until now, every child-key update waited a fixed 7,200 blocks — approximately 24 hours —
            regardless of the subnet&apos;s own operating cadence. Spec <strong>441</strong>{' '}
            replaces that fixed delay with a default of <strong>two subnet tempos</strong>.
          </p>
          <p>
            The result is a cooldown expressed in the unit that matters to the subnet: its epoch
            cycle. Child-key updates become usable sooner while still spanning two complete
            consensus periods by default.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>The cooldown follows the subnet</p>
          <p>
            A tempo is the number of blocks between a subnet&apos;s epochs. V441 stores one global
            cooldown value in tempos, then converts it to blocks using the tempo of the subnet where
            the child-key update was submitted.
          </p>
          <Code
            language='rust'
            code={`// pallets/subtensor/src/staking/set_children.rs
let cooldown = u64::from(Self::get_tempo(netuid))
    .saturating_mul(u64::from(ChildKeyCooldownTempos::<T>::get()));
let cooldown_block = Self::get_current_block_as_u64()
    .saturating_add(cooldown);`}
          />
          <p>
            With the default setting, a subnet with a 360-block tempo waits 720 blocks; a subnet
            with a 100-block tempo waits 200. If the subnet tempo later changes, already-scheduled
            updates keep the deadline calculated when they were submitted.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Exact activation boundary</p>
          <p>
            The deadline is inclusive. A pending update is eligible when the current block is equal
            to or greater than its cooldown block. If an update is submitted on epoch block{' '}
            <code>B</code> and the subnet tempo is <code>T</code>, the default deadline is{' '}
            <code>B + 2T</code>, and it activates on that second epoch — not one tempo later.
          </p>
          <Code
            language='rust'
            code={`if cool_down_block <= current_block {
    Self::persist_pending_chidren_ok(netuid, &hotkey, &children);
}`}
          />
          <p>
            Pending child keys are processed with subnet epochs. An update submitted between epoch
            boundaries becomes eligible after its full two-tempo block delay and is applied at the
            first subnet epoch that processes it. A root-configured value of zero makes the update
            eligible immediately at its deadline.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Parameters and administration</p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Setting</th>
                <th>Before V441</th>
                <th>V441 default</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Child-key cooldown</td>
                <td>7,200 blocks (~24 hours)</td>
                <td>2 subnet tempos</td>
              </tr>
              <tr>
                <td>Unit</td>
                <td>Blocks</td>
                <td>Tempos</td>
              </tr>
              <tr>
                <td>Configuration</td>
                <td>Legacy Subtensor setting</td>
                <td>Root sudo through AdminUtils</td>
              </tr>
            </tbody>
          </table>
          <p>
            The new <code>ChildKeyCooldownTempos</code> storage value can be changed only through{' '}
            <code>AdminUtils.sudo_set_childkey_cooldown_tempos</code>, which requires a root origin.
            A successful change emits <code>AdminUtils.ChildKeyCooldownTemposSet</code> with the new
            value.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Compatibility</p>
          <p>
            V441 layers the tempo-based setting on top of the existing chain interface. The legacy{' '}
            <code>DefaultPendingChildKeyCooldown</code>, <code>PendingChildKeyCooldown</code>{' '}
            storage, and <code>set_pending_childkey_cooldown</code> call remain available at their
            existing names and indexes so stored chain data and older clients continue to decode.
          </p>
          <p>
            Those block-based interfaces are deprecated and no longer control child-key activation.
            New clients should read <code>ChildKeyCooldownTempos</code> and use the root-only
            AdminUtils call.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <ul className={styles.list}>
            <li>
              <strong>Node operators:</strong> wait for the on-chain <code>spec_version</code> to
              move to 441, then update to the matching release.
            </li>
            <li>
              <strong>Subnet owners and validators:</strong> no action is required. New child-key
              updates use the two-tempo default automatically.
            </li>
            <li>
              <strong>Client developers:</strong> migrate from the deprecated block-based storage
              and call to <code>ChildKeyCooldownTempos</code> and the AdminUtils sudo call.
            </li>
            <li>
              <strong>Root administrators:</strong> keep the default at two unless governance
              explicitly chooses a different network-wide number of tempos.
            </li>
          </ul>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v441 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/code/pallets/subtensor/src/staking/set_children.rs'>
            Read the child-key cooldown implementation
          </Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
