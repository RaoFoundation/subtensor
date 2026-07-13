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
            Rao Foundation
          </p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Bittensor has completed a major network upgrade. The chain now runs
            <strong> spec version 430</strong>, and the release changes both the network&apos;s
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
            conviction was recorded on-chain but had no effect. As of spec 430, it governs
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
          <p>
            Subnet ownership is therefore no longer fixed at registration; it is contestable
            through open, on-chain rules. Two lock modes are available. Perpetual locks mature
            toward their full conviction mass in approximately six weeks. Decaying locks — the
            default — accrue and then unwind over approximately four months. The mechanism is
            designed to reward long-horizon commitment to a subnet&apos;s success. The lock
            modes, the conviction formula, and a worked example are documented in the{' '}
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
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Hardware and extension signing</p>
          <p>
            Every transaction can now be signed on a <strong>Ledger hardware wallet</strong>{' '}
            using clear signing. Through merkleized metadata, the device decodes the
            transaction on its own screen, and the chain verifies the same metadata digest that
            was signed; the device rejects any transaction it cannot decode and verify. Setup
            and usage are documented in{' '}
            <DocLink href='/docs/guides/ledger'>signing with a Ledger</DocLink>.
          </p>
          <p>
            Transactions can also be signed with a <strong>browser extension</strong> —
            Talisman, Polkadot.js, or SubWallet. The CLI relays the transaction to the
            extension through a local bridge, and only the signature is returned; no keyfile,
            password, or mnemonic is present on the machine running btcli. See{' '}
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
            software. The site also publishes agent catalogs and plain-text endpoints, allowing
            AI coding assistants to consume the documentation directly. Start at the{' '}
            <DocLink href='/docs'>documentation home</DocLink>, or point an agent at the{' '}
            <DocLink href='/docs/agents'>agents page</DocLink>.
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
          <p className={styles.subtitle}>Sign and verify</p>
          <p>
            This release also changes how runtime upgrades are approved, and v430 was the first
            upgrade shipped this way. The release pipeline publishes a
            <strong> proposal release</strong>: a GitHub release tagged at the exact commit
            deployed, carrying the runtime, its deterministic build digest, and the exact call
            data to be signed. Keyholders approve with a single command, and the tooling
            verifies — before anything is signed — that the call data is precisely a runtime
            upgrade and nothing else, that the embedded runtime matches the published digest,
            and that the on-chain proposal carries the same hash.
          </p>
          <p>
            Verification is not limited to keyholders. Runtime builds are deterministic —
            identical source produces a byte-identical runtime — so any participant can verify
            a deployed upgrade against its published source:
          </p>
          <p className={styles.code_block}>
            btcli upgrade check --url
            https://github.com/RaoFoundation/subtensor/releases/tag/v430
          </p>
          <p>
            Alternatively, build the runtime from source with the pinned toolchain and pass the
            resulting bytes with <strong>--wasm</strong>; a passing check proves the chain is
            running exactly the code that was compiled. The complete flow is documented in{' '}
            <DocLink href='/docs/internals/release-process'>the release process</DocLink>.
          </p>
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
              unchanged.
            </li>
            <li>
              <strong>Proxy users</strong> — proxy permissions are now deny-by-default. Review
              every existing proxy configuration.
            </li>
            <li>
              <strong>Node operators</strong> — nodes not yet running the spec 430 binary must
              upgrade to continue syncing.
            </li>
            <li>
              <strong>Indexers and SDK authors</strong> — chain metadata now carries typed
              currency units; verify decoders against the new{' '}
              <DocLink href='/docs/query'>query reference</DocLink>.
            </li>
            <li>
              <strong>Subnet owners and stakers</strong> — review the{' '}
              <DocLink href='/docs/guides/conviction'>conviction guide</DocLink>. Ownership of
              subnets older than one year is now contestable.
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
