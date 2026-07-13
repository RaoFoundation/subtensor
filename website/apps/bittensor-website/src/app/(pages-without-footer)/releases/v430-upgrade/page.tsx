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
            Written by Const, core dev @ Rao Foundation
          </p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Bittensor has just completed the largest upgrade in its history. I do not say that
            lightly, and I do not mean it in the way every project means it when they ship a
            version bump. I mean that more of the network&apos;s foundation moved in this single
            release than in any release before it. The chain now runs
            <strong> spec version 430</strong>. Subnet ownership is no longer a historical
            accident — it is earned, on-chain, by whoever commits the most for the longest.
            Emissions between subnets now follow one thing only: <i>what the market believes
            each subnet is worth</i>. And the software that carries all of it — the chain, the
            SDK, the command line, the documentation, this very website — now lives in one
            repository, ships as one release, and installs as one package.
          </p>
          <p>
            This page is my account of what changed and why it matters, and what — if anything —
            you need to do about it. Every section links into the{' '}
            <DocLink href='/docs'>new documentation</DocLink>, where the full detail lives.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Ownership by conviction</p>
          <p>
            When you lock alpha on a subnet, the locked amount accrues
            <strong> conviction </strong>— a time-weighted measure of commitment, credited to
            the hotkey of your choosing. Until this upgrade, conviction was a number the chain
            recorded and did nothing with. As of v430 it has teeth:
          </p>
          <p>
            <strong>
              If a subnet is more than a year old, and the total conviction across its lockers
              exceeds ten percent of its outstanding alpha, the hotkey with the highest
              conviction becomes the subnet&apos;s owner
            </strong>
            {' '}— emissions cut and all.
          </p>
          <p>
            Think about what this means. Owning a subnet is no longer a fact about the past —
            it is a position that must be <i>defended</i>. The person most committed to a
            subnet&apos;s future can now take stewardship of it, openly, through rules everyone
            can read. Perpetual locks mature toward their full mass in roughly six weeks;
            decaying locks — the default — rise and then unwind over roughly four months. This
            is the network rewarding exactly the thing it has always wanted from its
            participants: <strong>long-horizon alignment</strong>. Skin in the game, verifiable
            on-chain.
          </p>
          <p>
            The mechanics, the lock modes, and a worked example live in the{' '}
            <DocLink href='/docs/guides/conviction'>conviction guide</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Emissions, simplified</p>
          <p>
            Every block, the chain divides its TAO emission between subnets. That split is now
            driven purely by each subnet&apos;s <strong>moving average price</strong>, weighted
            by a miner-burn penalty. The root-proportion term — which structurally squeezed
            mature subnets as their alpha issuance grew, punishing them for the crime of
            getting older — has been removed from the cross-subnet split. Root proportion still
            does its job <i>within </i>each subnet, capping liquidity injection and reserving
            the root stakers&apos; share of dividends. But between subnets, the market decides.
            Full stop.
          </p>
          <p>
            A subnet&apos;s emission is now a direct function of what people believe it is
            worth. That is how it always should have been, and the formula is documented in{' '}
            <DocLink href='/docs/concepts/emissions'>emissions</DocLink> for anyone who wants
            to check my math.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>One package: bittensor v11</p>
          <p>
            <strong>pip install bittensor</strong>. That is the entire instruction now. The SDK
            and the <strong>btcli </strong>command line ship in a single package, powered by a
            new Rust core for keys, keyfiles, encoding, and timelock encryption. The separate
            bittensor-cli and bittensor-wallet packages are superseded entirely. Your wallet
            keyfiles are unchanged and fully compatible — we do not break wallets.
          </p>
          <p>
            The Rust core was not a rewrite for its own sake, and we did not guess at the
            benefit. We measured it, against the live network, before and after:
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
            I will be honest about what did not change: submission and inclusion are still bound
            by the chain itself — twelve-second blocks do not care how fast your codec is. But
            everything you actually <i>feel</i> — startup, metagraph pulls, batch construction —
            got dramatically faster. v11 is also a major revision of the API: the old Subtensor
            class gives way to a client-and-intent model with planning, policy gates, and typed
            results. The <DocLink href='/docs/migration'>migration guide</DocLink> maps every
            v9/v10 call to its v11 form, and the{' '}
            <DocLink href='/docs/quickstart'>quickstart</DocLink> covers a fresh start.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Sign with anything</p>
          <p>
            Every transaction can now be signed on a <strong>Ledger hardware wallet</strong>.
            And this is <i>clear signing</i>, not blind signing: through merkleized metadata,
            the device decodes the actual transaction on its own screen, and the chain verifies
            the same metadata digest that was signed. Nothing can show you &quot;transfer 1
            TAO&quot; while signing something else — the device refuses anything it cannot
            prove. See <DocLink href='/docs/guides/ledger'>signing with a Ledger</DocLink>.
          </p>
          <p>
            Prefer a <strong>browser extension</strong>? Talisman, Polkadot.js, and SubWallet
            all work. The CLI relays the transaction to the extension through a local bridge;
            only signatures flow back, so no keyfile, no password, and no mnemonic ever touches
            the machine running btcli. See{' '}
            <DocLink href='/docs/guides/extension-signing'>
              signing with a browser extension
            </DocLink>
            .
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Documentation, rebuilt</p>
          <p>
            The documentation you are one click away from is new, and it holds itself to a
            standard I have wanted for years: the reference for <strong>all 74
            transactions </strong>and <strong>all 82 queries </strong>is generated from the SDK
            itself, so it <i>cannot</i> drift from the software. It is written for humans and
            for machines — agent catalogs and plain-text endpoints mean an AI coding assistant
            can drive Bittensor natively. That last part matters more than most people realize
            yet. Start at <DocLink href='/docs'>the documentation home</DocLink>, or point your
            agent at <DocLink href='/docs/agents'>the agents page</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>One repository, releases on rails</p>
          <p>
            The chain, the SDK, the CLI, the documentation, and this website now live together
            in{' '}
            <span className={styles.paper_link}>
              <Link href='https://github.com/RaoFoundation/subtensor' isExternal={true}>
                github.com/RaoFoundation/subtensor
              </Link>
            </span>
            . One repository, one release train, one version. And this release rode those rails
            all the way to mainnet: every runtime change was tested against a <i>live clone of
            mainnet state</i> before it merged; a single deterministic build was promoted
            through devnet and testnet with automated checks at each stage; and the upgrade the
            keyholders signed was cryptographically verified against the exact bytes the
            pipeline built. A new public devnet, documented in{' '}
            <DocLink href='/docs/concepts/network'>the network overview</DocLink>, joins finney
            and testnet as a first-class environment.
          </p>
          <p>
            The runtime itself was hardened in kind: proxy permissions are now deny-by-default,
            a crowdloan reentrancy flaw is closed, and the randomness pipeline securing
            commit-reveal can no longer be wedged.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Sign and verify — trust, but check</p>
          <p>
            This upgrade also changed how upgrades themselves are approved — and v430 was the
            first to ship the new way. The release pipeline publishes a
            <strong> proposal release </strong>— tagged at the exact commit deployed, carrying
            the runtime, its deterministic build digest, and the exact call data that was
            signed. The keyholders approved it with a single command, and that same tooling
            verified everything before anything was signed: that the call data was precisely a
            runtime upgrade and nothing else, that the embedded runtime matched the published
            digest, and that the on-chain proposal carried the same hash.
          </p>
          <p>
            Here is the part I care about most: <strong>verification is not reserved for
            keyholders</strong>. Runtime builds are deterministic — identical source produces a
            byte-identical runtime — so <i>anyone</i> can check what was deployed against the
            code it claims to be:
          </p>
          <p className={styles.code_block}>
            btcli upgrade check --url
            https://github.com/RaoFoundation/subtensor/releases/tag/v430
          </p>
          <p>
            Or go further: build the runtime from source with the pinned toolchain and pass
            your own bytes with <strong>--wasm </strong>— a passing check proves the chain runs
            exactly the code you compiled yourself. A URL anyone can fetch, call data anyone
            can re-derive, an on-chain hash anyone can compare. Do not trust me;{' '}
            <i>check</i>. This is the template for verifiable governance, and the full flow is
            documented in{' '}
            <DocLink href='/docs/internals/release-process'>the release process</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What you need to do</p>
          <p style={{textAlign: 'left', width: '100%'}}>
            Most of you need to do very little. In order of urgency:
          </p>
          <ol className={styles.list}>
            <li>
              <strong>Python users</strong> — uninstall bittensor-cli and bittensor-wallet,
              then install the new bittensor package. Follow the{' '}
              <DocLink href='/docs/migration'>migration guide</DocLink>; your keyfiles are
              unchanged.
            </li>
            <li>
              <strong>Proxy users</strong> — permissions are now deny-by-default. Review every
              proxy configuration you have.
            </li>
            <li>
              <strong>Node operators</strong> — if you are not yet running the spec 430 binary,
              you have already noticed. Upgrade.
            </li>
            <li>
              <strong>Indexers and SDK authors</strong> — chain metadata now carries typed
              currency units; verify your decoders against the new{' '}
              <DocLink href='/docs/query'>query reference</DocLink>.
            </li>
            <li>
              <strong>Subnet owners and stakers</strong> — understand{' '}
              <DocLink href='/docs/guides/conviction'>conviction</DocLink>. Ownership of
              subnets older than one year is now contestable. That includes yours.
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
