import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';
import layout from './page.module.css';

export const metadata: Metadata = {
  title: 'The V482 Upgrade — Quantum-Resistant and Rotating Wallets',
  description:
    'V482 introduces ML-DSA-65 wallets without account registration, optional key rotation ' +
    'for all three signing schemes, and safe multisig, proxy, and derivative accounts ' +
    'while preserving existing transaction decoding.',
  alternates: {canonical: '/releases/v482-upgrade'},
};

const DocLink = ({href, children}: {href: string; children: React.ReactNode}) => (
  <Link href={href} className={styles.inline_link}>
    {children}
  </Link>
);

export default function page() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>The V482 Upgrade</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Quantum-resistant and rotating wallets · Upcoming
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>New keys, familiar accounts</h2>
          <p>
            Spec <strong>482</strong> introduces <strong>MS wallets</strong>, using ML-DSA-65
            signatures designed to resist attacks from quantum computers. A standard MS wallet has
            one familiar SS58 address. It can receive TAO and sign its first transaction without
            account registration, a sponsor, or a registration deposit. Existing Sr25519 and Ed25519
            wallets continue to work.
          </p>
          <p>
            The same release adds optional key rotation to all three signing schemes. A rotating
            wallet keeps its address and recovery phrase while changing the key that authorizes each
            included transaction. Multisig, proxy, and derivative accounts gain a safe address mode
            so MS users can combine authorities without introducing a classical signing key for the
            resulting account.
          </p>
          <p>
            This page describes the proposed upgrade in{' '}
            <DocLink href='https://github.com/RaoFoundation/subtensor/pull/3220'>PR #3220</DocLink>.
            Runtime deployment and feature activation are separate steps; creating an MS wallet does
            not establish that the connected network has enabled it.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Choose a signature scheme and account mode</h2>
          <Code
            language='bash'
            code={`# Standard wallets: fixed signing keys, no account registration
btcli wallet create --crypto-type sr
btcli wallet create --crypto-type ed
btcli wallet create --crypto-type ms

# Rotating wallets: add --type hashed to any scheme
btcli wallet create --crypto-type sr --type hashed
btcli wallet create --crypto-type ed --type hashed
btcli wallet create --crypto-type ms --type hashed`}
          />
          <div className={layout.table_scroller}>
            <table className={styles.metrics_table}>
              <thead>
                <tr>
                  <th>Wallet</th>
                  <th>Address to share</th>
                  <th>Account registration</th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td>Standard SR / ED</td>
                  <td>SS58</td>
                  <td>None</td>
                </tr>
                <tr>
                  <td>Standard MS</td>
                  <td>SS58</td>
                  <td>None</td>
                </tr>
                <tr>
                  <td>Rotating SR / ED</td>
                  <td>
                    <code>tao_</code> receiving address
                  </td>
                  <td>Required; 0.2 TAO deposit</td>
                </tr>
                <tr>
                  <td>Rotating MS</td>
                  <td>
                    <code>tao_</code> receiving address
                  </td>
                  <td>Required; 0.2 TAO deposit</td>
                </tr>
              </tbody>
            </table>
          </div>
          <p>
            <code>--crypto-type</code> chooses the signature scheme; <code>--type hashed</code>{' '}
            chooses rotation. The default account mode is standard. These choices apply to both
            coldkey and hotkey unless overridden individually. Save the scheme and mode with each
            recovery phrase: restoring the same phrase with a different choice creates a different
            account. Existing accounts do not change type automatically.
          </p>
          <p>
            A standard MS address is a 32-byte hash of its public descriptor. Every transaction
            carries the full public key and ML-DSA signature; the runtime checks their connection to
            that address and verifies the signature. No prior account record is needed. The large
            public key stays in the proof, while the account keeps its short SS58 address.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Rotation without changing your address</h2>
          <p>
            For a rotating wallet, the chain records the next authorized key commitment and a
            counter. Each included transaction advances that counter, even if the operation itself
            fails. The wallet derives the next key from the same master seed. Recovery reads the
            finalized counter and reconstructs the current key; there is no growing history of
            private keys to back up. Rotation alone does not make SR or ED signatures
            quantum-resistant. Select MS for post-quantum signatures.
          </p>
          <p>
            A rotating wallet shows two representations of one account. Its receiving address
            contains the public descriptor needed to register it; its SS58 form is the account ID
            displayed by explorers. Descriptor-aware clients accept the receiving address for
            transactions. Bare SS58 works as an account identifier after registration, but cannot
            tell a sender how to register the recipient. Register and wait for finalization before
            publicly sharing a new rotating account&apos;s address or assigning it roles.
          </p>
          <p>
            A transfer can register a rotating recipient atomically. The CLI asks for explicit
            sponsorship consent before adding the <strong>0.2 TAO permanent reserve</strong> from
            the sender, separate from the transfer and transaction fees. <code>--yes</code> alone
            does not approve it. Registration defaults to MEV Shield; explicitly require{' '}
            <code>--mev-shield</code> to prevent fallback to public submission. Standard MS
            recipients need none of this setup.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Multisig and proxies with the right authority</h2>
          <p>
            MS wallets sign native transfers, staking, neuron registration, validator weights, and
            other authorized Subtensor operations through the same account permissions as ordinary
            wallets. They can approve multisig operations and grant or use proxies. Normal fees,
            neuron registration costs, and multisig or proxy deposits still apply.
          </p>
          <p>
            New <strong>safe composite accounts</strong> cover multisigs, derivatives, and pure
            proxies. Their full 32-byte addresses cannot also be SR or ED public keys, preventing
            direct classical signing from bypassing the intended members or parent. Safe mode
            requires no account registration or registration deposit. It produces a different
            address from legacy mode; existing groups keep their original addresses.
          </p>
          <Code
            language='bash'
            code={`# New CLI groups default to safe mode; it can also be explicit
btcli multisig add treasury --account-mode safe \\
  --threshold 2 --signatories alice,bob,carol`}
          />
          <p>
            Safe multisigs accept 2–100 distinct members and thresholds from one to the member
            count. SDK callers select <code>account_mode="safe"</code>; omitted SDK mode remains
            legacy. The SDK selects safe derivative and pure-proxy paths for fixed MS wallets and
            explicitly safe multisig origins. Preserve the mode when restoring an account.
          </p>
          <p>
            Quantum resistance depends on who can actually authorize an operation. Every usable
            quorum must require a quantum-resistant authority; a sufficient classical quorum or a
            freely authorized classical proxy weakens that protection. Legacy composite addresses do
            not gain a post-quantum guarantee merely because their members use MS. Registered
            protected composites remain a separate option for qualifying rotating authorities.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>EVM and client compatibility</h2>
          <p>
            Standard MS wallets use the existing EVM key association workflow. Ethereum transactions
            still use a separate ECDSA EVM wallet, with its own balance and nonce. Associating it
            with a native neuron does not merge the wallets or make ECDSA quantum-resistant. Native
            EVM calls remain available under their existing source and backing-account rules. An MS
            signature does not replace an Ethereum signature.
          </p>
          <p>
            Existing transaction framing and signature types are preserved. MS proofs fit inside the
            existing signer-address field, so metadata-aware clients such as Polkadot.js can decode
            mixed blocks and show the original operation. Existing SR and ED transactions retain
            their format. Creating MS or rotating transactions requires an updated signer;
            block-decoding compatibility does not add MS support to an older browser or hardware
            wallet.
          </p>
          <p>
            MS message signatures, HTTP authentication, and detached orders use dedicated,
            owner-approved capabilities. These need their own setup and verifier support; they are
            separate from account registration. The{' '}
            <DocLink href='/docs/guides/wallet-crypto-types'>wallet guide</DocLink> explains the
            supported interfaces and recovery steps.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Rollout and upgrading</h2>
          <p>
            The runtime switch <code>AdminUtils.sudo_set_hashed_accounts_enabled</code> controls
            registration and signing for every hashed wallet and every MS wallet, including
            registration-free standard MS. It defaults to disabled. Ordinary standard SR and ED
            signing is unaffected. Disabling the feature pauses its use without deleting existing
            registration, protection, or rotation state.
          </p>
          <p>
            The matching client releases are <code>bittensor 11.3.2</code> (SDK and CLI) and{' '}
            <code>bittensor-core 0.1.9</code>. Upgrade them together when published, and refresh
            chain metadata after the runtime upgrade. Rebuild offline transaction payloads prepared
            for the previous runtime. Operators must deploy a compatible node and runtime and enable
            the feature before advertising transaction support. MS proofs are larger and
            verification adds weight; obtain a fresh fee quote for the operation and submission
            route, including Shield.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/docs/guides/wallet-crypto-types'>Wallet creation and recovery guide</Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
}
