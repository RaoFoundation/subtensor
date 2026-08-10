import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V444 Upgrade — Pure Price Emissions',
  description:
    'Subnet emission returns to pure price EMA through the emission gate, while v444 ' +
    'completes the EVM surface, makes btcli safer for multisigs and automation, adds ' +
    'human-readable Ledger orders, and lands a broad reliability pass.',
  alternates: {canonical: '/releases/v444-upgrade'},
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
          <p className={styles.paper_title}>The V444 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Pure Price Emissions · August 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Spec <strong>444</strong> makes the market signal simple again: a subnet&apos;s share of
            network emission is determined by its moving price, passed through the emission gate.
            The share is no longer reduced when a subnet directs miner incentive to an owner or burn
            hotkey. Recycling and burning still do exactly what the subnet chose locally; they no
            longer change its standing against every other subnet.
          </p>
          <p>
            The release also makes the chain substantially easier to use from every external
            surface. Solidity contracts gain five new Bittensor precompiles and 69 functions on
            existing interfaces. A saved multisig now behaves like a wallet throughout
            <code>btcli</code>. Automated dry runs carry enough information to approve and replay a
            transaction safely. Ledger users can read the actual fields of a limit order before
            signing it. Underneath those interfaces, v444 corrects transaction-pool validation,
            proxy charging, commitment cleanup, storage growth, and GRANDPA finality.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Price is the signal</p>
          <p>
            The emission gate introduced in <DocLink href='/releases/v440-upgrade'>v440</DocLink>{' '}
            starts with each subnet&apos;s share of price EMA, then suppresses emission below the
            market-set bar. A second factor had been applied before that gate:{' '}
            <code>1 − MinerBurned</code>. That meant two subnets with the same demand could receive
            different cross-network emission solely because one withheld miner incentive for
            recycling or burning.
          </p>
          <Code
            language='rust'
            code={`before v444:  s_i = normalize(price_ema_i × (1 − miner_burned_i))
              e_i ∝ s_i × gate(s_i)

v444:         s_i = normalize(price_ema_i)
              e_i ∝ s_i × gate(s_i)`}
          />
          <p>
            V444 removes that extra multiplier. <code>MinerBurned</code> remains on-chain as an
            informational measure, and the miner-incentive path still recycles or burns according to
            the subnet&apos;s configuration. What changes is the boundary between local token policy
            and network allocation: demand determines how much emission a subnet earns; the subnet
            determines what it does with the miner portion after that.
          </p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Miner incentive policy</th>
                <th>Effect on subnet&apos;s network share in v444</th>
                <th>Local effect</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Paid to miners</td>
                <td>Price EMA through the gate</td>
                <td>Miner alpha is distributed</td>
              </tr>
              <tr>
                <td>Withheld and recycled</td>
                <td>Price EMA through the gate</td>
                <td>Value returns through the recycle path</td>
              </tr>
              <tr>
                <td>Withheld and burned</td>
                <td>Price EMA through the gate</td>
                <td>Value is removed by the burn path</td>
              </tr>
            </tbody>
          </table>
          <p>
            Subnet teams do not need to change a setting. Forecasting software should remove the
            miner-burn factor from cross-subnet share calculations and retain it only where it
            describes the subnet&apos;s own incentive accounting.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>The runtime, typed for Solidity</p>
          <p>
            The Bittensor precompile suite now covers the deterministic, typed runtime surface that
            an EVM caller is authorized to use. Five new domain addresses expose system state that
            previously required a Substrate client or duplicated constants:
          </p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Address</th>
                <th>Precompile</th>
                <th>What contracts can inspect</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>
                  <code>0x…080f</code>
                </td>
                <td>Scheduler</td>
                <td>Scheduled calls, retry state, task addresses, and incomplete work</td>
              </tr>
              <tr>
                <td>
                  <code>0x…0810</code>
                </td>
                <td>Drand</td>
                <td>Beacon configuration, pulses, stored rounds, and unsigned timing</td>
              </tr>
              <tr>
                <td>
                  <code>0x…0811</code>
                </td>
                <td>Timestamp</td>
                <td>Runtime timestamp and whether it was updated in the current block</td>
              </tr>
              <tr>
                <td>
                  <code>0x…0812</code>
                </td>
                <td>Runtime configuration</td>
                <td>
                  Chain ID and grouped economic, consensus, registration, and pallet constants
                </td>
              </tr>
              <tr>
                <td>
                  <code>0x…0813</code>
                </td>
                <td>Precompile registry</td>
                <td>Whether a selector is deprecated, disabled, or replaced</td>
              </tr>
            </tbody>
          </table>
          <p>
            Existing precompiles gain another 69 functions across staking V2, neurons, subnets,
            alpha, balances, proxies, leasing, crowdloans, UID lookup, voting power, and transfer
            surfaces. The additions include typed registration and identity operations, weight and
            commitment calls, stake and collateral management, subnet configuration, global and
            per-subnet state, and a maintained total-voting-power view. The{' '}
            <DocLink href='/docs/guides/evm/precompiles/extrinsic-coverage'>coverage audit</DocLink>{' '}
            inventories the deliberate exclusions: Root-only, unsigned, inherent, disabled, and
            compatibility-only calls are not made reachable by pretending an EVM caller has a
            stronger origin.
          </p>
          <p>
            Every state-changing method dispatches the highest-level runtime call as the mapped EVM
            signer. The pallet still enforces ownership, role, rate limits, freeze windows, and
            every other authorization rule. Released addresses and selectors remain stable; the new
            registry gives contracts a typed way to discover lifecycle and operational status before
            calling.
          </p>
          <Code
            language='solidity'
            code={`IPrecompileRegistry registry =
    IPrecompileRegistry(0x0000000000000000000000000000000000000813);

IPrecompileRegistry.PrecompileStatus memory status =
    registry.getPrecompileStatus(target, selector);

IRuntimeConfiguration config =
    IRuntimeConfiguration(0x0000000000000000000000000000000000000812);

uint256 chainId = config.getEvmChainId();`}
          />
          <p>
            Canonical Solidity interfaces, JSON ABIs, generated Python ABI copies, documentation,
            gas accounting, and tests ship together. Integrators should use the v444 copies rather
            than reconstructing selectors from release notes. The new maintainer documentation also
            fixes the rules for ABI versioning, state exposure, lifecycle metadata, coverage, and
            backwards compatibility so later runtime releases can extend this surface without
            breaking deployed contracts.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>A multisig is now a wallet</p>
          <p>
            The v11 CLI no longer makes operators translate a multisig workflow into low-level
            approvals by hand. Save a signer set once, then pass its name wherever a coldkey wallet
            is accepted. Reads resolve the derived account. Writes select a local member, wrap the
            intended call, find an existing approval round, and supply its timepoint. A co-signer
            completes the round by running the same command.
          </p>
          <Code
            language='bash'
            code={`btcli multisig add team-treasury \
  --threshold 2 --signatories alice,bob,carol

# Alice opens the operation; Bob runs the identical command to complete it.
btcli wallet transfer --dest 5F... --amount-tao 10 -w team-treasury`}
          />
          <p>
            Before signing, <code>--dry-run --json</code> now returns the parsed arguments, exact
            spend or an explicit unbounded-spend marker, estimated fee, warnings, policy verdict,
            and a replay command that submits the same invocation without the dry-run flags. That
            makes a plan reviewable by a human, an agent, or a policy engine without asking any of
            them to infer intent from prose.
          </p>
          <Code
            language='bash'
            code={`btcli tx transfer \
  --dest 5F... --amount-tao 10 -w team-treasury \
  --dry-run --json`}
          />
          <p>
            The same release improves everyday staking: <code>stake add</code> shows free balance,
            accepts <code>all</code>, offers local hotkeys without requiring the target to live on
            disk, and accepts pasted or address-book targets. <code>stake burn</code> joins the
            normal command tree with a price-aware default. Names resolve inside raw-call JSON,
            multisig approvals fail early when the signer cannot cover the deposit and fee, and
            wrapper order is preserved so intent safety survives multisig dispatch. Root position
            rows now stay aligned with their human table columns, and explicit validator details
            produce one consolidated JSON document.
          </p>
          <p>
            Secret-bearing flags now warn that values are visible in shell history and the process
            list; omitting them uses a hidden prompt. EVM private-key export prefers the clipboard
            on a terminal, and wallet regeneration accepts 64-byte sr25519 private keys. Keyfiles
            once again include the legacy fields expected by older subnet tooling, while the reader
            tolerates older encodings and gives specific guidance for browser and mobile exports.
            The complete operational flow is in the{' '}
            <DocLink href='/docs/guides/multisig'>multisig guide</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Read the order before signing it</p>
          <p>
            <DocLink href='/releases/v438-upgrade'>V438</DocLink> let Ledger and compatible signers
            authorize limit orders by signing a wrapped order hash. V444 adds an alternative
            clear-signing form: one canonical printable message containing the order type, amount,
            subnet, limit or trigger price, expiry, hotkey, relayer policy, fee, slippage, chain ID,
            partial-fill policy, and signer. The hardware wallet displays those fields before
            approval, and the runtime deterministically rebuilds the same message before accepting
            the signature.
          </p>
          <Code
            language='text'
            code={`TAO.com order v1: Limit buy <amount> on subnet <netuid>,
limit price <price>, expiry <block>, hotkey <ss58>, fee <rate> to <ss58>,
relayer <policy>, max slippage <value>, chain <id>,
partial fills <true|false>, signer <ss58>`}
          />
          <p>
            This format is additive. Existing raw SCALE signatures and wrapped-hash signatures
            remain valid, and every format resolves to the same canonical order ID for replay
            protection, cancellation, relayer restrictions, and partial fills. Sr25519 and ed25519
            remain supported; ECDSA remains rejected. Rust and TypeScript parity tests plus
            device-derived Ledger vectors lock the exact bytes so a frontend cannot show one order
            and submit another.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Failures get cheaper, state stays smaller</p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Area</th>
                <th>Change in v444</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Commit transactions</td>
                <td>
                  Deterministic failures are rejected during transaction validation; competing
                  commits in the same signer and rate-limit lane conflict in the pool.
                </td>
              </tr>
              <tr>
                <td>Neuron trimming</td>
                <td>
                  Deregistration purges active and revealed commitments, metadata, usage and
                  timelock indexes, and releases the associated commitment deposit.
                </td>
              </tr>
              <tr>
                <td>Proxy fees</td>
                <td>
                  <code>proxy</code> and <code>proxy_announced</code> propagate the inner
                  call&apos;s actual post-dispatch weight, refunding unused worst-case weight.
                </td>
              </tr>
              <tr>
                <td>Storage</td>
                <td>
                  A bounded, resumable <code>on_idle</code> migration removes obsolete pre-dTAO
                  prefixes and explicit zero rows; abandoned Swap V3 state is cleared separately.
                </td>
              </tr>
              <tr>
                <td>Voting power</td>
                <td>
                  A maintained subnet total avoids repeated aggregation, and neuron removal or
                  subnet dissolution now clears the corresponding voting-power state.
                </td>
              </tr>
              <tr>
                <td>GRANDPA</td>
                <td>
                  The Polkadot SDK is pinned to fork revision <code>cacb4310</code>, including
                  warp-finality and concluded-round cleanup fixes; testnet&apos;s warp checkpoint
                  now carries the correct authority set and set ID.
                </td>
              </tr>
            </tbody>
          </table>
          <p>
            These changes do not introduce new operator workflows. They move predictable failures
            out of blocks, stop deleted identities from leaving chargeable state behind, return
            overestimated proxy weight, and keep historical defaults from accumulating forever.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <ul className={styles.list}>
            <li>
              <strong>Node operators:</strong> wait for the on-chain <code>spec_version</code> to
              move to 444, then update to the matching release. Testnet operators should update
              promptly for the corrected GRANDPA warp checkpoint.
            </li>
            <li>
              <strong>Subnet teams and analysts:</strong> remove <code>1 − MinerBurned</code> from
              cross-subnet emission forecasts. The emission gate remains active and miner recycling
              or burning remains a local policy.
            </li>
            <li>
              <strong>EVM integrators:</strong> refresh the complete canonical ABI set before using
              v444 selectors. Add <code>0x…080f</code> through <code>0x…0813</code> only from the
              published interfaces, and use the registry to inspect selector status.
            </li>
            <li>
              <strong>SDK and CLI users:</strong> install the matching <code>bittensor 11.1.0</code>
              release and <code>bittensor-core 0.1.3</code>. Existing wallet files remain usable;
              saved multisigs can now be passed directly as <code>-w</code>.
            </li>
            <li>
              <strong>Limit-order applications:</strong> add the human-readable signing format for
              hardware-wallet users. Do not remove raw or wrapped-hash support; all three formats
              remain valid.
            </li>
          </ul>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v444 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/docs/guides/evm/precompiles'>Read the complete precompile reference</Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
