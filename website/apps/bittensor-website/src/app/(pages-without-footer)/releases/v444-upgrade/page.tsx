import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'The V444 Upgrade — EVM, btcli, and Reliability',
  description:
    'V444 completes the EVM surface, makes btcli safer for multisigs and automation, adds ' +
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
          <h1 className={styles.paper_title}>The V444 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            EVM, btcli, and Reliability · August 2026
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            Spec <strong>444</strong> makes the chain substantially easier to use from every
            external surface. Solidity contracts gain 31 functions across five new Bittensor
            precompiles, plus 135 additions to existing interfaces. A saved multisig now behaves
            like a wallet throughout <code>btcli</code>. Automated dry runs carry enough information
            to approve and replay a transaction safely. Ledger users can read the actual fields of a
            limit order before signing it. Underneath those interfaces, v444 recycles transaction
            fees and corrects transaction-pool validation, proxy charging, commitment cleanup,
            storage growth, and GRANDPA finality.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>The runtime, typed for Solidity</h2>
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
                <td>Whether a precompile is currently disabled</td>
              </tr>
            </tbody>
          </table>
          <p>
            Existing precompiles gain 68 state-changing methods and 67 typed views across staking
            V2, neurons, subnets, alpha, balances, proxies, leasing, crowdloans, UID lookup, voting
            power, and transfer surfaces. The additions include typed registration and identity
            operations, weight and commitment calls, stake and collateral management, subnet
            configuration, global and per-subnet state, and a maintained total-voting-power view.
            The{' '}
            <DocLink href='/docs/guides/evm/precompiles/extrinsic-coverage'>coverage audit</DocLink>{' '}
            inventories the deliberate exclusions: Root-only, unsigned, inherent, disabled, and
            compatibility-only calls are not made reachable by pretending an EVM caller has a
            stronger origin.
          </p>
          <p>
            Every state-changing method dispatches the highest-level runtime call as the mapped EVM
            signer. The pallet still enforces ownership, role, rate limits, freeze windows, and
            every other authorization rule. Released addresses and selectors remain stable; the new
            registry gives contracts a typed way to discover whether a whole precompile is currently
            disabled. Supported selectors remain defined by the published interfaces and ABIs.
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
            In v444, only the registry&apos;s <code>isDisabled</code> field is populated. Its
            selector parameter and selector-lifecycle fields are reserved for a future extension and
            must not be used to infer whether a selector exists.
          </p>
          <p>
            Canonical Solidity interfaces, JSON ABIs, generated Python ABI copies, documentation,
            gas accounting, and tests ship together. Integrators should use the v444 copies rather
            than reconstructing selectors from release notes. The new maintainer documentation also
            fixes the rules for ABI versioning, state exposure, lifecycle metadata, coverage, and
            backwards compatibility so later runtime releases can extend this surface without
            breaking deployed contracts.
          </p>
          <p>
            Some existing staking reads now scan more stake records, so their gas estimate is
            higher. The affected methods are <code>getTotalHotkeyStake</code>,{' '}
            <code>getTotalColdkeyStake</code>, and <code>getTotalColdkeyStakeOnSubnet</code>.
            Contracts and services should estimate these calls again after the upgrade and avoid
            hard-coded gas limits. The selectors and return values have not changed.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>A multisig is now a wallet</h2>
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
          <h2 className={styles.subtitle}>Read the order before signing it</h2>
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
limit price <price>, expiry <unix timestamp ms>, hotkey <ss58>, fee <rate> to <ss58>,
relayer <policy>, max slippage <value>, chain <id>,
partial fills <true|false>, signer <ss58>`}
          />
          <p>
            The signed message uses raw integer units: <code>amount</code> is rao for a buy and raw
            alpha units for a sell; <code>price</code> uses a ×10<sup>9</sup> scale; fee and
            slippage use parts per billion; and <code>expiry</code> is a Unix timestamp in
            milliseconds. Frontends may show friendlier values alongside the message, but must sign
            these exact integers.
          </p>
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
          <h2 className={styles.subtitle}>Failures get cheaper, state stays smaller</h2>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Area</th>
                <th>Change in v444</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Transaction fees</td>
                <td>
                  Native TAO fees and EVM fees are recycled instead of paid to the block author.
                  Eligible alpha-paid fees are sold for TAO and recycled in the same transaction.
                </td>
              </tr>
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
                  Proxy calls propagate the inner call&apos;s actual post-dispatch weight, refunding
                  unused worst-case weight. This applies to <code>proxy</code> and{' '}
                  <code>proxy_announced</code>.
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
                  warp-finality and concluded-round cleanup fixes.
                </td>
              </tr>
            </tbody>
          </table>
          <p>
            Transaction fees are separate from swap fees; swap fees still go to the block author.
            The other changes do not introduce new operator workflows. They move predictable
            failures out of blocks, stop deleted identities from leaving chargeable state behind,
            return overestimated proxy weight, and keep historical defaults from accumulating
            forever.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>What to do</h2>
          <ul className={styles.list}>
            <li>
              <strong>EVM integrators:</strong> refresh the complete canonical ABI set before using
              v444 selectors. Add <code>0x…080f</code> through <code>0x…0813</code> only from the
              published interfaces, and use the registry to inspect whole-precompile availability.
              Re-estimate aggregate staking reads and do not rely on fixed gas stipends.
            </li>
            <li>
              <strong>SDK and CLI users:</strong> older clients that read current chain metadata can
              keep using existing commands. To use the new v444 features, upgrade to{' '}
              <code>bittensor 11.1.0</code> and <code>bittensor-core 0.1.3</code> alongside the
              runtime upgrade. Existing wallet files remain usable, and saved multisigs can now be
              passed directly as <code>-w</code>. Rebuild any offline signing payload prepared
              before the runtime upgrade.
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
