import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V441 Upgrade — Commit-Reveal-Safe Child Keys',
  description:
    'Child-key updates now cool down for the greater of the configured tempo count and the ' +
    'subnet reveal period, preventing stake from hopping between validator identities inside ' +
    'one commit-reveal window.',
  alternates: {canonical: '/releases/v441-upgrade'},
};

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The V441 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Commit-Reveal-Safe Child Keys · July 2026
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
            replaces that fixed delay with a subnet-aware rule. The configured default is{' '}
            <strong>two subnet tempos</strong>, but the effective cooldown can never be shorter than
            that subnet&apos;s commit-reveal period.
          </p>
          <p>
            In symbols, for subnet tempo <code>T</code>, configured child-key cooldown{' '}
            <code>C</code>, and reveal period <code>R</code>, a relationship submitted at block{' '}
            <code>B</code> receives the deadline <code>B + T × max(C, R)</code>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Why the reveal-period floor matters</p>
          <p>
            A parent stake position changes the inherited stake used for epoch consensus and
            validator-permit selection. Without a reveal-period floor, one mature position could be
            redirected between validator identities that keep separate weight commits, activity
            histories, permits, and bond portfolios.
          </p>
          <p>
            V441 keeps every new relationship pending through at least one complete commit-reveal
            window. This prevents the same capital position from hopping to whichever identity is
            currently most advantageous while commitments from that window are still unresolved.
            Existing permit-loss behavior still clears an abandoned validator&apos;s bonds; the new
            floor also covers validators that retain a permit on residual stake.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>The cooldown follows the subnet and its reveal period</p>
          <p>
            A tempo is the number of blocks between a subnet&apos;s epochs. V441 stores one global
            child-key cooldown in tempos, compares it with the subnet&apos;s{' '}
            <code>RevealPeriodEpochs</code>, and converts the larger value to blocks using the
            subnet tempo at submission.
          </p>
          <Code
            language='rust'
            code={`// pallets/subtensor/src/staking/set_children.rs
let cooldown_tempos =
    u64::from(ChildKeyCooldownTempos::<T>::get())
        .max(Self::get_reveal_period(netuid));
let cooldown = u64::from(Self::get_tempo(netuid))
    .saturating_mul(cooldown_tempos);
let cooldown_block = Self::get_current_block_as_u64()
    .saturating_add(cooldown);`}
          />
          <p>
            With the two-tempo default, a subnet with a 360-block tempo and one-epoch reveal period
            waits 720 blocks. If its reveal period is three epochs, it waits 1,080 blocks instead. A
            100-block subnet with a five-epoch reveal period waits 500 blocks. Changes made after
            submission do not rewrite an already-scheduled deadline.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Exact activation boundary</p>
          <p>
            The deadline is inclusive. A pending update is eligible when the current block is equal
            to or greater than its cooldown block. If an update is submitted on epoch block{' '}
            <code>B</code>, it becomes eligible at <code>B + T × max(C, R)</code> — not one tempo
            later.
          </p>
          <Code
            language='rust'
            code={`if cool_down_block <= current_block {
    Self::persist_pending_chidren_ok(netuid, &hotkey, &children);
}`}
          />
          <p>
            Pending child keys are processed with subnet epochs. An update submitted between epoch
            boundaries becomes eligible after its full block delay and is applied at the first
            subnet epoch that processes it. Setting <code>ChildKeyCooldownTempos</code> to zero does
            not disable the protection: the subnet reveal period remains the minimum.
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
                <td>Tempo × max(configured cooldown, reveal period)</td>
              </tr>
              <tr>
                <td>Configured cooldown</td>
                <td>Legacy block setting</td>
                <td>2 tempos by default, global root setting</td>
              </tr>
              <tr>
                <td>Security floor</td>
                <td>None</td>
                <td>Per-subnet RevealPeriodEpochs</td>
              </tr>
            </tbody>
          </table>
          <p>
            The new <code>ChildKeyCooldownTempos</code> storage value can be changed only through{' '}
            <code>AdminUtils.sudo_set_childkey_cooldown_tempos</code>, which requires a root origin.
            A successful change emits <code>AdminUtils.ChildKeyCooldownTemposSet</code> with the new
            value. Raising a subnet&apos;s reveal period above that value automatically raises its
            effective child-key cooldown; lowering it cannot take the cooldown below the configured
            global floor.
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
              updates use the two-tempo default automatically, or the subnet reveal period when it
              is longer.
            </li>
            <li>
              <strong>Client developers:</strong> migrate from the deprecated block-based storage
              and call to <code>ChildKeyCooldownTempos</code> and the AdminUtils sudo call.
            </li>
            <li>
              <strong>Root administrators:</strong> <code>ChildKeyCooldownTempos</code> controls the
              global floor. The commit-reveal floor applies even if this value is configured below a
              subnet&apos;s reveal period.
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
