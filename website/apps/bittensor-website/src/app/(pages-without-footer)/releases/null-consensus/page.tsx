import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'Null Consensus — Large Subnets and Exact Weight Ratios',
  description:
    'Opt-in Null consensus selects the highest-stake validator for miner rewards, pays ' +
    'stake-proportional dividends, and supports a shared capacity of 2,500 UIDs.',
  alternates: {canonical: '/releases/null-consensus'},
};

export default function Page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>Null Consensus</h1>
          <p className={styles.subtitle}>Large subnets and exact weight ratios · Upcoming</p>
        </section>
        <section className={styles.section}>
          <h2 className={styles.subtitle}>What changes</h2>
          <p>
            Subnet owners can opt into a second epoch mechanism. Null consensus takes miner
            incentives from the highest-stake UID, which holds the sole validator permit. Ties
            select the first UID. Only that permit holder may submit or commit weights. All staked
            UIDs retain proportional dividends, including inactive UIDs and those without permits.
            When the selected row has no valid weights, every registered UID shares the miner budget
            equally. Yuma remains the default for existing and new subnets.
          </p>
          <p>
            Null skips consensus and bond calculations and leaves bond storage unchanged. The epoch
            and payout interfaces remain compatible. Rewards to an existing sole-owner staking pool
            increase its balance without rewriting ownership shares; shared pools and collateral
            retain their accounting protections. Existing submission stake thresholds,
            stale-destination checks and commit-reveal protections still apply.
          </p>
        </section>
        <section className={styles.section}>
          <h2 className={styles.subtitle}>Capacity and switching</h2>
          <p>
            Null shares a 2,500-UID capacity budget across emission mechanisms: 2,500 with one
            mechanism, 1,250 with two, and 625 with four. Switching to Null preserves current
            capacity until the owner raises it. Returning to Yuma requires the registered population
            to fit its shared 256-UID budget; successful switching also clamps configured capacity.
            Pruning is an explicit owner action, with at most 64 UID deletions per transaction.
            Owners repeat the same target until completion; pending weight commits are cancelled
            before UID compaction. Returning to Yuma restores the previous validator limit, clamped
            to the remaining capacity.
          </p>
        </section>
        <section className={styles.section}>
          <h2 className={styles.subtitle}>Registration without upfront TAO</h2>
          <p>
            Owners can enable proof-of-work registration on non-root subnets, including large Null
            subnets. The SDK mines a recent challenge and the CLI exposes it through
            <code>btcli subnets register --netuid 1 --pow</code>. A direct, zero-tip proof
            registration pays no transaction fee, TAO burn or initial collateral purchase. Burn
            registration remains available when enabled. Burn registrations raise only burn cost,
            and PoW registrations raise only PoW difficulty. They share the multiplier and per-block
            half-life, with separate bounds and a team-controlled PoW minimum. Owners can enable
            either route or both, and must leave at least one enabled. PoW defaults to disabled for
            new subnets; previously stored explicit toggles are retained. It retains per-block
            registration limits, pruning and immunity protections.
          </p>
          <p>
            The Rust miner uses all discovered OpenCL GPUs by default, with CPU fallback. Long
            searches refresh the latest block hash every twelve seconds. Proofs bind the subnet and
            both keys, use a five-block freshness window, and cannot reuse an accepted challenge for
            the same hotkey. New hotkey associations respect a 256-entry coldkey ownership and
            staking work limit; existing associations can register on additional subnets. Root
            registration remains separate.
          </p>
        </section>
        <section className={styles.section}>
          <h2 className={styles.subtitle}>Precise rewards and large weight submissions</h2>
          <p>
            Null stores submitted unsigned 16-bit weights without max-scaling. The SDK and CLI
            provide an explicit raw-integer path, including JSON file input for large rows. Exact
            wide integer arithmetic computes rewards before display values are quantized. Whole-unit
            remainders are allocated deterministically in UID order; total miner emissions never
            exceed their epoch budget. Fractional entitlements are not carried to later epochs.
          </p>
          <p>
            Null timelocked ciphertexts can be up to 32 KiB divided by the mechanism count, leaving
            room above a full row at the corresponding UID ceiling. Queues share byte and count
            limits across mechanisms, replace a hotkey’s own pending row, and allow the leading
            eligible validator to displace lower-priority commits. Mode changes wait for pending
            commits to drain. Yuma retains its 5,000-byte admission limit.
          </p>
          <pre className={styles.code_block}>{`# Subnet owner: switch, then raise capacity
btcli hparams set --netuid 1 --name epoch_consensus --value Null
btcli hparams set --netuid 1 --name max_allowed_uids --value 2500

# Validator: weights.json contains {"2": 65534, "3": 1, ...}
btcli misc weights set --netuid 1 --raw-u16 --weights-file weights.json`}</pre>
          <p>
            See the <Link href='/docs/guides/null-consensus'>Null consensus guide</Link> for
            validation rules, integer rounding and returning to Yuma.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
