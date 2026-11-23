import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';
import layout from './page.module.css';

export const metadata: Metadata = {
  title: 'The V495 Upgrade — 16 Null Mechanisms, a Shared Null Epoch, and the V494 Changes',
  description:
    'V495 lets a Null subnet run up to 16 mechanisms with 2,500 UIDs, caps Null weight rows, ' +
    'shares Null epoch work across mechanisms, removes the default pause of the deposit that ' +
    'gamma emission opens for a subnet owner, and sizes the weight of a single-validator root ' +
    'claim to its fund. ' +
    'This guide also covers V494: channel commitment burns, weight delegation, basket controls, ' +
    'and the Etao owner limit.',
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
      <FadeInWrapper className={`${styles.page_container} ${layout.page}`}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V495 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            16 Null mechanisms · 2,500 UIDs · weight-row cap · shared Null epoch · gamma emission
            deposits · root claim weight · with the V494 changes · Proposed upgrade
          </p>
        </section>

        <section className={styles.section} id='scope'>
          <h2 className={styles.subtitle}>Scope, versions, and compatibility</h2>
          <p>
            The proposed runtime specification version is <strong>495</strong>. The transaction
            version stays <strong>2</strong>. This page describes a proposed upgrade. It does not
            give an activation time and it is not a deployment instruction. Governance schedules
            each deployment and each sudo call separately. All times on this page are in UTC.
          </p>
          <p>
            This guide gives one picture of two releases. <strong>V494</strong> is the release
            before this one: channel commitment burns, weight delegation for one mechanism, basket
            trade controls, and a shorter Etao owner limit. <strong>V495</strong> adds the changes
            for Null consensus and for gamma emission deposits. Each section names its release. The{' '}
            <DocLink href='/releases/v494-upgrade'>V494 release page</DocLink> has the original
            V494 notes.
          </p>
          <p>
            Wallets, indexers, SDK users, and other signers must refresh runtime metadata for spec
            495. Do not reuse a signed payload that you prepared for another runtime version.
          </p>
          <h3 className={styles.subtitle_small}>Python packages</h3>
          <p>
            The Python packages for both releases are <code>bittensor</code>{' '}
            <strong>11.3.6</strong> and <code>bittensor-core</code> <strong>0.1.13</strong>. V494
            published no Python packages, so 11.3.6 is the first SDK release with the V494 calls,
            reads, and commands. <code>bittensor-core</code> 0.1.13 has the same code as 0.1.12.
          </p>
          <Code language='bash' code={`uv pip install "bittensor==11.3.6"
btcli --version`} />
          <h3 className={styles.subtitle_small}>What changes on chain</h3>
          <p>
            V495 adds one SubtensorModule error, <code>TooManyWeightsForNullMechanism</code>, at
            error index <strong>209</strong>, and one SubtensorModule storage item,{' '}
            <code>BasketRowsOpenedAt</code>. It adds no call, no event, and no migration. Mechanism
            storage keeps the index <code>netuid + mecid * 4096</code>.
          </p>
          <p>
            <code>BasketRowsOpenedAt</code> is a map from a validator hotkey to a block number and
            an extrinsic index. The{' '}
            <DocLink href='#root-claims'>root claim section</DocLink> explains it. The error{' '}
            <code>RootClaimTooHeavy</code> keeps its index and gets one more cause.
          </p>
          <p>V494 added these calls. Regenerate metadata; do not hard-code call indices.</p>
          <div className={layout.table_scroller}>
            <table className={styles.metrics_table}>
              <thead>
                <tr><th>Pallet</th><th>Call</th><th>Index</th><th>Signer</th></tr>
              </thead>
              <tbody>
                <tr><td>Commitments</td><td><code>set_commitment_to</code></td><td>3</td><td>Coldkey</td></tr>
                <tr><td>SubtensorModule</td><td><code>set_mech_weight_delegate</code></td><td>170</td><td>Validator hotkey</td></tr>
                <tr><td>SubtensorModule</td><td><code>set_mechanism_weights_for</code></td><td>171</td><td>Delegate</td></tr>
                <tr><td>SubtensorModule</td><td><code>commit_mechanism_weights_for</code></td><td>172</td><td>Delegate</td></tr>
                <tr><td>SubtensorModule</td><td><code>reveal_mechanism_weights_for</code></td><td>173</td><td>Delegate</td></tr>
                <tr><td>SubtensorModule</td><td><code>commit_timelocked_mechanism_weights_for</code></td><td>174</td><td>Delegate</td></tr>
                <tr><td>AdminUtils</td><td><code>sudo_set_commit_burn_enabled</code>, <code>_min</code>, <code>_max</code>, <code>_half_life</code>, <code>_increase_mult</code></td><td>123–127</td><td>Root or subnet owner</td></tr>
                <tr><td>AdminUtils</td><td><code>sudo_set_basket_trade_max_slippage_bps</code>, <code>_refill_blocks</code>, <code>_price_reference</code></td><td>128–130</td><td>Root</td></tr>
                <tr><td>AdminUtils</td><td><code>sudo_set_subnet_emission_weights_rate_limit</code></td><td>131</td><td>Root</td></tr>
              </tbody>
            </table>
          </div>
        </section>

        <section className={styles.section} id='summary'>
          <h2 className={styles.subtitle}>What changes, at a glance</h2>
          <div className={layout.table_scroller}>
            <table className={styles.metrics_table}>
              <thead>
                <tr><th>Subject</th><th>Release</th><th>Before</th><th>After</th></tr>
              </thead>
              <tbody>
                <tr>
                  <td>UID capacity of a Null subnet</td><td>V495</td>
                  <td><code>MaxAllowedUids * mechanism_count</code> at most 2,500</td>
                  <td><code>MaxAllowedUids</code> at most 2,500 for any mechanism count</td>
                </tr>
                <tr>
                  <td>Mechanisms on a Null subnet</td><td>V495</td>
                  <td>More mechanisms meant fewer UIDs</td>
                  <td>Up to 16 mechanisms with the full UID capacity, when root raises the global limit</td>
                </tr>
                <tr>
                  <td>Length of a new Null weight row</td><td>V495</td>
                  <td>At most the registered UID count</td>
                  <td><code>max(1, floor(2500 / mechanism_count))</code> entries</td>
                </tr>
                <tr>
                  <td>Null epoch work</td><td>V495</td>
                  <td>One complete epoch for each mechanism</td>
                  <td>Subnet-wide state is read once; results are identical</td>
                </tr>
                <tr>
                  <td>Block weight of a Null epoch</td><td>V495</td>
                  <td>Inside one fixed hook weight</td>
                  <td>The hook adds a weight for each Null epoch that runs</td>
                </tr>
                <tr>
                  <td>Deposit that gamma emission opens</td><td>V495</td>
                  <td>Daily spend cap of 0: paused until the owner coldkey sets a cap</td>
                  <td>No cap: the owner hotkey can spend at once</td>
                </tr>
                <tr>
                  <td>Block weight that <code>claim_root_with_hotkey</code> reserves</td><td>V495</td>
                  <td>The same 2,093 ms for every validator</td>
                  <td>Sized to the validator&apos;s fund: 478 ms for a fund with 129 rows and no queued credits</td>
                </tr>
                <tr>
                  <td>Largest fund that one <code>claim_root_with_hotkey</code> admits</td><td>V495</td>
                  <td>130 claim units: rows on 128 subnets</td>
                  <td>258 claim units: rows on 256 subnets</td>
                </tr>
                <tr>
                  <td>Owner limit for subnet emission weights (Etao)</td><td>V494</td>
                  <td>Constant of 50,400 blocks</td>
                  <td>Storage value, 360 blocks by default; root can change it</td>
                </tr>
                <tr>
                  <td>Basket trade band, price reference, refill</td><td>V494</td>
                  <td>Constants</td>
                  <td>Storage values with the same defaults; root can change them</td>
                </tr>
                <tr>
                  <td>Channel commitments with an optional burn</td><td>V494</td>
                  <td>Not available</td>
                  <td>One commitment for each hotkey, mechanism, and channel</td>
                </tr>
                <tr>
                  <td>Weight delegation</td><td>V494</td>
                  <td>Not available</td>
                  <td>A validator hotkey authorizes one delegate for one mechanism</td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>

        <section className={styles.section} id='capacity'>
          <h2 className={styles.subtitle}>V495: Null UID capacity for any mechanism count</h2>
          <p>
            A Null subnet allows <code>MaxAllowedUids</code> up to <strong>2,500</strong>. The
            mechanism count does not change that limit. A Yuma subnet keeps its rule:{' '}
            <code>MaxAllowedUids * mechanism_count</code> is at most 256. Before V495, a Null subnet
            had to keep <code>MaxAllowedUids * mechanism_count</code> at or below 2,500, so each
            added mechanism lowered the UID capacity.
          </p>
          <p>
            The global maximum mechanism count still defaults to <strong>2</strong>. Root can raise
            it to the hard maximum of <strong>16</strong> with the existing call{' '}
            <code>sudo_set_max_mechanism_count</code>. The upgrade does not make that call and does
            not change the mechanism count of a subnet.
          </p>
          <Code language='bash' code={`# The subnet owner selects Null and sets the capacity.
btcli hparams set --netuid 1 --name epoch_consensus --value Null -w subnet_owner
btcli hparams set --netuid 1 --name max_allowed_uids --value 2500 -w subnet_owner
btcli hparams get --netuid 1 --name max_allowed_uids`} />
          <Code language='python' code={`import bittensor as bt

async with bt.Client(network="finney") as client:
    await client.execute(
        bt.SetHyperparameter(netuid=1, name="epoch_consensus", value="Null"),
        owner_wallet,
    )
    await client.execute(
        bt.SetHyperparameter(netuid=1, name="max_allowed_uids", value=2500),
        owner_wallet,
    )

    # Raw forms. The owner signs them; they are not sudo calls.
    await client.submit_call(
        bt.calls.AdminUtils.sudo_set_epoch_consensus(netuid=1, mode="Null"),
        owner_wallet,
    )
    await client.submit_call(
        bt.calls.AdminUtils.sudo_set_max_allowed_uids(netuid=1, max_allowed_uids=2500),
        owner_wallet,
    )`} />
          <p>
            A capacity above the limit fails with <code>TooManyUIDsPerMechanism</code>. See the{' '}
            <DocLink href='/docs/guides/null-consensus'>Null consensus guide</DocLink>.
          </p>
        </section>

        <section className={styles.section} id='weight-budget'>
          <h2 className={styles.subtitle}>V495: weight rows and the row cap</h2>
          <p>
            A weight row is the list of UIDs and weights that one validator sets on one mechanism.
            On a Null subnet, a new row holds at most{' '}
            <code>max(1, floor(2500 / current_mechanism_count))</code> entries. The budget of 2,500
            entries is fixed. No sudo call changes it. Yuma rows have no such cap.
          </p>
          <div className={layout.table_scroller}>
            <table className={styles.metrics_table}>
              <thead>
                <tr><th>Mechanisms</th><th>Entries in one new Null weight row</th></tr>
              </thead>
              <tbody>
                <tr><td>1</td><td>2,500</td></tr>
                <tr><td>2</td><td>1,250</td></tr>
                <tr><td>3</td><td>833</td></tr>
                <tr><td>4</td><td>625</td></tr>
                <tr><td>8</td><td>312</td></tr>
                <tr><td>16</td><td>156</td></tr>
              </tbody>
            </table>
          </div>
          <p>
            With one mechanism, nothing changes. With more mechanisms, a row cannot name every UID
            of a full subnet: at 16 mechanisms a row names at most 156 of up to 2,500 UIDs. UIDs
            that a row does not name receive no miner reward from that mechanism in that epoch.
          </p>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              The cap applies to direct weights, delegated <code>*_for</code> calls, legacy hash
              reveals, timelocked reveals, and batch calls. The error is{' '}
              <code>TooManyWeightsForNullMechanism</code>.
            </li>
            <li>
              The chain cannot read the row length of a timelocked commit. It checks the length at
              the reveal and skips a row above the cap. The block hook does not stop.
            </li>
            <li>
              Rows that are in storage stay as they are when the mechanism count changes. The cap
              applies to new rows.
            </li>
            <li>
              A Null row that is not a single self-weight needs at least{' '}
              <code>min(MinAllowedWeights, row cap, registered UIDs)</code> entries. The stored{' '}
              <code>MinAllowedWeights</code> value does not change.
            </li>
          </ul>
          <Code language='bash' code={`# Mechanism 15 of a Null subnet with 16 mechanisms: at most 156 entries.
btcli misc weights set --netuid 1 --mechid 15 --raw-u16 --weights-file weights.json \\
  -w my_coldkey -H my_hotkey

# weights.json maps each UID to an integer weight, for example {"2": 65534, "3": 1}`} />
          <Code language='python' code={`# From bittensor 11.3.6 the SDK reads the cap and refuses a longer row
# before it signs or encrypts anything.
intent = bt.SetWeights(netuid=1, mechid=15, weights={2: 65534, 3: 1}, raw_u16=True)
try:
    result = await client.execute(intent, wallet)
except bt.ChainError as error:
    if error.code == bt.ErrorCode.LIMIT_EXCEEDED:
        print(error.name, error.message)

# Raw direct call, signed by the validator hotkey.
raw = bt.calls.SubtensorModule.set_mechanism_weights(
    netuid=1, mecid=15, dests=[2, 3], weights=[65534, 1], version_key=0,
)
await client.submit_call(raw, wallet, signer="hotkey")`} />
          <p>
            <code>btcli weights</code> is a hidden alias of <code>btcli misc weights</code>.
          </p>
        </section>

        <section className={styles.section} id='mechanisms'>
          <h2 className={styles.subtitle}>Guide: run several mechanisms on a subnet</h2>
          <p>
            A mechanism is one scoring track inside a subnet. It has its own weights and its own
            share of the emission. Miners and validators keep one UID for all mechanisms. The
            complete guide, with every error, is{' '}
            <DocLink href='/docs/guides/mechanisms'>Run multiple mechanisms</DocLink>.
          </p>
          <h3 className={styles.subtitle_small}>Rules for the owner calls</h3>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              <strong>Rate limits.</strong> The owner can change the mechanism count once per{' '}
              <strong>7,200 blocks</strong> for each subnet. The emission split has its own limit of
              7,200 blocks. The first change is always allowed. Root is exempt. The error is{' '}
              <code>TxRateLimitExceeded</code>.
            </li>
            <li>
              <strong>Admin freeze window.</strong> The chain refuses owner changes in the last
              blocks before the epoch of the subnet (<code>AdminFreezeWindow</code>, 10 blocks by
              default), and while an epoch that the owner triggered is pending. The error is{' '}
              <code>AdminActionProhibitedDuringWeightsWindow</code>.
            </li>
            <li>
              <strong>Pending commits on a Null subnet.</strong> The count cannot change while a
              timelocked commit or a legacy hash commit is pending on any mechanism.
            </li>
          </ul>
          <h3 className={styles.subtitle_small}>Step 1. Read the current state</h3>
          <Code language='bash' code={`btcli hparams mechanisms count --netuid 1
btcli hparams mechanisms emissions --netuid 1
btcli query epoch-status --netuid 1
btcli query timelocked-weight-commits --netuid 1 --mechid 0`} />
          <Code language='python' code={`count = await client.subnets.mechanism_count(netuid=1)
split = await client.subnets.mechanism_emission_split(netuid=1)
limit = await client.query(bt.storage.SubtensorModule.MaxMechanismCount)`} />
          <h3 className={styles.subtitle_small}>Step 2. Root: raise the global limit, if necessary</h3>
          <p>
            A count above <code>MaxMechanismCount</code> fails with <code>InvalidValue</code>. This
            root call is a separate governance decision.
          </p>
          <Code language='bash' code={`btcli call AdminUtils.sudo_set_max_mechanism_count \\
  --args '{"max_mechanism_count": 16}' --sudo -w sudo_key`} />
          <Code language='python' code={`inner = await client.compose(
    bt.calls.AdminUtils.sudo_set_max_mechanism_count(max_mechanism_count=16)
)
await client.submit_call(bt.calls.Sudo.sudo(call=inner), sudo_wallet)`} />
          <h3 className={styles.subtitle_small}>Step 3. Owner: set the mechanism count</h3>
          <Code language='bash' code={`btcli hparams mechanisms count --netuid 1 --value 4 -w subnet_owner

# The generated command does the same and can show a preview.
btcli tx set-mechanism-count --netuid 1 --mechanism-count 4 --dry-run`} />
          <Code language='python' code={`await client.execute(bt.SetMechanismCount(netuid=1, mechanism_count=4), owner_wallet)

# Raw form, AdminUtils call index 76. The owner signs it.
await client.submit_call(
    bt.calls.AdminUtils.sudo_set_mechanism_count(netuid=1, mechanism_count=4),
    owner_wallet,
)`} />
          <p>
            New mechanisms start without weights. A lower count deletes the state of the removed
            mechanisms: weights, incentive, bonds, and pending commits. The chain then removes
            their weight delegates and channel commitments in bounded steps, and the count cannot
            change again until that work is complete. Each change of the count resets the emission
            split to an equal split.
          </p>
          <h3 className={styles.subtitle_small}>Step 4. Owner: set the emission split</h3>
          <p>
            The split is a list of 16-bit shares, one for each mechanism in order. The shares must
            add up to exactly 65,535. The SDK has no intent for this call, so use the raw call,
            AdminUtils call index 77.
          </p>
          <Code language='bash' code={`# 40%, 30%, 20% and 10% for mechanisms 0 to 3
btcli call AdminUtils.sudo_set_mechanism_emission_split \\
  --args '{"netuid": 1, "maybe_split": [26214, 19661, 13107, 6553]}' -w subnet_owner

# Back to an equal split
btcli call AdminUtils.sudo_set_mechanism_emission_split \\
  --args '{"netuid": 1, "maybe_split": null}' -w subnet_owner`} />
          <Code language='python' code={`call = bt.calls.AdminUtils.sudo_set_mechanism_emission_split(
    netuid=1, maybe_split=[26214, 19661, 13107, 6553],
)
await client.submit_call(call, owner_wallet)`} />
          <p>
            A mechanism receives <code>floor(emission * share / 65535)</code>. Mechanism 0 also
            receives the units that rounding leaves.
          </p>
          <h3 className={styles.subtitle_small}>Step 5. Check the result</h3>
          <Code language='bash' code={`btcli hparams mechanisms count --netuid 1
btcli query mechanism-emission-split --netuid 1 --json`} />
        </section>

        <section className={styles.section} id='pipelines'>
          <h2 className={styles.subtitle}>Guide: commit pipelines for one mechanism</h2>
          <p>
            Each mechanism has its own pipelines. Name the mechanism with <code>--mechid</code>, or{' '}
            <code>mechid</code> in Python. The default is 0.
          </p>
          <div className={layout.table_scroller}>
            <table className={styles.metrics_table}>
              <thead>
                <tr><th>Pipeline</th><th>Command</th><th>Intent</th><th>Chain call</th></tr>
              </thead>
              <tbody>
                <tr>
                  <td>Direct weights</td>
                  <td><code>btcli misc weights set</code>, when commit-reveal is off</td>
                  <td><code>SetWeights</code></td>
                  <td><code>set_mechanism_weights</code></td>
                </tr>
                <tr>
                  <td>Timelocked commit</td>
                  <td><code>btcli misc weights set</code>, when commit-reveal is on, or <code>btcli misc weights commit</code></td>
                  <td><code>SetWeights</code>, <code>CommitWeights</code></td>
                  <td><code>commit_timelocked_mechanism_weights</code></td>
                </tr>
                <tr>
                  <td>Legacy hash commit</td>
                  <td><code>btcli misc weights hash</code></td>
                  <td><code>CommitWeightHash</code></td>
                  <td><code>commit_mechanism_weights</code></td>
                </tr>
                <tr>
                  <td>Legacy reveal</td>
                  <td><code>btcli misc weights reveal</code></td>
                  <td><code>RevealWeights</code></td>
                  <td><code>reveal_weights</code> on mechanism 0, <code>reveal_mechanism_weights</code> on the others</td>
                </tr>
                <tr>
                  <td>Channel commitment (V494)</td>
                  <td><code>btcli commitments commit</code></td>
                  <td><code>SetChannelCommitment</code></td>
                  <td><code>Commitments.set_commitment_to</code></td>
                </tr>
              </tbody>
            </table>
          </div>
          <h3 className={styles.subtitle_small}>Direct weights and timelocked commits</h3>
          <p>
            <code>set</code> selects the path that the subnet runs. <code>commit</code> always sends
            a timelocked commit. The chain decrypts a timelocked commit at its drand round. No
            reveal call is necessary.
          </p>
          <Code language='bash' code={`btcli misc weights set --netuid 1 --mechid 2 \\
  --uids 0,1,2 --weights 0.5,0.3,0.2 -w my_coldkey -H my_hotkey
btcli misc weights commit --netuid 1 --mechid 2 \\
  --uids 0,1,2 --weights 0.5,0.3,0.2 -w my_coldkey -H my_hotkey
btcli query timelocked-weight-commits --netuid 1 --mechid 2`} />
          <Code language='python' code={`await client.execute(
    bt.SetWeights(netuid=1, mechid=2, weights={0: 0.5, 1: 0.3, 2: 0.2}), wallet,
)
await client.execute(
    bt.CommitWeights(netuid=1, mechid=2, weights={0: 0.5, 1: 0.3, 2: 0.2}), wallet,
)
pending = await client.weights.timelocked_weight_commits(netuid=1, mechid=2)`} />
          <p>
            On a Null subnet, one ciphertext has at most{' '}
            <code>floor(32 KiB / mechanism_count)</code> bytes. All mechanisms of the subnet share
            one queue for each epoch: 64 commits and 64 KiB. A hotkey has one pending row for each
            mechanism, and a new commit replaces it. On a Yuma subnet the limit stays 5,000 bytes.
          </p>
          <h3 className={styles.subtitle_small}>Legacy hash commit and reveal</h3>
          <Code language='bash' code={`btcli misc weights hash --netuid 1 --mechid 2 --hash 0xCOMMIT_HASH \\
  -w my_coldkey -H my_hotkey
btcli misc weights reveal --netuid 1 --mechid 2 \\
  --uids 0,1,2 --weights 0.5,0.3,0.2 --salt 1,2,3 -w my_coldkey -H my_hotkey`} />
          <Code language='python' code={`await client.execute(
    bt.CommitWeightHash(netuid=1, mechid=2, commit_hash="0x" + "11" * 32), wallet,
)
await client.execute(
    bt.RevealWeights(
        netuid=1, mechid=2, uids=[0, 1, 2], weights=[0.5, 0.3, 0.2], salt=[1, 2, 3],
    ),
    wallet,
)`} />
          <h3 className={styles.subtitle_small}>V494: channel commitments with an optional burn</h3>
          <p>
            A channel commitment is a payload that a coldkey publishes for a registered hotkey that
            it owns, at one <code>(netuid, mechid, channel)</code>. It is separate from the legacy
            commitment and from each other channel. The coldkey signs and pays. With no burn
            configuration, the commitment costs only the transaction fee.
          </p>
          <p>
            The subnet owner can turn on a burn for each channel. Each paid commitment raises the
            price of that channel. The price falls to its minimum over its half-life.{' '}
            <code>max_burn</code> is a price limit, not an extra payment. The chain does not refund
            burned TAO.
          </p>
          <Code language='bash' code={`# Read the live price, publish, and read the commitment.
btcli commitments price --netuid 1 --mechid 1 --channel 7
btcli commitments commit --netuid 1 --mechid 1 --channel 7 \\
  --data "model endpoint and metadata" --max-burn 0.25 -w my_coldkey -H my_hotkey
btcli commitments show --netuid 1 --mechid 1 --channel 7 --hotkey HOTKEY

# The subnet owner configures one channel. Set max before min.
btcli commitments burn-config --netuid 1 --mechid 1 --channel 7 \\
  --name max_burn --value 10 -w subnet_owner
btcli commitments burn-config --netuid 1 --mechid 1 --channel 7 \\
  --name min_burn --value 0.1 -w subnet_owner
btcli commitments burn-config --netuid 1 --mechid 1 --channel 7 \\
  --name half_life --value 360 -w subnet_owner
btcli commitments burn-config --netuid 1 --mechid 1 --channel 7 \\
  --name increase_mult --value 1.26 -w subnet_owner
btcli commitments burn-config --netuid 1 --mechid 1 --channel 7 \\
  --name enabled --value true -w subnet_owner`} />
          <Code language='python' code={`intent = bt.SetChannelCommitment(
    netuid=1, mechid=1, channel=7,
    data="model endpoint and metadata", max_burn_tao="0.25",
)
result = await client.execute(intent, wallet)

price = await client.identity.commit_burn(netuid=1, mechid=1, channel=7)
commitment = await client.identity.channel_commitment(
    netuid=1, mechid=1, channel=7, hotkey_ss58=hotkey,
)

# Owner configuration through the intent.
await client.execute(
    bt.SetCommitBurn(netuid=1, mechid=1, channel=7, name="enabled", value=True),
    owner_wallet,
)

# Raw form, Commitments call index 3. The coldkey signs it; it is not a sudo call.
raw = bt.calls.Call("Commitments", "set_commitment_to", {
    "netuid": 1,
    "mecid": 1,
    "channel": 7,
    "hotkey": hotkey,
    "info": {"fields": [{"Raw3": b"abc"}]},
    "max_burn": 250_000_000,
})
result = await client.submit_call(raw, wallet)`} />
          <p>
            For a timelocked payload, use <code>--reveal-round</code> or the{' '}
            <code>reveal_round</code> field of the intent. Raw plaintext has at most 512 bytes. New
            writes to a channel wait while a hotkey swap or a purge of that hotkey is pending. See
            the <DocLink href='/docs/guides/channel-commitments'>channel commitment guide</DocLink>.
          </p>
        </section>

        <section className={styles.section} id='delegation'>
          <h2 className={styles.subtitle}>V494 guide: delegate weights for one mechanism</h2>
          <p>
            A registered validator hotkey can authorize one account as its delegate on exactly one
            subnet and one mechanism. The delegate pays the normal transaction fee. It sends direct
            weights, legacy hash commits and reveals, or timelocked commits for that validator. The
            registration, stake, permit, rate limit, and row rules of the validator still apply.
            The grant does not move stake or ownership, and it does not apply to another mechanism.
          </p>
          <Code language='bash' code={`# The validator hotkey signs the grant and the revocation.
btcli misc weights delegate set --netuid 1 --mechid 1 --delegate OPERATOR_HOTKEY \\
  -w validator_coldkey -H validator_hotkey
btcli misc weights delegate show --netuid 1 --mechid 1 --hotkey VALIDATOR_HOTKEY
btcli misc weights delegate revoke --netuid 1 --mechid 1 \\
  -w validator_coldkey -H validator_hotkey

# The delegate signs. --for-hotkey names the validator.
btcli misc weights set --netuid 1 --mechid 1 --for-hotkey VALIDATOR_HOTKEY \\
  --uids 0,1,2 --weights 0.5,0.3,0.2 -w operator_coldkey -H operator_hotkey
btcli misc weights commit --netuid 1 --mechid 1 --for-hotkey VALIDATOR_HOTKEY \\
  --uids 0,1,2 --weights 0.5,0.3,0.2 -w operator_coldkey -H operator_hotkey
btcli misc weights hash --netuid 1 --mechid 1 --for-hotkey VALIDATOR_HOTKEY \\
  --hash 0xCOMMIT_HASH -w operator_coldkey -H operator_hotkey
btcli misc weights reveal --netuid 1 --mechid 1 --for-hotkey VALIDATOR_HOTKEY \\
  --uids 0,1,2 --weights 0.5,0.3,0.2 --salt 1,2,3 \\
  -w operator_coldkey -H operator_hotkey`} />
          <Code language='python' code={`# The validator grants one mechanism. delegate=None revokes the grant.
await client.execute(
    bt.SetMechWeightDelegate(netuid=1, mechid=1, delegate=operator_hotkey),
    validator_wallet,
)
delegate = await client.weights.mech_weight_delegate(
    netuid=1, mechid=1, hotkey_ss58=validator_hotkey,
)

# The SDK selects the direct or the timelocked *_for call from chain state.
await client.execute(
    bt.SetWeights(
        netuid=1, mechid=1, weights={0: 0.5, 1: 0.3, 2: 0.2},
        for_hotkey=validator_hotkey,
    ),
    operator_wallet,
)

# Raw grant, SubtensorModule call index 170, signed by the validator hotkey.
raw = bt.calls.Call("SubtensorModule", "set_mech_weight_delegate", {
    "netuid": 1, "mecid": 1, "delegate": operator_hotkey,
})
await client.submit_call(raw, validator_wallet, signer="hotkey")

# Raw direct weights, index 171, signed by the delegate hotkey.
raw = bt.calls.Call("SubtensorModule", "set_mechanism_weights_for", {
    "netuid": 1,
    "mecid": 1,
    "hotkey": validator_hotkey,
    "dests": [0, 1, 2],
    "weights": [65_535, 39_321, 26_214],
    "version_key": 0,
})
await client.submit_call(raw, operator_wallet, signer="hotkey")

# Legacy delegated commit and reveal: indices 172 and 173.
commit = bt.calls.Call("SubtensorModule", "commit_mechanism_weights_for", {
    "netuid": 1, "mecid": 1, "hotkey": validator_hotkey,
    "commit_hash": commit_hash,
})
reveal = bt.calls.Call("SubtensorModule", "reveal_mechanism_weights_for", {
    "netuid": 1, "mecid": 1, "hotkey": validator_hotkey,
    "uids": [0, 1, 2], "values": [65_535, 39_321, 26_214],
    "salt": [1, 2, 3], "version_key": 0,
})

# Timelocked delegated commit: index 174.
sealed = bt.calls.Call("SubtensorModule", "commit_timelocked_mechanism_weights_for", {
    "netuid": 1, "mecid": 1, "hotkey": validator_hotkey,
    "commit": ciphertext, "reveal_round": reveal_round, "commit_reveal_version": 4,
})`} />
          <p>
            Give the grant to a fresh operational hotkey, and revoke it when you no longer need it.
            A hotkey that registers again does not get an old grant. A hotkey swap moves a grant
            only while the destination owns the former UID of the validator. A lower mechanism
            count removes the grants of the removed mechanisms.
          </p>
        </section>

        <section className={styles.section} id='null-epoch'>
          <h2 className={styles.subtitle}>V495: one Null epoch for all mechanisms</h2>
          <p>
            Before V495, a Null subnet ran one complete epoch for each mechanism. Each run read
            every hotkey, registration block, and stake value again. V495 reads that subnet-wide
            state once for the subnet. Each mechanism then adds only its own work: its last-update
            blocks, the weight row of the elected validator, and its pending commits.
          </p>
          <p>
            The results are identical: storage, events, emissions, dividends, incentive, and
            permits. Tests run the old path and the new path on the same state and compare the
            complete state. Yuma epochs do not change.
          </p>
          <div className={layout.table_scroller}>
            <table className={styles.metrics_table}>
              <thead>
                <tr><th>Mechanisms</th><th>UIDs</th><th>Epoch time before</th><th>Epoch time after</th></tr>
              </thead>
              <tbody>
                <tr><td>1</td><td>256</td><td>9.3 ms</td><td>9.3 ms</td></tr>
                <tr><td>16</td><td>100</td><td>35.6 ms</td><td>4.9 ms</td></tr>
                <tr><td>16</td><td>256</td><td>130.7 ms</td><td>11.4 ms</td></tr>
                <tr><td>16</td><td>2,500</td><td>1,632.7 ms</td><td>119.5 ms</td></tr>
              </tbody>
            </table>
          </div>
          <p>
            These numbers are from one development machine, with a release build. They are not
            reference weights. At 16 mechanisms and 2,500 UIDs the epoch made 603,107 storage reads
            before and 40,330 after.
          </p>
          <h3 className={styles.subtitle_small}>Block weight</h3>
          <p>
            The block hook had one fixed weight for every block. From V495 it adds a weight for
            each Null epoch that runs in the block. That weight has two inputs: the mechanism count
            and the UID count. Until reference hardware measures it, the weight reserves one more
            fixed hook envelope for each Null epoch, about 0.83 seconds of block weight. A block
            with a Null epoch therefore has less room for transactions. The scheduler already runs
            a Null epoch as the only epoch of its block.
          </p>
          <p>
            Node operators do not configure anything. No event, storage value, or reward changes.
          </p>
        </section>

        <section className={styles.section} id='gamma'>
          <h2 className={styles.subtitle}>V495: a deposit that gamma emission opens has no cap</h2>
          <p>
            A subnet can pay shares of its emission into the gamma of another subnet. The chain
            credits that gamma to the deposit of the subnet owner, at the subnet owner hotkey, on
            the target subnet. Since V493 a deposit that this opened had a daily spend cap of 0.
            The owner hotkey could spend only after the owner coldkey set a cap.
          </p>
          <p>
            From V495 a deposit that gamma emission opens has <strong>no cap</strong>. The owner
            hotkey can spend the minted gamma at once.
          </p>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              An existing deposit keeps its cap. A deposit that is paused today stays paused until
              its coldkey sets or removes the cap.
            </li>
            <li>No migration changes existing deposits.</li>
            <li>
              Only this case changes. A deposit that you open with a mint (
              <code>btcli gamma mint</code>, <code>mint_gamma</code>,{' '}
              <code>mint_gamma_with_stake</code>) has the cap that the mint names, or no cap when
              the mint names none, as before.
            </li>
            <li>
              The pause after a hotkey swap, the rules for cap values, and the cap call of the
              coldkey do not change.
            </li>
            <li>
              The event <code>GammaEmissionMinted</code> does not change. It has no cap field.
            </li>
          </ul>
          <Code language='bash' code={`# Read the deposit of the owner hotkey on the target subnet.
btcli gamma show --netuid 3 --hotkey OWNER_HOTKEY

# The owner coldkey sets a cap of 20 gamma a day, pauses the deposit, or removes the cap.
btcli gamma set-cap --netuid 3 --hotkey OWNER_HOTKEY --daily-spend-cap-gamma 20
btcli gamma set-cap --netuid 3 --hotkey OWNER_HOTKEY --daily-spend-cap-gamma 0
btcli gamma set-cap --netuid 3 --hotkey OWNER_HOTKEY --remove-cap

# To have a cap from the first credit, open the deposit with a cap before
# the first emission arrives. Sign with the owner coldkey.
btcli gamma mint --netuid 3 --hotkey OWNER_HOTKEY --amount-tao 1 --daily-spend-cap-gamma 0`} />
          <Code language='python' code={`deposit = await client.gamma.gamma_deposit(netuid=3, hotkey_ss58=owner_hotkey)

await client.execute(
    bt.SetGammaDepositDailySpendCap(
        netuid=3, hotkey_ss58=owner_hotkey, daily_spend_cap_gamma=20,
    ),
    owner_wallet,
)
await client.execute(
    bt.SetGammaDepositDailySpendCap(netuid=3, hotkey_ss58=owner_hotkey, remove_cap=True),
    owner_wallet,
)

# Raw form, SubtensorModule call index 163. Amounts have 10^9 units for each gamma;
# None removes the cap.
raw = bt.calls.SubtensorModule.set_gamma_deposit_daily_spend_cap(
    hotkey=owner_hotkey, netuid=3, daily_spend_cap=20_000_000_000,
)
await client.submit_call(raw, owner_wallet)`} />
          <p>
            The cap belongs to the deposit, not to the hotkey. The chain removes a deposit when its
            coldkey closes it, moves its whole balance to another hotkey, or swaps its hotkey while
            it holds gamma. If gamma emission credits the same hotkey after that, it opens a new
            deposit without a cap. Before V495 that deposit opened paused. To keep a cap or a pause
            on the owner hotkey, do not close its deposit and do not move its whole balance out.
          </p>
          <p>
            Gamma emission credits the owner hotkey of the subnet that pays the share. A hotkey
            swap on the target subnet only moves the deposit to the new hotkey, but the paying
            subnet keeps the old owner hotkey. Later credits then open a new deposit at the old
            hotkey, without a cap. To replace an owner hotkey, swap it on the paying subnet too, or
            on all subnets.
          </p>
          <p>
            Protect the owner hotkey. Without a cap, a person who controls that hotkey can spend
            all the gamma in its deposit. See the <DocLink href='/docs/guides/gamma'>gamma guide</DocLink>.
          </p>
        </section>

        <section className={styles.section} id='root-claims'>
          <h2 className={styles.subtitle}>V495: a root claim reserves the weight of its fund</h2>
          <p>
            <code>claim_root_with_hotkey</code> redeems your root dividends from the fund (the
            basket) of one validator. A fund has one row for each subnet where it holds alpha, and
            one row for its TAO. Before a transaction runs, it reserves block weight: the compute
            time that the block keeps for it. After the transaction, the chain returns the part
            that the transaction did not use.
          </p>
          <p>
            Until V494 this claim reserved the same weight for every validator: about 2,093 ms.
            A block had room for only one such claim. From V495 the claim reserves the weight of
            the validator&apos;s own fund. The chain counts two things before the claim runs: the
            stored rows of the fund, and its queued dividend credits. A queued dividend credit is
            a dividend that the fund has earned but that the chain has not yet put into the fund.
            The claim settles these credits first.
          </p>
          <div className={layout.table_scroller}>
            <table className={styles.metrics_table}>
              <thead>
                <tr><th>Fund of the validator</th><th>Reserved in V494</th><th>Reserved in V495</th></tr>
              </thead>
              <tbody>
                <tr><td>Up to 8 rows, no queued credits</td><td>2,093 ms</td><td>67 ms</td></tr>
                <tr><td>129 rows, no queued credits</td><td>2,093 ms</td><td>478 ms</td></tr>
                <tr><td>129 rows, 128 queued credits</td><td>2,093 ms</td><td>1,273 ms</td></tr>
                <tr><td>161 rows, no queued credits</td><td>Claim refused</td><td>587 ms</td></tr>
                <tr><td>161 rows, 160 queued credits</td><td>Claim refused</td><td>1,570 ms</td></tr>
                <tr><td>257 rows, no queued credits</td><td>Claim refused</td><td>913 ms</td></tr>
                <tr><td>257 rows, 256 queued credits</td><td>Claim refused</td><td>2,459 ms</td></tr>
              </tbody>
            </table>
          </div>
          <p>
            These numbers come from the weight formula of the runtime. They are the reserved
            weight, not a measured time. The chain still returns the unused part after the claim.
          </p>
          <p>
            Each number includes a fixed 32 ms for one more task of the claim. Sometimes a
            dividend credit must wait before it can go into the fund. If your root stake changes
            while a credit waits, the chain writes a correction for your account, so that the
            change does not move dividends between stakers. The correction stays until your next
            claim, also after the credit is in the fund. The claim settles these corrections
            first. The chain reserves the weight before it knows your account. Thus it reserves
            the weight for the most corrections that one account can have on one validator: 256.
          </p>
          <h3 className={styles.subtitle_small}>The limit: 258 claim units</h3>
          <p>
            One claim admits a fund of at most <strong>258 claim units</strong>. The validator is
            one unit. Each stored row of the fund is one unit. Each queued credit on a subnet
            where the fund has no row is one unit, because the claim can open that row. The limit
            was 130 units: a fund with rows on 128 subnets. 258 units is a fund with rows on 256
            subnets. The chain refuses a claim on a larger fund with{' '}
            <code>RootClaimTooHeavy</code>.
          </p>
          <p>
            The largest weight that a claim can reserve is 2,464 ms. That is 94.8% of the largest
            transaction that a block accepts, 2,599.9 ms. Only a fund with rows on 256 subnets and
            a queued credit for each row comes near this value.
          </p>
          <h3 className={styles.subtitle_small}>One new rule for batches</h3>
          <p>
            A batch computes the weight of all its calls before it runs the first call. If an
            earlier call in the batch gives the fund a new row, the weight of a later claim in
            the same batch is too small. The chain therefore refuses{' '}
            <code>claim_root_with_hotkey</code> with <code>RootClaimTooHeavy</code> when one of
            these calls ran earlier in the same transaction, for the same validator:
          </p>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              A <code>swap_basket</code> or <code>swap_basket_many</code> trade that bought alpha
              on a subnet where the fund had no row.
            </li>
            <li>A hotkey swap that moved a fund, or queued credits, to this validator hotkey.</li>
          </ul>
          <p>
            Send the claim in a later transaction. The same block is sufficient. The chain records
            the block and the transaction index of such a trade or swap in the new storage item{' '}
            <code>BasketRowsOpenedAt</code>. A trade that buys more of a subnet that the fund
            already holds, or that sells into TAO, does not stop a claim.
          </p>
          <h3 className={styles.subtitle_small}>What does not change</h3>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              The fee. The chain charges a claim for at most four claim units, for every fund
              size. A fee quote returns the same fee, and a weight that now depends on the fund.
            </li>
            <li>The payout, the claim threshold, and the dust rules.</li>
            <li>
              The claim for all validators of a coldkey, <code>claim_root</code>. It keeps its
              limit of 256 units and its weight.
            </li>
            <li>
              The subnet limit. V495 does not change it. With V495, a fund with a row on each
              of 256 subnets stays claimable with one single-validator claim.
            </li>
          </ul>
          <Code language='bash' code={`# Preview the claim: the reserved fee, the fee that will settle, and the claim-unit check.
btcli root claim --hotkey VALIDATOR_HOTKEY --dry-run -w my_wallet

# Send the claim.
btcli root claim --hotkey VALIDATOR_HOTKEY -w my_wallet`} />
          <Code language='python' code={`# The rows of the fund, and its queued dividend credits.
rows = await client.read("validator_basket", hotkey_ss58=validator_hotkey)
queued = await client.query_map(
    bt.storage.SubtensorModule.PendingBasketDeposits, [validator_hotkey],
)

# The plan stops a claim on a fund above the limit of the connected runtime before it
# signs: 258 claim units from runtime 495, 130 before.
plan = await client.plan(bt.ClaimRootWithHotkey(hotkey_ss58=validator_hotkey), wallet)
await client.execute(bt.ClaimRootWithHotkey(hotkey_ss58=validator_hotkey), wallet)

# Raw form, SubtensorModule call index 148.
raw = bt.calls.SubtensorModule.claim_root_with_hotkey(hotkey=validator_hotkey)
await client.submit_call(raw, wallet)

# The last trade or hotkey swap that opened a row: (block, transaction index), or None.
opened = await client.query(
    bt.storage.SubtensorModule.BasketRowsOpenedAt, [validator_hotkey],
)`} />
          <p>
            See the <DocLink href='/docs/guides/root-reborn'>root guide</DocLink>.
          </p>
        </section>

        <section className={styles.section} id='etao'>
          <h2 className={styles.subtitle}>V494: Etao owner updates once per 360 blocks</h2>
          <p>
            A subnet owner changes the emission weight vector of the subnet with a rate limit. The
            limit was a constant of <strong>50,400 blocks</strong>. From V494 it is the storage
            value <code>SubnetEmissionWeightsRateLimit</code>, with a default of{' '}
            <strong>360 blocks</strong>, one default tempo. Root can change it through AdminUtils
            call index 131. A value of 0 removes the wait. The limit is for each subnet. Root is
            exempt, and the first owner change of a subnet is always allowed.
          </p>
          <Code language='bash' code={`# The status view reads the limit from the chain and shows the next owner change.
btcli subnets emission show --netuid 1

# The owner changes one slot; the other slots are rescaled.
btcli subnets emission set-value --netuid 1 --slot validator --value 30% -w subnet_owner

# Root changes the limit for all subnets.
btcli call AdminUtils.sudo_set_subnet_emission_weights_rate_limit \\
  --args '{"rate_limit": 360}' --sudo -w sudo_key`} />
          <Code language='python' code={`info = await client.subnets.subnet_emission_weights(netuid=1)
print(info.rate_limit_blocks, info.rate_limit())

limit = await client.query(bt.storage.SubtensorModule.SubnetEmissionWeightsRateLimit)

# Root-only raw call, AdminUtils call index 131.
inner = await client.compose(bt.calls.Call(
    "AdminUtils", "sudo_set_subnet_emission_weights_rate_limit", {"rate_limit": 360},
))
await client.submit_call(bt.calls.Sudo.sudo(call=inner), sudo_wallet)`} />
          <p>
            <code>bittensor</code> 11.3.6 reads the limit from the chain. Earlier SDK versions
            assume 50,400 blocks: they show a later block for the next owner change, and their
            check before signing stops an owner change that the chain accepts. The limit applies to
            the emission weight vector of the owner, not to the weights of validators.
          </p>
        </section>

        <section className={styles.section} id='basket'>
          <h2 className={styles.subtitle}>V494: basket trade controls with the old defaults</h2>
          <p>
            Root can set the trade band for each leg of a basket trade, the price reference, and
            the refill period of the turnover bucket. Without a root call, the values are the same
            as before: a band of 200 basis points (2%), the reference{' '}
            <code>StrictestEmaAndSpot</code>, and a refill over 7,200 blocks. No migration is
            necessary to keep those values.
          </p>
          <Code language='bash' code={`# Root-only calls. Add --dry-run to see a preview.
btcli call AdminUtils.sudo_set_basket_trade_max_slippage_bps \\
  --args '{"bps": 500}' --sudo -w sudo_key
btcli call AdminUtils.sudo_set_basket_trade_refill_blocks \\
  --args '{"blocks": 7200}' --sudo -w sudo_key
btcli call AdminUtils.sudo_set_basket_trade_price_reference \\
  --args '{"reference": "SpotOnly"}' --sudo -w sudo_key`} />
          <Code language='python' code={`# Root-only raw call, AdminUtils call index 128.
inner = await client.compose(bt.calls.Call(
    "AdminUtils", "sudo_set_basket_trade_max_slippage_bps", {"bps": 500},
))
await client.submit_call(bt.calls.Sudo.sudo(call=inner), sudo_wallet)

# Read the three values.
band = await client.query(bt.storage.SubtensorModule.BasketTradeMaxSlippageBps)
reference = await client.query(bt.storage.SubtensorModule.BasketTradePriceReferenceMode)
refill = await client.query(bt.storage.SubtensorModule.BasketTradeRefillBlocks)`} />
          <p>
            See the{' '}
            <DocLink href='/docs/guides/basket-trading-governance'>basket governance guide</DocLink>.
          </p>
        </section>

        <section className={styles.section} id='compatibility'>
          <h2 className={styles.subtitle}>Compatibility and migration notes</h2>
          <h3 className={styles.subtitle_small}>Node operators</h3>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>Wait for the final tag that governance selects. Build it from a clean checkout.</li>
            <li>
              Neither release runs a storage migration at the upgrade. V494 moves and removes
              channel commitments and weight delegates in bounded steps, in the idle time of
              blocks.
            </li>
            <li>
              A block with a Null epoch reserves more hook weight than before. You do not configure
              anything.
            </li>
          </ul>
          <h3 className={styles.subtitle_small}>Validators</h3>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>Upgrade to <code>bittensor</code> 11.3.6 before you use V494 or V495 features.</li>
            <li>
              On a Null subnet with several mechanisms, keep each new row inside the row cap. A
              timelocked row above the cap is skipped at its reveal.
            </li>
            <li>
              The size limit of one timelocked commit on a Null subnet is{' '}
              <code>floor(32 KiB / mechanism_count)</code>.
            </li>
            <li>Grant a weight delegate for each mechanism separately (V494).</li>
            <li>
              Do not put a <code>swap_basket</code> buy on a new subnet, or a hotkey swap, in one
              batch with a <code>claim_root_with_hotkey</code> on the same fund. The claim fails
              with <code>RootClaimTooHeavy</code>. Send it in a later transaction.
            </li>
          </ul>
          <h3 className={styles.subtitle_small}>Miners</h3>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>No migration is necessary. One UID serves all mechanisms.</li>
            <li>
              With several mechanisms, the reward of a miner is the sum of its rewards in each
              mechanism. A lower mechanism count deletes the state of the removed mechanisms.
            </li>
            <li>
              Channel commitments are optional. If a channel has a burn, read the live price before
              you publish (V494).
            </li>
          </ul>
          <h3 className={styles.subtitle_small}>Subnet owners</h3>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              More than two mechanisms need a root call first. Then use the steps in the guide
              above. The count and the split each have a limit of 7,200 blocks.
            </li>
            <li>
              A deposit that gamma emission opens after the upgrade has no cap. Set a cap, or open
              the deposit with a cap first, if you want a limit. Deposits that exist keep their cap.
            </li>
            <li>The owner limit for emission weights is 360 blocks by default (V494).</li>
            <li>Channel burns are off until you configure them (V494).</li>
          </ul>
          <h3 className={styles.subtitle_small}>SDK users</h3>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              Use <code>bittensor</code> 11.3.6 and <code>bittensor-core</code> 0.1.13. Earlier
              versions do not have the V494 intents, reads, and commands.
            </li>
            <li>
              <code>SetWeights</code> and <code>CommitWeights</code> can now raise{' '}
              <code>ChainError</code> with the code <code>LIMIT_EXCEEDED</code> before they sign.
            </li>
            <li>
              The <code>subnet_emission_weights</code> read has the new field{' '}
              <code>rate_limit_blocks</code>.
            </li>
            <li>
              The <code>channel_commitment</code> read and <code>btcli commitments show</code>{' '}
              decode a commitment that the node returns as text. In earlier versions the read
              failed with <code>ValueError</code> in that case. Revealed text that starts with{' '}
              <code>0x</code> is shown as the node returns it; earlier versions decoded it a second
              time.
            </li>
            <li>
              The check before <code>ClaimRootWithHotkey</code> uses the rules of the connected
              runtime. It reads the runtime version at the block from which it reads the fund. On
              a runtime before 495 the limit is 130 claim units. From runtime 495 the limit is 258
              claim units, and the check also counts queued credits on subnets where the fund has
              no row. Thus the SDK does not sign a claim that the chain refuses with{' '}
              <code>RootClaimTooHeavy</code>.
            </li>
            <li>
              For a raw call, use <code>client.submit_call(call, wallet)</code>. Add{' '}
              <code>signer=&quot;hotkey&quot;</code> for a call that the hotkey signs. A client with
              a policy needs <code>bt.Policy(allow_raw_calls=True)</code>.
            </li>
          </ul>
          <h3 className={styles.subtitle_small}>Wallets and indexers</h3>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              Regenerate metadata for spec 495. The transaction version stays 2. V495 appends one
              error at index 209 and one storage item, <code>BasketRowsOpenedAt</code>. It changes
              no call, no event, and no type of an existing storage item.
            </li>
            <li>
              The weight in a fee quote for <code>claim_root_with_hotkey</code> now depends on the
              fund of the validator. The fee does not. Do not use a fixed weight for this call.
            </li>
            <li>
              If you did not update for V494, also add its calls, storage items, events, and
              errors.
            </li>
            <li>
              If you derive the cap of a gamma deposit from events: a deposit that{' '}
              <code>GammaEmissionMinted</code> opens has no cap from spec 495. It had a cap of 0
              before.
            </li>
            <li>
              Null epochs emit the same events as before, one{' '}
              <code>IncentiveAlphaEmittedToMiners</code> for each mechanism.
            </li>
          </ul>
        </section>

        <section className={styles.section} id='risks'>
          <h2 className={styles.subtitle}>Accepted risks and open items</h2>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              <strong>Parent stake reads (accepted).</strong> The Null epoch reads the stake of
              every parent of every UID. A hotkey can have up to 100 parents, and older lists can
              be longer. The weight of the epoch does not grow with the number of parents. In a
              local test with 100 parents for each of 2,500 UIDs, one epoch took about 1.9
              seconds. The project accepts this risk for V495: live subnets do not have such parent
              lists. V495 adds no parent cap and no charge for each parent.
            </li>
            <li>
              <strong>Lower mechanism count (accepted).</strong> When an owner lowers the mechanism
              count, the call deletes the weight rows of each removed mechanism at once, and the
              weight of the call is fixed. A Null subnet can now hold up to 2,500 weight rows for
              each mechanism, so a change from 16 mechanisms to 1 can delete up to 37,500 rows in
              one call. Only the elected validator can set a row, so rows collect only as the
              elected validator changes. The project accepts this risk for V495.
            </li>
            <li>
              <strong>Null UID trimming (accepted).</strong> The owner of a Null subnet can remove
              UIDs in batches (<code>sudo_trim_null_uids_batch</code>). The call has a small fixed
              weight, about 0.34 ms. A batch reads the weight rows of every mechanism and deletes
              the rows of UIDs without a validator permit. With 16 mechanisms and 2,500 rows for
              each, one batch can need about 5 seconds by the database weights. That is more than
              the compute time of one block. Rows collect only as the elected validator changes.
              The project accepts this risk for V495.
            </li>
            <li>
              <strong>Interim weight (open).</strong> The weight of a Null epoch is a placeholder
              until reference hardware measures it. It does not include the payout to hotkeys that
              follows the epoch.
            </li>
            <li>
              <strong>Gamma spending (accepted).</strong> The owner hotkey can spend emission gamma
              without a call from the owner coldkey. This is the purpose of the change.
            </li>
            <li>
              <strong>Gamma cap after a removed deposit (accepted).</strong> A cap or a pause does
              not stay when the chain removes the deposit: after a close, a move of the whole
              balance, or a hotkey swap. The next emission credit to that hotkey opens a deposit
              without a cap. The <DocLink href='#gamma'>gamma section</DocLink> gives the details.
            </li>
            <li>
              <strong>Largest root claim (to confirm).</strong> A single-validator claim on a fund
              at the limit of 258 claim units, with a queued credit for each row, reserves 2,464 ms.
              That is 94.8% of the largest transaction. It stays below the limit, and a claim on a
              real fund reserves much less than before. The project accepted 2,432 ms. The fixed
              32 ms for the corrections of the claimant came after that, from an audit.
            </li>
            <li>
              <strong>Claims with many waiting credits (open).</strong> A queued credit that cannot
              go into the fund at the current price waits, and a claimant can sell their own part
              of it in the claim. For each of these sales the chain goes through all waiting
              credits of the validator again. The reserved weight counts each waiting credit one
              time. This code is the same as in V494, but V494 reserved a fixed 2,093 ms for
              every claim, which left more room for this work. This case is not measured on
              reference hardware. The work grows with the number of subnets.
            </li>
            <li>
              <strong>Trade and deposit weight at 256 subnets (open).</strong>{' '}
              <code>swap_basket</code>, <code>swap_basket_many</code>, and{' '}
              <code>stake_into_basket</code> reserve weight for a fund of 256 rows. With 256
              subnets a fund can have 257 rows: 256 subnets and its TAO. Review these values
              before the subnet limit goes above 255.
            </li>
            <li>
              <strong>Spot-only basket reference (V494).</strong> The reference{' '}
              <code>SpotOnly</code> removes the moving-price anchors. A manipulated spot price
              inside the band can then be traded against. Root selects it deliberately or not at
              all.
            </li>
          </ul>
        </section>

        <section className={styles.section} id='sudo'>
          <h2 className={styles.subtitle}>Sudo calls after the upgrade</h2>
          <p>
            The runtime upgrade makes none of these calls. Each call is a separate governance
            decision with its own schedule, in UTC.
          </p>
          <h3 className={styles.subtitle_small}>Proposed in the V494 guide, in this order</h3>
          <ol className={`${styles.list} ${layout.ordered}`}>
            <li><code>AdminUtils::sudo_set_basket_trade_max_slippage_bps(500)</code></li>
            <li><code>AdminUtils::sudo_set_basket_trade_price_reference(SpotOnly)</code></li>
            <li><code>AdminUtils::sudo_set_basket_concentration_cap(9830)</code></li>
          </ol>
          <p>
            The third call uses a setter that existed before V494. <code>SpotOnly</code> has the
            risk that the section above describes.
          </p>
          <h3 className={styles.subtitle_small}>Not scheduled</h3>
          <p>V495 proposes no sudo call. These values stay at their defaults until root acts:</p>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              <strong>Global mechanism limit, 2.</strong> Subnets can run more than two mechanisms
              only after root calls <code>AdminUtils::sudo_set_max_mechanism_count(n)</code>, with{' '}
              <code>n</code> up to 16. This release does not propose a value or a time.
            </li>
            <li>
              <strong>Etao owner limit, 360 blocks.</strong> Root can change it with{' '}
              <code>AdminUtils::sudo_set_subnet_emission_weights_rate_limit(blocks)</code>.
            </li>
            <li>
              <strong>Subnet limit.</strong> V495 does not change the subnet limit. A higher limit
              is a separate root call. From V495 a single-validator root claim admits a fund with
              rows on 256 subnets, so a limit of up to 256 is safe for claims. Read the open item
              about trade and deposit weight first.
            </li>
          </ul>
        </section>

        <section className={styles.section} id='checklist'>
          <h2 className={styles.subtitle}>Upgrade checklist</h2>
          <ul className={`${styles.list} ${layout.bullets}`}>
            <li>
              <strong>Node operators:</strong> build and run the final v495 tag that governance
              selects. Read the spec version and the transaction version from metadata.
            </li>
            <li>
              <strong>Validators:</strong> upgrade the SDK, name the mechanism in each weight call,
              and keep Null rows inside the row cap.
            </li>
            <li>
              <strong>Miners:</strong> no state migration. Refresh metadata.
            </li>
            <li>
              <strong>Subnet owners:</strong> decide on the mechanism count and the split, check
              the cap of your gamma emission deposit, and read the Etao limit from the chain.
            </li>
            <li>
              <strong>Root stakers:</strong> no action. A single-validator claim reserves less
              block weight; the fee and the payout do not change.
            </li>
            <li>
              <strong>SDK and indexer operators:</strong> regenerate calls, storage, errors, and
              events for spec 495.
            </li>
            <li>
              <strong>Governance:</strong> the sudo calls above are separate decisions.
            </li>
          </ul>
          <p>When the release train creates the final tag, build it from a clean checkout:</p>
          <Code language='bash' code={`git fetch https://github.com/RaoFoundation/subtensor.git tag v495
git checkout --detach v495
cargo build --release --locked -p node-subtensor`} />
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
