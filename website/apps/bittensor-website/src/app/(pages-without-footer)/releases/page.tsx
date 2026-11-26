import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'Releases',
  description:
    'Bittensor network releases: every runtime upgrade with what changed, why it matters, ' +
    'and what to do about it.',
  alternates: {canonical: '/releases'},
};

type Release = {
  tag: string;
  date: string;
  title: string;
  summary: string;
  href: string;
};

// Newest first. Add new releases to the top.
const releases: Release[] = [
  {
    tag: 'v498',
    date: 'Proposed',
    title: 'Runtime Version Bump',
    summary:
      'Raises the runtime spec to 498 without changing calls or parameters relative to V497. ' +
      'Refresh metadata and rebuild signing payloads after activation.',
    href: '/releases/v498-upgrade',
  },
  {
    tag: 'v497',
    date: 'Proposed',
    title: 'Runtime and Python SDK',
    summary:
      'Raises the runtime spec to 497 without changing calls or parameters relative to V496. ' +
      'Python SDK 11.3.7 ships the V496 sudo signing safeguards.',
    href: '/releases/v497-upgrade',
  },
  {
    tag: 'v496',
    date: 'Proposed',
    title: 'Root Parameter Proposals',
    summary:
      'Tracks 51 root-controlled settings across three networks and produces read-only proposals ' +
      'with a verified call hash and pinned multisig approvals. Merging does not change live settings.',
    href: '/releases/v496-upgrade',
  },
  {
    tag: 'v495',
    date: 'Proposed',
    title: '16 Null Mechanisms, a Shared Null Epoch, Gamma Emission Deposits and Root Claim Weight',
    summary:
      'Null subnets allow 2,500 UIDs for any mechanism count and can run up to 16 mechanisms ' +
      'when root raises the global limit of two. New Null weight rows share a budget of 2,500 ' +
      'entries across the mechanisms. The Null epoch reads subnet-wide state once, with ' +
      'identical results, and reports its own block weight. A deposit that gamma emission ' +
      'opens for a subnet owner has no daily spend cap. A single-validator root claim reserves ' +
      'the block weight of its own fund and admits funds with rows on 256 subnets. The guide ' +
      'gives one picture of V494 and V495, with CLI, Python SDK and raw-call examples.',
    href: '/releases/v495-upgrade',
  },
  {
    tag: 'v494',
    date: 'Proposed',
    title: 'Commit Burns, Weight Delegation, Basket Controls and Etao',
    summary:
      'Coldkeys publish commitments for each mechanism and channel, with an optional burn that ' +
      'the subnet owner configures. A validator hotkey authorizes one delegate to send weights ' +
      'for one mechanism. Root can set the basket trade band, price reference and refill ' +
      'period; the defaults do not change. The owner limit for subnet emission weights is a ' +
      'storage value of 360 blocks instead of a constant of 50,400 blocks.',
    href: '/releases/v494-upgrade',
  },
  {
    tag: 'v493',
    date: 'Upcoming',
    title: 'Hotkey Gamma Spends, Yuma Commit Limits and Release Announcements',
    summary:
      'Gamma deposits are spent fee-free by their own hotkey at services listed on the subnet, ' +
      'replacing the separate spender; gamma integrations must update. Also adds ' +
      'sudo-configurable Yuma timelocked commit limits, Discord release announcements, ' +
      'standalone stable Python publishing, and required release notes.',
    href: '/releases/v493-upgrade',
  },
  {
    tag: 'v487',
    date: 'Upcoming',
    title: 'Gamma Service Credits + Quantum-Resistant Wallets',
    summary:
      'Gamma adds dollar-valued service credits minted by burning subnet alpha, with ' +
      'published service endpoints, designated spenders, and daily spending caps. Wallets ' +
      'support Sr25519, Ed25519, ML-DSA-65, Falcon-512, and Falcon-1024, with optional key ' +
      'rotation. The release also adds burn and recycle emission slots, proxy permissions, ' +
      'basket dividend settlement fixes, and SDK and CLI support.',
    href: '/releases/v487-upgrade',
  },
  {
    tag: 'v481',
    date: 'October 2026',
    title: 'Etao',
    summary:
      'Subnets choose their emission split. A per-subnet five-slot vector sets how much of each ' +
      'block goes to the owner (0–33%), validators (≤50%) and miners (≥33%), changeable by the ' +
      'owner once a week. Root TAO stakers keep their baseline payout whatever the owner picks: ' +
      'every block the chain pays root its baseline share off the top of the validator slot, ' +
      'recomputed from live state, so neither owners nor drifting stake can move it. Payouts ' +
      'are identical at upgrade; sudo_set_subnet_owner_cut is deprecated.',
    href: '/releases/etao',
  },
  {
    tag: 'next',
    date: 'Upcoming',
    title: 'Null Consensus',
    summary:
      'An opt-in epoch mechanism with highest-stake-validator miner incentives, ' +
      'stake-proportional dividends, exact integer payouts, and a shared 16,000-UID capacity. ' +
      'Includes raw-u16 SDK and CLI submission and larger bounded timelock commits.',
    href: '/releases/null-consensus',
  },
  {
    tag: 'next',
    date: 'September 2026',
    title: 'Basket Trading',
    summary:
      'V461 adds swap_basket: root validators trade one basket holding for another through a ' +
      'dedicated BasketTrading proxy, boxed in by a 2% per-leg price band, a token-bucket ' +
      'turnover budget of 10% of NAV per day, a 10% per-pool liquidity cap, the 1/16 ' +
      'concentration cap, and governance freeze switches. Trading becomes the only way a ' +
      'basket changes shape: set_root_weights is removed and dividends accumulate in place. ' +
      'Launches gated off.',
    href: '/releases/v461-upgrade',
  },
  {
    tag: 'v450',
    date: 'August 2026',
    title: 'Curated Beta',
    summary:
      'V450 enables set_root_weights. Validators curate their dividend baskets under a ' +
      '1/16 concentration cap, and the chain now computes the basket index, display ' +
      'prices, and staker yield itself — one canonical scoreboard for every consumer. ' +
      'btcli root list, allocate, claim, and weights are the working surface.',
    href: '/releases/v450-upgrade',
  },
  {
    tag: 'v448',
    date: 'August 2026',
    title: 'Root Claims, Safer Staking, and Linked Orders',
    summary:
      'V448 makes root claims predictable, protects cross-subnet stake moves, adds bulk ' +
      'multi-hotkey exits, exposes live staking indexes, and introduces composable linked orders.',
    href: '/releases/v448-upgrade',
  },
  {
    tag: 'v447',
    date: 'August 2026',
    title: 'Conviction Normalization',
    summary:
      'The subnet ownership gate now measures one hotkey alone against an 18% conviction ' +
      'threshold — matching the owner cut — restoring the TAO cost of a takeover to above ' +
      'pre-v446 levels.',
    href: '/releases/conviction-normalization',
  },
  {
    tag: 'v446',
    date: 'August 2026',
    title: 'Accounting, Liquid Alpha, and Timelock Recovery',
    summary:
      'This release repairs historical alpha accounting, bases the conviction ownership gate on ' +
      'eligible alpha, adds selectable Liquid Alpha consensus modes, makes failed timelock ' +
      'reveals auditable, and corrects GRANDPA warp-sync set handling.',
    href: '/releases/v446-upgrade',
  },
  {
    tag: 'v445',
    date: 'August 2026',
    title: 'EVM, btcli, and Reliability',
    summary:
      'This release completes the typed EVM surface, makes multisigs first-class btcli wallets, ' +
      'recycles transaction fees, adds human-readable ' +
      'Ledger orders, and lands a broad reliability pass.',
    href: '/releases/v445-upgrade',
  },
  {
    tag: 'v441',
    date: 'July 2026',
    title: 'Root Reborn',
    summary:
      'Nearly half of all TAO sits on root. This release turns its dividend stream into ' +
      'validator-curated baskets — live network numbers, how the fund works, btcli ' +
      'commands, breaking changes, and the migration of legacy claimable state.',
    href: '/releases/v441-upgrade',
  },
  {
    tag: 'v440',
    date: 'July 2026',
    title: 'The Emission Gate',
    summary:
      'Subnet emission now has to beat a market-set demand bar: price-proportional above it, ' +
      'a smooth collapse below it. Idle slots stop earning, and the cost of building on ' +
      'Bittensor falls toward the registration transaction.',
    href: '/releases/v440-upgrade',
  },
  {
    tag: 'v439',
    date: 'July 2026',
    title: 'Conviction for Contracts',
    summary:
      'EVM access to stake locks and miner conviction, rolled lock views, bounded conviction ' +
      'queries, locked-alpha transfer policy, and subnet owner-cut auto-lock controls.',
    href: '/releases/v439-upgrade',
  },
  {
    tag: 'v438',
    date: 'July 2026',
    title: 'Interfaces & Reliability',
    summary:
      'Bounded EVM staking views, Ledger-friendly limit-order signatures, exact mechanism ' +
      'emission splits, predictable epoch counters, repaired testnet warp sync, and a more ' +
      'recoverable release train.',
    href: '/releases/v438-upgrade',
  },
  {
    tag: 'v437',
    date: 'July 2026',
    title: 'Collateral & Key Lineage',
    summary:
      'Miner registration collateral, on-chain hotkey and coldkey swap lineage, bonded ' +
      'key-swap hardening, one-call stake transfer to a new coldkey and hotkey, air-gapped ' +
      'Polkadot Vault signing, and fully benchmarked extrinsic weights.',
    href: '/releases/v436-upgrade',
  },
  {
    tag: 'v431',
    date: 'July 2026',
    title: 'The Monorepo Release',
    summary:
      'Conviction-based subnet ownership, price-driven emissions, the bittensor v11 SDK ' +
      'with a Rust core, Ledger and browser-extension signing, and a verifiable upgrade ' +
      'pipeline — the chain, SDK, CLI, and docs developed and released together.',
    href: '/releases/v431-upgrade',
  },
];

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>Releases</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Network upgrades, in order
          </p>
        </section>
        <section className={styles.section} style={{width: '100%'}}>
          <div className={styles.release_list}>
            {releases.map((release) => (
              <Link key={release.href} href={release.href} className={styles.release_item}>
                <span className={styles.release_meta}>
                  <span className={styles.release_tag}>{release.tag}</span>
                  <span className={styles.release_date}>{release.date}</span>
                </span>
                <span className={styles.release_title}>{release.title}</span>
                <p className={styles.release_summary}>{release.summary}</p>
              </Link>
            ))}
          </div>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
