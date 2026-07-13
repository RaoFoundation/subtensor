import FadeInWrapper from '@/app/components/FadeInWrapper';
import {MenuSchema} from '@/app/components/Header/MenuSchema';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'The V430 Upgrade',
  description:
    'One repository, one package, new documentation, and new network economics: ' +
    'conviction-based subnet ownership, price-driven emissions, and the bittensor v11 SDK.',
  alternates: {canonical: '/releases/v430-upgrade'},
  openGraph: {images: '/images/og_thumbs/v430-upgrade.png'},
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

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The V430 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            One network, one repository, one package · July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            The pending runtime upgrade to <strong>spec version 430</strong>{' '}is the largest
            release in Bittensor&apos;s history — not because of any single feature, but because
            of how much of the network&apos;s foundation moves at once. The chain gains new
            economics: subnet ownership becomes contestable through long-term commitment, and
            emissions between subnets are now driven purely by market price. The software gains a
            new shape: the SDK, the command line, the documentation, and the chain itself now
            live in one repository, ship as one release, and install as one package.
          </p>
          <p>
            This page summarizes what changes, why it matters, and what — if anything — you need
            to do. Every section links into the{' '}
            <DocLink href='/docs'>new documentation</DocLink>, where the full detail lives.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Ownership by conviction</p>
          <p>
            When you lock alpha on a subnet, the locked amount accrues <strong>conviction</strong>{' '}
            — a time-weighted commitment score credited to the hotkey of your choice. Until now,
            conviction was recorded but had no consequence. With this upgrade it gains one, and it
            is significant:
          </p>
          <p>
            <strong>
              If a subnet is more than a year old, and the total conviction across its lockers
              exceeds ten percent of its outstanding alpha, the hotkey with the highest conviction
              becomes the subnet&apos;s owner
            </strong>
            {' '}— including the owner&apos;s share of emissions.
          </p>
          <p>
            Subnet ownership is now contestable, on-chain, by whoever commits the most for the
            longest. Perpetual locks mature toward their full mass in roughly six weeks; decaying
            locks, the default, rise and then unwind over roughly four months. This is a new
            game-theoretic layer over every subnet in the network, and it rewards exactly the
            behavior the network wants: long-horizon alignment.
          </p>
          <p>
            The mechanics, the lock modes, and a worked example are in the{' '}
            <DocLink href='/docs/guides/conviction'>conviction guide</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Emissions, simplified</p>
          <p>
            Block emission is now divided between subnets purely in proportion to each
            subnet&apos;s <strong>moving average price</strong>, weighted by a miner-burn penalty.
            The root-proportion term — which structurally squeezed mature subnets as their alpha
            issuance grew — has been removed from the cross-subnet split. Root proportion still
            plays its role <i>within</i>{' '}each subnet, capping liquidity injection and reserving
            the root stakers&apos; share of dividends, but it no longer decides how emission is
            divided between subnets.
          </p>
          <p>
            The consequence: a subnet&apos;s emission share is now a direct function of what the
            market believes it is worth. The full formula and its parameters are documented in{' '}
            <DocLink href='/docs/concepts/emissions'>emissions</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>One package: bittensor v11</p>
          <p>
            <strong>pip install bittensor</strong> now delivers everything: the SDK and the{' '}
            <strong>btcli</strong> command line in a single package, powered by a new Rust core
            for keys, keyfiles, encoding, and timelock encryption. It replaces the separate
            bittensor-cli and bittensor-wallet packages entirely. Wallet keyfiles are unchanged
            and fully compatible.
          </p>
          <p>
            The Rust core is not a rewrite for its own sake. It was measured against the live
            network, before and after:
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
            Submission and inclusion remain bound by the chain itself — the gains are in startup,
            decoding, and construction, which is what validators pulling metagraphs and anyone
            scripting the CLI actually feel. v11 is a major API revision: the old Subtensor class
            gives way to a client-and-intent model with planning, policy gates, and typed
            results. The <DocLink href='/docs/migration'>migration guide</DocLink> maps every
            v9/v10 call to its v11 form, and the{' '}
            <DocLink href='/docs/quickstart'>quickstart</DocLink> covers a fresh start.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Sign with anything</p>
          <p>
            Every transaction can now be signed on a <strong>Ledger hardware wallet</strong>.
            This is clear signing, not blind signing: through merkleized metadata, the device
            decodes the actual transaction on its own screen, and the chain verifies the same
            metadata digest that was signed. Nothing can display &quot;transfer 1 TAO&quot; while
            signing something else — the device refuses anything it cannot prove. See{' '}
            <DocLink href='/docs/guides/ledger'>signing with a Ledger</DocLink>.
          </p>
          <p>
            Alternatively, sign with a <strong>browser extension</strong> — Talisman, Polkadot.js,
            or SubWallet. The CLI relays the transaction to the extension through a local bridge;
            only signatures flow back, so no keyfile, password, or mnemonic ever touches the
            machine running btcli. See{' '}
            <DocLink href='/docs/guides/extension-signing'>
              signing with a browser extension
            </DocLink>
            .
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Documentation, rebuilt</p>
          <p>
            The documentation you are one click away from is new. Roughly two hundred pages,
            including a generated reference for <strong>all 74 transactions</strong> and{' '}
            <strong>all 82 queries</strong> — produced from the SDK itself, so the reference can
            never drift from the software. It is written for humans and for machines: agent
            catalogs and plain-text endpoints mean an AI coding assistant can drive Bittensor
            natively. Start at <DocLink href='/docs'>the documentation home</DocLink> or point
            your agent at <DocLink href='/docs/agents'>the agents page</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>One repository, releases on rails</p>
          <p>
            The chain, the SDK, the CLI, the documentation, and this website now live together in{' '}
            <span className={styles.paper_link}>
              <Link href='https://github.com/RaoFoundation/subtensor' isExternal={true}>
                github.com/RaoFoundation/subtensor
              </Link>
            </span>
            . One repository, one release train, one version. That train carries real safety
            rails: every runtime change is tested against a <i>live clone of mainnet state</i>{' '}
            before it merges; a single deterministic build is promoted through devnet and testnet
            with automated checks at each stage before it is proposed to mainnet; and the upgrade
            the keyholders sign is cryptographically verified against the exact bytes the
            pipeline built. A new public devnet, documented in{' '}
            <DocLink href='/docs/concepts/network'>the network overview</DocLink>, joins finney
            and testnet as a first-class environment.
          </p>
          <p>
            The runtime itself is hardened in kind: proxy permissions are now deny-by-default, a
            crowdloan reentrancy flaw is closed, and the randomness pipeline that secures
            commit-reveal can no longer be wedged.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Sign and verify — trust, but check</p>
          <p>
            This upgrade also changes how upgrades themselves are approved. The release pipeline
            publishes a <strong>proposal pre-release</strong> — a GitHub release tagged at the
            exact commit being deployed, carrying the runtime, its deterministic build digest,
            and the exact call data awaiting signatures. Keyholders approve it with a single
            command, and the same tooling verifies everything before anything is signed: that
            the call data is precisely a runtime upgrade and nothing else, that the embedded
            runtime matches the published digest, and that the on-chain proposal carries the
            same hash.
          </p>
          <p>
            The part that matters for everyone else: <strong>verification is not reserved for
            keyholders</strong>. Runtime builds are deterministic — identical source produces a
            byte-identical runtime — so any holder can check a pending upgrade against the code
            it claims to be:
          </p>
          <p className={styles.code_block}>
            btcli upgrade pending
            <br />
            btcli upgrade check --url https://github.com/RaoFoundation/subtensor/releases/tag/v430
          </p>
          <p>
            Or go further: build the runtime from source with the pinned toolchain and pass your
            own bytes with <strong>--wasm</strong> — a passing check then proves the on-chain
            proposal executes exactly the code you compiled yourself. A URL anyone can fetch,
            call data anyone can re-derive, and an on-chain hash anyone can compare: this is the
            template for verifiable governance. The full flow is documented in{' '}
            <DocLink href='/docs/internals/release-process'>the release process</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What you need to do</p>
          <p style={{textAlign: 'left', width: '100%'}}>
            Most participants need to do very little. In order of urgency:
          </p>
          <ol className={styles.list}>
            <li>
              <strong>Python users</strong> — uninstall bittensor-cli and bittensor-wallet, then
              install the new bittensor package. Follow the{' '}
              <DocLink href='/docs/migration'>migration guide</DocLink>; keyfiles are unchanged.
            </li>
            <li>
              <strong>Proxy users</strong> — permissions are now deny-by-default. Review every
              proxy configuration before the upgrade executes.
            </li>
            <li>
              <strong>Node operators</strong> — upgrade to the spec 430 binary before the
              on-chain upgrade is authorized.
            </li>
            <li>
              <strong>Indexers and SDK authors</strong> — chain metadata now carries typed
              currency units; verify your decoders against the new{' '}
              <DocLink href='/docs/query'>query reference</DocLink>.
            </li>
            <li>
              <strong>Subnet owners and stakers</strong> — understand{' '}
              <DocLink href='/docs/guides/conviction'>conviction</DocLink>. Ownership of subnets
              older than one year is now contestable.
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
