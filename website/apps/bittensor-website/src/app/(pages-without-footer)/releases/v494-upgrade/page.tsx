import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V494 Upgrade — Commit Burns, Weight Delegation, Basket Controls, and Etao',
  description:
    'V494 adds optional per-channel commitment burns, per-mechanism weight delegation, ' +
    'basket controls with legacy defaults, and a configurable Etao owner-update interval.',
  alternates: {canonical: '/releases/v494-upgrade'},
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
          <h1 className={styles.paper_title}>The V494 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Commit burns · mechanism weight delegation · basket controls · Etao · Release candidate
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Scope, versions, and compatibility</h2>
          <p>
            The runtime specification version is <strong>494</strong> and the transaction version is{' '}
            <strong>2</strong>. Node operators should build the final tagged release when governance
            schedules it. Wallets, indexers, SDK users, and other signers must regenerate metadata
            and use transaction version 2 when signing for this runtime; do not reuse a signed
            payload prepared for a different runtime version. This page describes the release
            candidate, not an activation time. Any deployment or sudo action must be scheduled
            separately, in UTC.
          </p>
          <p>
            V493, including gamma, is already on main. V494 does not change gamma. New
            SubtensorModule delegated-weight calls are at <strong>170–174</strong>, leaving gamma
            at 166–169. Commitments adds <code>set_commitment_to</code> at <strong>3</strong>.
            AdminUtils 122 remains the existing Yuma timelocked-commit limit; commitment-burn
            setters use 123–127, basket controls 128–130, and the Etao interval setter 131.
            Regenerate metadata rather than hard-coding call indices in an integration.
          </p>
          <table>
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
              <tr><td>AdminUtils</td><td><code>sudo_set_commit_burn_enabled</code>, <code>_min</code>, <code>_max</code>, <code>_half_life</code>, <code>_increase_mult</code></td><td>123–127</td><td>Root, or subnet owner under its existing rules</td></tr>
              <tr><td>AdminUtils</td><td><code>sudo_set_basket_trade_max_slippage_bps</code>, <code>_refill_blocks</code>, <code>_price_reference</code></td><td>128–130</td><td>Root</td></tr>
              <tr><td>AdminUtils</td><td><code>sudo_set_subnet_emission_weights_rate_limit</code></td><td>131</td><td>Root</td></tr>
            </tbody>
          </table>
          <p>
            The Python SDK for this runtime is <code>bittensor</code> 11.3.6 with{' '}
            <code>bittensor-core</code> 0.1.13. It adds the channel commitment, commit-burn, and
            mechanism weight-delegate intents, reads, and <code>btcli</code> commands described
            below; earlier SDKs do not include them.
          </p>
        </section>

        <section className={styles.section} id='commit-burns'>
          <h2 className={styles.subtitle}>Optional burn-for-commit, per mechanism and channel</h2>
          <p>
            A coldkey can publish a commitment for a registered hotkey it owns at an exact{' '}
            <code>(netuid, mechid, channel)</code>. This is independent of the legacy commitment
            and of every other channel. The coldkey signs, pays transaction fees and any configured
            burn, and funds any configured deposit. The hotkey does not pay the burn. With no
            enabled configuration, publishing is free apart from ordinary transaction fees.
          </p>
          <p>
            A successful paid commit raises that channel&apos;s price, which decays toward its
            minimum according to its half-life. The current price is computed lazily: a stored
            amount can be stale, so quote it before publishing. <code>max_burn</code> is a price
            limit, not an extra payment. The runtime charges the current price and never retains
            the unused difference. Burned TAO is not refunded when a commitment changes or a
            channel is retired.
          </p>
          <Code language='bash' code={`# Publish plaintext to subnet 1, mechanism 1, channel 7.
btcli commitments commit --netuid 1 --mechid 1 --channel 7 \\
  --data "model endpoint and metadata" --max-burn 0.25 \\
  -w my_coldkey -H my_hotkey

# Quote the live decayed burn and inspect the stored configuration.
btcli commitments price --netuid 1 --mechid 1 --channel 7
btcli commitments show --netuid 1 --mechid 1 --channel 7 --hotkey HOTKEY

# The subnet owner configures each channel independently. Set max before min.
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
          <Code language='python' code={`import bittensor as bt

# Use the SDK release carrying v494 metadata.
async with bt.Client(network="finney") as client:
    intent = bt.SetChannelCommitment(
        netuid=1,
        mechid=1,
        channel=7,
        data="model endpoint and metadata",
        max_burn_tao="0.25",
    )
    result = await client.execute(intent, wallet)

    price = await client.identity.commit_burn(netuid=1, mechid=1, channel=7)
    commitment = await client.identity.channel_commitment(
        netuid=1, mechid=1, channel=7, hotkey_ss58=hotkey,
    )

    # Raw form: a signed coldkey call, not a sudo call.
    raw = bt.calls.Call("Commitments", "set_commitment_to", {
        "netuid": 1,
        "mecid": 1,
        "channel": 7,
        "hotkey": hotkey,
        "info": {"fields": [{"Raw3": b"abc"}]},
        "max_burn": 250_000_000,
    })
    # Call index 3; info uses the runtime's length-tagged Data variants.
    result = await client.submit_call(raw, wallet, allow_raw_calls=True)

    # Root configuration: wrap AdminUtils 123 in Sudo.sudo.
    inner = await client.compose(bt.calls.Call(
        "AdminUtils", "sudo_set_commit_burn_enabled",
        {"netuid": 1, "mecid": 1, "channel": 7, "enabled": True},
    ))
    result = await client.submit_call(
        bt.calls.Sudo.sudo(call=inner), sudo_wallet, allow_raw_calls=True,
    )`} />
          <p>
            For timelocked payloads, use <code>--reveal-round</code> or the intent&apos;s{' '}
            <code>reveal_round</code>; the chain reveals at the supplied future drand round. Raw
            plaintext is limited to 512 bytes. See the{' '}
            <DocLink href='/docs/guides/channel-commitments'>channel commitment guide</DocLink> for
            encoding and live-price details.
          </p>
          <h3>Migration and lifecycle notes</h3>
          <p>
            Legacy commitments remain separate; no legacy payload is copied into a channel.
            Existing channels are unaffected until a coldkey publishes to them. New channel writes
            wait while that hotkey&apos;s bounded swap or purge migration is pending. Hotkey swaps,
            reverse swaps, deregistration, mechanism retirement, and subnet dissolution preserve
            the original reserve payer through bounded idle work. Empty cycles and terminal prefix
            scans are charged to that work budget. Miners and validators should use current SDK
            metadata and retry a pending migration write after its queue drains, not bypass the
            migration guard. No miner migration is required.
          </p>
        </section>

        <section className={styles.section} id='delegation'>
          <h2 className={styles.subtitle}>Weight delegation for an exact mechanism</h2>
          <p>
            A registered validator hotkey can authorize one account as its delegate on exactly one
            subnet and mechanism. The delegate pays normal transaction fees and submits direct
            weights, legacy hash commits and reveals, or timelocked commits for that validator.
            Registration, UID state, stake, validator permits, version checks, rate limits, and
            weight-conformance rules remain the validator&apos;s. Delegation does not transfer
            stake, ownership, validator status, or access to another mechanism. A missing grant
            authorizes nobody.
          </p>
          <Code language='bash' code={`# Grant and revoke from the validator's own hotkey.
btcli weights delegate set --netuid 1 --mechid 1 --delegate OPERATOR_HOTKEY \\
  -w validator_coldkey -H validator_hotkey
btcli weights delegate show --netuid 1 --mechid 1 --hotkey VALIDATOR_HOTKEY
btcli weights delegate revoke --netuid 1 --mechid 1 \\
  -w validator_coldkey -H validator_hotkey

# Submit as the authorized operator; --for-hotkey identifies the validator.
btcli weights set --netuid 1 --mechid 1 --for-hotkey VALIDATOR_HOTKEY \\
  --uids 0,1,2 --weights 0.5,0.3,0.2 -w operator_coldkey -H operator_hotkey
btcli weights commit --netuid 1 --mechid 1 --for-hotkey VALIDATOR_HOTKEY \\
  --uids 0,1,2 --weights 0.5,0.3,0.2 -w operator_coldkey -H operator_hotkey
btcli weights hash --netuid 1 --mechid 1 --for-hotkey VALIDATOR_HOTKEY \\
  --hash 0xCOMMIT_HASH -w operator_coldkey -H operator_hotkey
btcli weights reveal --netuid 1 --mechid 1 --for-hotkey VALIDATOR_HOTKEY \\
  --uids 0,1,2 --weights 0.5,0.3,0.2 --salt 1,2,3 \\
  -w operator_coldkey -H operator_hotkey`} />
          <Code language='python' code={`# Validator grants one exact mechanism.
await client.execute(
    bt.SetMechWeightDelegate(netuid=1, mechid=1, delegate=operator_hotkey),
    validator_wallet,
)

# The SDK chooses plaintext or timelocked *_for dispatch from chain state.
await client.execute(
    bt.SetWeights(
        netuid=1, mechid=1, weights={0: 0.5, 1: 0.3, 2: 0.2},
        for_hotkey=validator_hotkey,
    ),
    operator_wallet,
)

# Raw direct-weight call, SubtensorModule index 171.
raw = bt.calls.Call("SubtensorModule", "set_mechanism_weights_for", {
    "netuid": 1,
    "mecid": 1,
    "hotkey": validator_hotkey,
    "dests": [0, 1, 2],
    "weights": [65_535, 39_321, 26_214],
    "version_key": 0,
})
await client.submit_call(raw, operator_wallet, signer="hotkey", allow_raw_calls=True)

# Raw delegate grant, index 170. Pass None for delegate to revoke.
raw = bt.calls.Call("SubtensorModule", "set_mech_weight_delegate", {
    "netuid": 1, "mecid": 1, "delegate": operator_hotkey,
})
await client.submit_call(raw, validator_wallet, signer="hotkey", allow_raw_calls=True)

# Legacy delegated commit/reveal: indices 172 and 173.
commit = bt.calls.Call("SubtensorModule", "commit_mechanism_weights_for", {
    "netuid": 1, "mecid": 1, "hotkey": validator_hotkey,
    "commit_hash": commit_hash_32_bytes,
})
reveal = bt.calls.Call("SubtensorModule", "reveal_mechanism_weights_for", {
    "netuid": 1, "mecid": 1, "hotkey": validator_hotkey,
    "uids": [0, 1, 2], "values": [65_535, 39_321, 26_214],
    "salt": [1, 2, 3], "version_key": 0,
})

# Timelocked delegated commit: index 174.
sealed = bt.calls.Call(
    "SubtensorModule", "commit_timelocked_mechanism_weights_for",
    {"netuid": 1, "mecid": 1, "hotkey": validator_hotkey,
     "commit": ciphertext, "reveal_round": reveal_round,
     "commit_reveal_version": 4},
)`} />
          <p>
            Validators should grant a fresh operational hotkey only for the intended mechanism,
            revoke it when no longer needed, and use <code>for_hotkey</code> deliberately. An
            independent re-registration does not inherit an old grant. A hotkey swap moves a grant
            through bounded work only while the destination owns the validator&apos;s former UID.
            Miners need no migration, but weight-reading indexers should regenerate metadata for
            the new <code>*_for</code> calls.
          </p>
        </section>

        <section className={styles.section} id='basket'>
          <h2 className={styles.subtitle}>Basket trade band with legacy defaults</h2>
          <p>
            Root can configure the per-leg trade band, price reference, and turnover-bucket refill
            period. Missing storage defaults preserve legacy behavior:{' '}
            <code>StrictestEmaAndSpot</code>, a 200-bps (2%) band, and a 7200-block refill. No
            migration or post-upgrade setter is needed to retain those values. The sell floor uses
            the maximum of slow EMA, fast EMA, and spot; the buy ceiling uses their minimum. Both
            moving prices must exist in this mode.
          </p>
          <Code language='bash' code={`# Root-only calls; --dry-run previews without submitting.
btcli call AdminUtils.sudo_set_basket_trade_max_slippage_bps \\
  --args '{"bps": 500}' --sudo -w sudo_key
btcli call AdminUtils.sudo_set_basket_trade_refill_blocks \\
  --args '{"blocks": 7200}' --sudo -w sudo_key
btcli call AdminUtils.sudo_set_basket_trade_price_reference \\
  --args '{"reference": "SpotOnly"}' --sudo -w sudo_key`} />
          <Code language='python' code={`# Root-only raw governance call, AdminUtils index 128.
inner = await client.compose(bt.calls.Call(
    "AdminUtils", "sudo_set_basket_trade_max_slippage_bps", {"bps": 500},
))
await client.submit_call(
    bt.calls.Sudo.sudo(call=inner), sudo_wallet, allow_raw_calls=True,
)

# Index 130: choose the reference explicitly; StrictestEmaAndSpot is the default.
inner = await client.compose(bt.calls.Call(
    "AdminUtils", "sudo_set_basket_trade_price_reference",
    {"reference": "SpotOnly"},
))
await client.submit_call(
    bt.calls.Sudo.sudo(call=inner), sudo_wallet, allow_raw_calls=True,
)

# Read the new storage through current generated metadata.
band = await client.query("SubtensorModule", "BasketTradeMaxSlippageBps")
reference = await client.query("SubtensorModule", "BasketTradePriceReferenceMode")
refill = await client.query("SubtensorModule", "BasketTradeRefillBlocks")`} />
          <h3>Planned post-merge sudo calls</h3>
          <p>
            These are proposed governance actions, not defaults and not actions performed by the
            runtime upgrade. Submit them separately in this order:
          </p>
          <ol>
            <li><code>AdminUtils::sudo_set_basket_trade_max_slippage_bps(500)</code>.</li>
            <li><code>AdminUtils::sudo_set_basket_trade_price_reference(SpotOnly)</code>.</li>
            <li><code>AdminUtils::sudo_set_basket_concentration_cap(9830)</code>.</li>
          </ol>
          <p>
            <code>SpotOnly</code> drops the slow and fast EMA anchors, so a manipulated spot price
            within the configured band can be traded against. Selecting it is a deliberate sudo
            decision. The third call uses the existing concentration-cap setter and is not changed
            by this release. See the{' '}
            <DocLink href='/docs/guides/basket-trading-governance'>basket governance guide</DocLink>.
          </p>
        </section>

        <section className={styles.section} id='etao'>
          <h2 className={styles.subtitle}>Etáo owner updates once per tempo</h2>
          <p>
            <code>SubnetEmissionWeightsRateLimit</code> is now storage with a production default of{' '}
            <strong>360 blocks</strong>, one default tempo, rather than the former hard-coded
            50,400-block interval. Root can change it through AdminUtils 131. Zero disables the
            interval. The limit remains per subnet: root is exempt, a subnet&apos;s first owner
            call is always allowed, and dissolution clears the old key so a reused netuid starts
            fresh. It applies to owner emission-vector updates, not miners&apos; weight updates.
          </p>
          <Code language='bash' code={`# Set the global per-subnet interval to one default tempo.
btcli call AdminUtils.sudo_set_subnet_emission_weights_rate_limit \\
  --args '{"rate_limit": 360}' --sudo -w sudo_key

# Read the generated storage descriptor from Python as shown below.
# The existing subnet-emission status view continues to show owner updates.
btcli subnets emission show --netuid 1`} />
          <Code language='python' code={`# Root-only raw call, AdminUtils index 131.
inner = await client.compose(bt.calls.Call(
    "AdminUtils", "sudo_set_subnet_emission_weights_rate_limit",
    {"rate_limit": 360},
))
await client.submit_call(
    bt.calls.Sudo.sudo(call=inner), sudo_wallet, allow_raw_calls=True,
)

interval = await client.query(
    "SubtensorModule", "SubnetEmissionWeightsRateLimit",
)
assert interval == 360`} />
          <p>
            Subnet owners should update owner tooling to read the configured interval rather than
            assume a weekly wait. An immediate second owner call can be rate-limited even after a
            successful first call. Miners and validators do not need to migrate emission state;
            indexers should add the new storage item and AdminUtils event.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Upgrade checklist</h2>
          <ul>
            <li><strong>Node operators:</strong> build and deploy the final v494 tag selected by governance; verify the runtime and transaction versions from metadata.</li>
            <li><strong>Validators:</strong> renew SDK metadata, authorize delegates per mechanism, and keep the validator identity in every delegated call.</li>
            <li><strong>Miners:</strong> no state migration is required; update metadata and respect bounded channel migration status when publishing.</li>
            <li><strong>Subnet owners:</strong> channel burns are off unless explicitly configured; review each channel&apos;s live price, and read the Etao interval from storage.</li>
            <li><strong>SDK and indexer operators:</strong> regenerate calls, storage, errors, events, and transaction-version handling. Do not use a hard-coded old call map.</li>
            <li><strong>Governance:</strong> the three basket calls above are separate post-merge decisions; the upgrade itself preserves legacy basket behavior.</li>
          </ul>
          <p>
            Once the release train creates the final tag, build it from a clean checkout:
          </p>
          <Code language='bash' code={`git fetch https://github.com/RaoFoundation/subtensor.git tag v494
git checkout --detach v494
cargo build --release --locked -p node-subtensor`} />
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
