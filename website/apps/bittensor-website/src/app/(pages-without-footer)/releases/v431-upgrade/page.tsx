import FadeInWrapper from '@/app/components/FadeInWrapper';
import {MenuSchema} from '@/app/components/Header/MenuSchema';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'The V431 Upgrade',
  description:
    'One repository, one package, new documentation, and new network economics: ' +
    'conviction-based subnet ownership, price-driven emissions, and the bittensor v11 SDK.',
  alternates: {canonical: '/releases/v431-upgrade'},
  openGraph: {images: '/images/og_thumbs/v431-upgrade.png'},
};

const CONNECT_LINK_ORDER = ['DISCORD', 'X', 'GITHUB'] as const;
const connectLinks = CONNECT_LINK_ORDER.map((label) =>
  MenuSchema.connect.find((item) => item.label.toUpperCase() === label),
).filter((link): link is (typeof MenuSchema)['connect'][number] => Boolean(link));

const DocLink = ({href, children}: {href: string; children: React.ReactNode}) => (
  <Link href={href} className={styles.inline_link}>
    {children}
  </Link>
);

const GRAPH_TEXT = {
  fontFamily: 'FiraCode',
  fontSize: 10,
  fill: 'rgb(41, 41, 41)',
} as const;

const ConvictionGraph = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 340'
    role='img'
    aria-label='Total conviction rising over time; once the subnet is older than one year and conviction exceeds ten percent of outstanding alpha, ownership is contestable.'
  >
    {/* Region where both conditions hold */}
    <rect x='508' y='40' width='222' height='250' fill='rgba(209, 81, 104, 0.07)' />
    <text {...GRAPH_TEXT} x='619' y='60' textAnchor='middle' fill='#d15168'>
      OWNERSHIP
    </text>
    <text {...GRAPH_TEXT} x='619' y='74' textAnchor='middle' fill='#d15168'>
      CONTESTABLE
    </text>

    {/* Axes */}
    <line x1='70' y1='30' x2='70' y2='290' stroke='rgb(41, 41, 41)' strokeWidth='1' />
    <line x1='70' y1='290' x2='730' y2='290' stroke='rgb(41, 41, 41)' strokeWidth='1' />
    <text {...GRAPH_TEXT} x='730' y='310' textAnchor='end'>
      TIME
    </text>
    <text {...GRAPH_TEXT} x='62' y='293' textAnchor='end'>
      0%
    </text>
    <text {...GRAPH_TEXT} x='62' y='173' textAnchor='end'>
      10%
    </text>
    <text {...GRAPH_TEXT} x='62' y='53' textAnchor='end'>
      20%
    </text>

    {/* 10% threshold */}
    <line
      x1='70'
      y1='170'
      x2='730'
      y2='170'
      stroke='rgba(41, 41, 41, 0.5)'
      strokeWidth='1'
      strokeDasharray='4 4'
    />
    <text {...GRAPH_TEXT} x='76' y='162'>
      CONVICTION THRESHOLD: 10% OF OUTSTANDING ALPHA
    </text>

    {/* Subnet age = 1 year */}
    <line
      x1='430'
      y1='40'
      x2='430'
      y2='290'
      stroke='rgba(41, 41, 41, 0.5)'
      strokeWidth='1'
      strokeDasharray='4 4'
    />
    <text {...GRAPH_TEXT} x='430' y='28' textAnchor='middle'>
      SUBNET AGE = 1 YEAR
    </text>

    {/* Total conviction accrued by lockers */}
    <path
      d='M 70 288 C 250 280, 360 245, 460 195 C 560 145, 640 105, 730 85'
      fill='none'
      stroke='rgb(41, 41, 41)'
      strokeWidth='1.5'
    />
    <text {...GRAPH_TEXT} x='100' y='268'>
      TOTAL CONVICTION
    </text>

    {/* Point where the threshold is crossed past one year of age */}
    <circle cx='508' cy='170' r='4' fill='#d15168' />
  </svg>
);

const EMISSION_BARS = {
  before: [150, 108, 70],
  after: [109, 109, 109],
  ages: ['3 MO', '1 YR', '2 YR'],
} as const;

const EmissionGraph = () => (
  <svg
    className={styles.graph}
    viewBox='0 0 760 340'
    role='img'
    aria-label='Before v431, three subnets with the same price received progressively less emission with age; after v431, their emission shares are equal because price alone determines the split.'
  >
    {(['before', 'after'] as const).map((panel, p) => {
      const x0 = p === 0 ? 80 : 425;
      return (
        <g key={panel}>
          <text {...GRAPH_TEXT} x={x0 + 128} y='40' textAnchor='middle'>
            {panel === 'before' ? 'BEFORE V431' : 'AFTER V431'}
          </text>
          <text
            {...GRAPH_TEXT}
            x={x0 + 128}
            y='58'
            textAnchor='middle'
            fill='rgba(41, 41, 41, 0.55)'
          >
            {panel === 'before' ? 'SAME PRICE, LESS WITH AGE' : 'EMISSION FOLLOWS PRICE'}
          </text>
          {EMISSION_BARS[panel].map((h, i) => {
            const x = x0 + i * 90;
            return (
              <g key={i}>
                <rect
                  x={x}
                  y={270 - h}
                  width='56'
                  height={h}
                  fill={panel === 'before' ? 'rgba(41, 41, 41, 0.12)' : 'rgba(209, 81, 104, 0.12)'}
                  stroke={panel === 'before' ? 'rgb(41, 41, 41)' : '#d15168'}
                  strokeWidth='1'
                />
                <text {...GRAPH_TEXT} x={x + 28} y='288' textAnchor='middle'>
                  {EMISSION_BARS.ages[i]}
                </text>
              </g>
            );
          })}
          <line
            x1={x0 - 10}
            y1='270'
            x2={x0 + 266}
            y2='270'
            stroke='rgb(41, 41, 41)'
            strokeWidth='1'
          />
        </g>
      );
    })}
    <text {...GRAPH_TEXT} x='380' y='168' textAnchor='middle' fontSize='16'>
      {'\u2192'}
    </text>
    <text {...GRAPH_TEXT} x='380' y='320' textAnchor='middle' fill='rgba(41, 41, 41, 0.55)'>
      THREE SUBNETS, IDENTICAL MOVING-AVERAGE PRICE
    </text>
  </svg>
);

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The V431 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Written by Arbos
          </p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            The chain now runs
            <strong> spec version 431</strong>, and the release changes both the network&apos;s
            economics and the software used to interact with it. Subnet ownership is now
            contestable through a time-weighted commitment mechanism called{' '}
            <DocLink href='/docs/guides/conviction'>conviction</DocLink>. Emission between
            subnets is now allocated purely in proportion to each subnet&apos;s moving-average
            price. The Python SDK and the btcli command line now ship together as{' '}
            <strong>bittensor v11</strong>, built on a new Rust core. The documentation has
            been rebuilt at <DocLink href='/docs'>bittensor.com/docs</DocLink>. And the chain,
            SDK, CLI, documentation, and website are now developed and released together from a
            single repository.
          </p>
          <p>
            This page explains each change, the reasoning behind it, and the actions required
            of network participants. Each section links to the relevant documentation.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Ownership by conviction</p>
          <p>
            Locking alpha on a subnet accrues <strong>conviction</strong>: a time-weighted
            commitment score credited to a hotkey chosen by the locker. Prior to this upgrade,
            conviction was recorded on-chain but had no effect. As of spec 431, it governs
            subnet ownership:
          </p>
          <p>
            <strong>
              If a subnet is more than one year old, and the total conviction across its
              lockers exceeds ten percent of its outstanding alpha, ownership of the subnet —
              including the owner&apos;s share of emissions — transfers to the hotkey with the
              highest conviction.
            </strong>
          </p>
          <ConvictionGraph />
          <p className={styles.graph_caption}>
            Ownership becomes contestable once both conditions hold: the subnet is older than
            one year, and total conviction exceeds ten percent of its outstanding alpha. At
            that point the hotkey with the highest conviction takes ownership.
          </p>
          <p>
            Subnet ownership is therefore no longer fixed at registration; it is contestable
            through open, on-chain rules. Two lock modes are available, and both are
            exponential processes rather than fixed terms. A perpetual lock&apos;s conviction
            approaches its locked mass asymptotically — it never quite completes. The
            chain&apos;s <i>maturity rate </i>sets the exponential time constant, roughly 43
            days at current values: after one time constant conviction stands at about 63% of
            the locked mass, after two about 86%, and so on. A decaying lock — the default —
            frees its locked mass on the chain&apos;s <i>unlock rate</i>, a time constant of
            roughly 130 days, after which about 37% of the mass remains locked; its conviction
            peaks and then unwinds. Both rates are governance-set storage values — read them
            from chain state before planning a lock rather than relying on the figures here —
            and there is one exception: locks credited to the subnet owner&apos;s own hotkey
            mature instantly, so their conviction always equals their locked mass. The mechanism is designed to reward long-horizon commitment to a
            subnet&apos;s success. The lock modes, the conviction formula, and a worked example
            are documented in the{' '}
            <DocLink href='/docs/guides/conviction'>conviction guide</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Emissions, simplified</p>
          <p>
            Each block, the chain divides TAO emission between subnets. As of this upgrade,
            that division is determined solely by each subnet&apos;s
            <strong> moving-average price</strong>, weighted by a miner-burn penalty. The
            root-proportion term has been removed from the cross-subnet calculation.
            Previously, this term reduced a subnet&apos;s emission share as its alpha issuance
            grew, which structurally disadvantaged older subnets. Root proportion continues to
            operate <i>within </i>each subnet — capping liquidity injection and reserving the
            root stakers&apos; share of dividends — but it no longer affects how emission is
            divided between subnets.
          </p>
          <EmissionGraph />
          <p className={styles.graph_caption}>
            Three subnets with an identical moving-average price. Before v431, the
            root-proportion term reduced each subnet&apos;s share as its alpha issuance grew;
            after, the same price earns the same emission regardless of age.
          </p>
          <p>
            The result is that a subnet&apos;s emission share is a direct function of its
            market price. The full formula and its parameters are documented in{' '}
            <DocLink href='/docs/concepts/emissions'>emissions</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>One package: bittensor v11</p>
          <p>
            <strong>bittensor v11</strong> consolidates the SDK and the btcli command line into
            a single package, installed with <strong>pip install bittensor</strong>. It
            replaces the separate bittensor-cli and bittensor-wallet packages. Existing wallet
            keyfiles are unchanged and fully compatible.
          </p>
          <p>
            The package is built on a new Rust core covering keys, keyfiles, encoding, and
            timelock encryption. The following measurements were taken against the live
            network, before and after the change:
          </p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Operation</th>
                <th>v10</th>
                <th>v11</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Startup codec build (every btcli invocation)</td>
                <td>337 ms</td>
                <td>4 ms</td>
              </tr>
              <tr>
                <td>Metagraph decode throughput</td>
                <td>3–6 MB/s</td>
                <td>60–77 MB/s</td>
              </tr>
              <tr>
                <td>Storage map decode</td>
                <td>92k entries/s</td>
                <td>599k entries/s</td>
              </tr>
              <tr>
                <td>1,000-operation batch construction</td>
                <td>~1.1 s</td>
                <td>~50 ms</td>
              </tr>
            </tbody>
          </table>
          <p>
            Transaction submission and inclusion remain bound by chain block time; the
            improvements are concentrated in startup, decoding, and construction. v11 is also a
            major revision of the API: the Subtensor class is replaced by a client-and-intent
            model with planning, policy gates, and typed results. The{' '}
            <DocLink href='/docs/migration'>migration guide</DocLink> maps every v9/v10 call to
            its v11 equivalent, and the{' '}
            <DocLink href='/docs/quickstart'>quickstart</DocLink> covers new installations.
          </p>
          <p style={{textAlign: 'left', width: '100%'}}>
            To upgrade an existing environment, uninstall the old packages first — both own the
            btcli command, so order matters:
          </p>
          <pre className={styles.code_block}>
            {`pip uninstall -y bittensor-cli bittensor-wallet
pip install -U bittensor`}
          </pre>
          <p style={{textAlign: 'left', width: '100%'}}>
            In the new SDK, chain state is read through a typed client, and every transaction
            is an intent that can be planned before it is executed:
          </p>
          <pre className={styles.code_block}>
            {`import asyncio
import bittensor as bt
from bittensor.wallet import Wallet

async def main():
    wallet = Wallet(name="my_coldkey", hotkey="my_hotkey")
    async with bt.Subtensor() as client:
        balance = await client.balances.get("5F...coldkey")

        intent = bt.Transfer(dest_ss58="5F...dest", amount_tao=1.5)
        plan = await client.plan(intent, wallet)      # fee and effects; nothing submitted
        result = await client.execute(intent, wallet)
        if not result.success:
            print(result.error.code, result.error.remediation)

asyncio.run(main())`}
          </pre>
          <p style={{textAlign: 'left', width: '100%'}}>
            The CLI is generated from the same catalog: every transaction is a btcli tx
            command and every query a btcli query command, with hand-written groups wrapping
            the familiar workflows and the v9 shorthands preserved as aliases. Every mutation
            supports --dry-run, which shows the fee, the predicted effects, and any policy
            verdict without submitting:
          </p>
          <pre className={styles.code_block}>
            {`btcli config set network finney
btcli wallet balance my_coldkey
btcli query metagraph --netuid 1
btcli tx transfer --dest 5F...dest --amount-tao 1.5 --dry-run
btcli tx transfer --dest 5F...dest --amount-tao 1.5 -w my_coldkey`}
          </pre>
          <p style={{textAlign: 'left', width: '100%'}}>
            Beyond the consolidation, v11 adds capabilities that did not exist in the old
            stack:
          </p>
          <ol className={styles.list}>
            <li>
              <strong>Unit-safe money</strong> — every Balance is tagged with its currency, so
              TAO and subnet alpha cannot be silently mixed; arithmetic across units raises
              instead of producing a wrong number. See{' '}
              <DocLink href='/docs/concepts/money'>money</DocLink>.
            </li>
            <li>
              <strong>Policy guardrails</strong> — a client can be bound to hard limits
              (maximum fee, maximum spend, allowed subnets), and any transaction that would
              exceed them is refused before it is signed. See{' '}
              <DocLink href='/docs/concepts/transactions'>the transaction model</DocLink>.
            </li>
            <li>
              <strong>Typed errors</strong> — every failure returns a semantic error code and
              a remediation hint rather than prose, with the full mapping published as a
              machine-readable catalog. See <DocLink href='/docs/errors'>errors</DocLink>.
            </li>
            <li>
              <strong>Signed requests</strong> — hotkey-signed HTTP between validators and
              miners, so a request provably came from a specific hotkey, covers exactly the
              bytes received, and cannot be replayed. See{' '}
              <DocLink href='/docs/guides/signed-requests'>signed requests</DocLink>.
            </li>
            <li>
              <strong>Timelock encryption</strong> — seal data that anyone can open at a known
              future time and nobody, including the author, can open early; the same mechanism
              that secures commit-reveal weights, exposed directly. See{' '}
              <DocLink href='/docs/guides/timelock'>timelock</DocLink>.
            </li>
            <li>
              <strong>Proxies as a first-class signer</strong> — every transaction accepts
              --proxy-for, so a delegate key can act for a coldkey that never comes online;
              scoped proxy types, announced (delayed) proxies, and pure proxy accounts are all
              supported. See <DocLink href='/docs/concepts/advanced'>advanced operations</DocLink>.
            </li>
            <li>
              <strong>Multisig accounts</strong> — create and operate k-of-n multisig
              accounts, with the full approve, execute, and cancel flow wrapped by the btcli
              multisig command group.
            </li>
            <li>
              <strong>Atomic batches and MEV-shielded submission</strong> — compose several
              intents into one all-or-nothing transaction, or encrypt a coldkey transaction to
              the next block&apos;s ephemeral key so it cannot be observed or front-run in the
              mempool.
            </li>
            <li>
              <strong>Safer key rotation</strong> — hotkey swaps move registrations and stake
              to a new key, and a leaked coldkey can be evacuated through an announced,
              five-day-delayed swap that the real owner can dispute. See{' '}
              <DocLink href='/docs/concepts/wallets'>wallets and keys</DocLink>.
            </li>
            <li>
              <strong>Address safety</strong> — CLI address arguments resolve from a saved
              address book or local key names as well as raw ss58, a defense against address
              poisoning documented in{' '}
              <DocLink href='/docs/concepts/wallets'>address hygiene</DocLink>.
            </li>
          </ol>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Hardware and extension signing</p>
          <p>
            Every transaction can now be signed on a <strong>Ledger hardware wallet</strong>{' '}
            using clear signing. Through merkleized metadata, the device decodes the
            transaction on its own screen, and the chain verifies the same metadata digest that
            was signed; the device rejects any transaction it cannot decode and verify. Any
            command that signs accepts the --ledger flag:
          </p>
          <pre className={styles.code_block}>
            {`btcli tx transfer --dest 5F...dest --amount-tao 1 --ledger
btcli tx transfer --dest 5F...dest --amount-tao 1 --ledger --ledger-account 1`}
          </pre>
          <p>
            Setup and usage are documented in{' '}
            <DocLink href='/docs/guides/ledger'>signing with a Ledger</DocLink>.
          </p>
          <p>
            Transactions can also be signed with a <strong>browser extension</strong> —
            Talisman, Polkadot.js, or SubWallet. The CLI relays the transaction to the
            extension through a local bridge, and only the signature is returned; no keyfile,
            password, or mnemonic is present on the machine running btcli:
          </p>
          <pre className={styles.code_block}>
            {`btcli extension accounts    # list accounts the extensions expose
btcli tx transfer --dest 5F...dest --amount-tao 1 --signer extension`}
          </pre>
          <p>
            The full flow is documented in{' '}
            <DocLink href='/docs/guides/extension-signing'>
              signing with a browser extension
            </DocLink>
            .
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Documentation, rebuilt</p>
          <p>
            The documentation has been rebuilt and now lives at{' '}
            <DocLink href='/docs'>bittensor.com/docs</DocLink>. The reference pages for{' '}
            <strong>all 74 transactions </strong>and <strong>all 82 queries </strong>are
            generated directly from the SDK, so the reference cannot drift from the released
            software. Start at the <DocLink href='/docs'>documentation home</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Built for agents</p>
          <p>
            The entire stack — SDK, CLI, documentation, and this website — is designed to be
            driven by AI agents as well as humans. Every operation is discoverable at runtime
            with a JSON schema (<strong>btcli tools</strong> on the CLI,{' '}
            <strong>bt.intents.list_tools()</strong> in Python) and can be executed by name
            from a plain dictionary, validated against that schema. Every mutation can be
            previewed before it spends anything, every failure returns a machine-readable code
            with a remediation hint, and a Policy can hard-bound what an agent&apos;s session
            is allowed to do — spend caps, fee caps, allowed subnets.
          </p>
          <p>
            The CLI never traps automation: --json produces machine-readable output on any
            command, and a non-interactive session missing a confirmation is declined rather
            than left hanging. The documentation publishes the same catalogs statically —
            intents, reads, and errors as JSON — every page is fetchable as raw markdown, and
            the full corpus is available at a single plain-text endpoint for loading into a
            context window. The complete workflow is documented on{' '}
            <DocLink href='/docs/agents'>the agents page</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>One repository, releases on rails</p>
          <p>
            The chain, SDK, CLI, documentation, and this website are now developed in a single
            repository:{' '}
            <span className={styles.paper_link}>
              <Link href='https://github.com/RaoFoundation/subtensor' isExternal={true}>
                github.com/RaoFoundation/subtensor
              </Link>
            </span>
            . Releases are produced by an automated pipeline. Every runtime change is tested
            against a <i>live clone of mainnet state</i> before it merges; a single
            deterministic build is promoted through devnet and testnet with automated checks at
            each stage; and the upgrade signed by the keyholders is cryptographically verified
            against the exact bytes the pipeline produced. A new public devnet, documented in{' '}
            <DocLink href='/docs/concepts/network'>the network overview</DocLink>, joins finney
            and testnet as a supported environment.
          </p>
          <p>
            The runtime was also hardened in this release: proxy permissions are now
            deny-by-default, a crowdloan reentrancy flaw was closed, and the randomness
            pipeline that secures commit-reveal can no longer be stalled.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Operational breaking changes</p>
          <p>
            Beyond the economics and the new package, several call sites and accounting
            assumptions change under the upgraded runtime. The full checklist lives in the{' '}
            <DocLink href='/docs/migration#chain-and-runtime-changes'>migration guide</DocLink>;
            the short version:
          </p>
          <ol className={styles.list}>
            <li>
              <strong>Stake moves are strict.</strong> <code>move_stake</code>,{' '}
              <code>transfer_stake</code>, and <code>swap_stake</code> no longer silently shrink
              an oversize alpha amount after an alpha-paid fee. Submitting the full stake balance
              can fail — leave dust, or read the post-fee balance first.
            </li>
            <li>
              <strong>Limit stakes refund leftover TAO.</strong> When a price limit stops a
              stake-in before the full amount swaps, the unswapped TAO returns to the coldkey.
              Prefer post-state balances over summing <code>StakeAdded</code> for partial fills.
            </li>
            <li>
              <strong>Insufficient-balance errors split.</strong> SubtensorModule renamed{' '}
              <code>InsufficientBalance</code> to <code>InsufficientTaoBalance</code> and added{' '}
              <code>InsufficientAlphaBalance</code>. Match on the semantic error code, or handle
              both names; other pallets may still emit the old name.
            </li>
            <li>
              <strong>Owner tempo and activity cutoff moved.</strong> Set them through{' '}
              <code>AdminUtils::sudo_set_tempo</code> and{' '}
              <code>AdminUtils::sudo_set_activity_cutoff_factor</code>. The old SubtensorModule
              call indices are retired — retarget raw encodings and Owner proxy prebuilds. The
              live inactivity window is the tempo-relative factor, not the legacy absolute
              blocks value.
            </li>
            <li>
              <strong>Default slippage is on.</strong> Stake intents in v11 apply a 5% price
              bound unless you opt out. Scripts that assumed any-price execution will see{' '}
              <code>SlippageTooHigh</code>.
            </li>
            <li>
              <strong>Take changes pay fees.</strong> <code>increase_take</code> and{' '}
              <code>decrease_take</code> are no longer free extrinsics.
            </li>
            <li>
              <strong>Subnet registration can queue.</strong> Under full capacity or pending
              cleanup, <code>register_network</code> may emit{' '}
              <code>NetworkRegistrationQueued</code> without creating the subnet — wait for{' '}
              <code>NetworkAdded</code>.
            </li>
            <li>
              <strong>Drand rounds are sequential.</strong> After the first pulse, only{' '}
              <code>last_stored_round + 1</code> is accepted; missing rounds must be backfilled
              in order.
            </li>
            <li>
              <strong>Metadata and catalogs changed shape.</strong>{' '}
              <code>get_proxy_filter</code> is now <code>get_proxy_filters</code>; TAO/Alpha and{' '}
              <code>NetUid</code> have distinct metadata types;{' '}
              <code>/catalog/errors.json</code> <code>chain_errors</code> values are objects
              with <code>code</code>, <code>description</code>, and <code>docs_url</code>.
            </li>
          </ol>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What you need to do</p>
          <p style={{textAlign: 'left', width: '100%'}}>
            Most participants require little or no action. In order of urgency:
          </p>
          <ol className={styles.list}>
            <li>
              <strong>Python users</strong> — uninstall bittensor-cli and bittensor-wallet,
              then install the new bittensor package. Follow the{' '}
              <DocLink href='/docs/migration'>migration guide</DocLink>; keyfiles are
              unchanged. Expect default 5% slippage on stake trades, and branch on error{' '}
              <em>codes</em> rather than the old <code>InsufficientBalance</code> name alone.
            </li>
            <li>
              <strong>Proxy users</strong> — permissions are deny-by-default. Review every
              existing grant. Coldkey-swap and dissolve calls are narrower than before; Owner
              proxies that set tempo or activity cutoff must target the AdminUtils calls.
            </li>
            <li>
              <strong>Stakers and automation</strong> — do not submit a full alpha balance when
              fees are paid in alpha; leave dust. If you stake with a price limit, check the
              coldkey balance after inclusion rather than trusting the pre-refund event amount.
            </li>
            <li>
              <strong>Subnet owners</strong> — review the{' '}
              <DocLink href='/docs/guides/conviction'>conviction guide</DocLink>. Ownership of
              subnets older than one year is contestable. Set tempo and{' '}
              <code>activity_cutoff_factor</code> through AdminUtils /{' '}
              <code>btcli sudo set</code>. Emission forecasts that still fold{' '}
              <code>root_proportion</code> into the cross-subnet split are wrong.
            </li>
            <li>
              <strong>Indexers and SDK authors</strong> — regenerate bindings for typed
              currency units and <code>get_proxy_filters</code>; handle{' '}
              <code>NetworkRegistrationQueued</code>; treat limit-stake{' '}
              <code>StakeAdded</code> / volume deltas cautiously until you confirm consumed
              amounts; update any parser of <code>errors.json</code> chain_errors.
            </li>
            <li>
              <strong>Node operators and drand submitters</strong> — no binary upgrade is
              required to keep syncing Wasm; install the latest node image when published for
              node-software updates. Custom pulse submitters must backfill rounds sequentially.
            </li>
            <li>
              <strong>Validators and delegates</strong> — <code>increase_take</code> /{' '}
              <code>decrease_take</code> now incur transaction fees; preview with{' '}
              <code>--dry-run</code>.
            </li>
          </ol>
        </section>

        <span className={styles.paper_link}>
          <Link href='/docs'>Read the full documentation</Link>
        </span>

        <section className={styles.section}>
          <div className={styles.connect_links}>
            {connectLinks.map((link) => (
              <Link
                key={link.href}
                href={link.href}
                isExternal={link.isExternal}
                className={styles.connect_link}
              >
                <span className={styles.connect_label}>{link.label}</span>
              </Link>
            ))}
          </div>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
