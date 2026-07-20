import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from './page.module.css';

export const metadata: Metadata = {
  title: 'The V434 Upgrade',
  description:
    'One-call stake transfer to a new coldkey and hotkey, a dedicated stake-transfer minimum, ' +
    'air-gapped signing with Polkadot Vault, and fully benchmarked extrinsic weights.',
  alternates: {canonical: '/releases/v434-upgrade'},
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
          <p className={styles.paper_title}>The V434 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Spec <strong>434</strong> is the next mainnet runtime after{' '}
            <DocLink href='/releases/v432-upgrade'>v432</DocLink>. It adds one new extrinsic —
            a stake transfer that changes the owning coldkey <i>and</i> the delegated hotkey in
            a single atomic call — and gives stake transfers their own on-chain minimum,
            decoupled from the staking minimum. Off chain, the release ships air-gapped
            transaction signing with Polkadot Vault, and every extrinsic weight in the runtime
            is now measured by benchmark rather than assigned by hand.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Stake transfers, generalized</p>
          <p>
            <DocLink href='/docs/tx/transfer-stake'>
              <code>transfer_stake_and_hotkey</code>
            </DocLink>{' '}
            hands a stake position to another coldkey and lands it on a different hotkey —
            optionally on a different subnet — in one atomic extrinsic. Previously this took
            two calls (<code>transfer_stake</code> then <code>move_stake</code>), with the
            position exposed on the wrong validator between them and the second call left to
            the recipient. The intent surface is unchanged: pass <code>--dest-hotkey</code> to{' '}
            <code>btcli tx transfer-stake</code> (or <code>dest_hotkey_ss58</code> on the{' '}
            <code>TransferStake</code> intent) and the SDK dispatches the new call. The
            existing <code>Transfer</code> and <code>SmallTransfer</code> proxy types cover it
            under the same rules as <code>transfer_stake</code>.
          </p>
          <p>
            Stake transfers also get their own minimum:{' '}
            <DocLink href='/code/runtime/src/lib.rs#L819'>
              <code>InitialMinTransfer</code>
            </DocLink>{' '}
            is 0.0001 TAO, where transfers previously had to clear the 0.002 TAO staking
            minimum. Moving a small position to another coldkey no longer fails with{' '}
            <code>AmountTooLow</code> at thresholds that were designed for stake creation.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Air-gapped signing with Polkadot Vault</p>
          <p>
            Every transaction can now be signed from a{' '}
            <DocLink href='/docs/guides/vault'>Polkadot Vault</DocLink> phone — a device that
            never goes online. Pass <code>--signer vault</code> and the CLI shows the
            transaction as a QR code; the phone decodes the call on its own screen, you approve
            it there, and the signature travels back through your webcam. No keyfile, password,
            or mnemonic ever exists on the machine running btcli:
          </p>
          <pre className={styles.code_block}>
            {`btcli addresses add my-vault --vault    # scan the address off the phone, once
btcli tx transfer --dest 5F...dest --amount-tao 1 --signer vault --signer-address my-vault`}
          </pre>
          <p>
            Like <DocLink href='/docs/guides/ledger'>Ledger signing</DocLink>, the flow is
            clear-signing through merkleized metadata: each transaction QR carries a proof of
            the runtime types it touches, the phone verifies the proof before displaying the
            call, and the chain verifies the same digest inside the signature. Setup is a
            single chain-specs scan — there are no metadata updates to sync, ever, including
            across runtime upgrades. MEV-shielded stake trades work too, as a timed two-scan
            flow. The same signer is available in Python as <code>bt.VaultSigner</code>.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Weights, measured</p>
          <p>
            Every dispatchable in the runtime now carries a{' '}
            <DocLink href='/code/pallets/subtensor/src/weights.rs'>benchmarked weight</DocLink>{' '}
            — including <code>proof_size</code>, which was previously ignored — replacing the
            hand-assigned constants used before. The benchmark suite was rebuilt around
            worst-case state (heavier registration, block-step, and commitment paths), and a CI
            lint now fails any PR that adds an extrinsic without a plugged-in benchmark. Fees
            follow weights, so per-call fees shift slightly in both directions; nothing changes
            by an order of magnitude.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Tooling and docs</p>
          <ul className={styles.list}>
            <li>
              The chain&apos;s Rust source is browsable at{' '}
              <DocLink href='/code'>bittensor.com/code</DocLink> exactly as built into the
              running runtime, and transaction docs link each call to its declaration.
            </li>
            <li>
              Extension signing is smoother: a remembered account is reused without
              re-prompting, the extension popup is the confirmation (no terminal bounce), and
              the bridge page can toggle individual extensions on or off. See{' '}
              <DocLink href='/docs/guides/extension-signing'>extension signing</DocLink>.
            </li>
            <li>URLs in CLI output are clickable hyperlinks in supporting terminals.</li>
          </ul>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <p>
            Operators should treat this like any other runtime upgrade: wait for the on-chain{' '}
            <code>spec_version</code> to move to 434, then upgrade nodes and clients. SDK users
            should pull the matching bittensor release once the train publishes it — older
            clients keep working, they simply don&apos;t know the new call. Indexers should add{' '}
            <code>SubtensorModule.transfer_stake_and_hotkey</code> (call index 143) and its{' '}
            <code>StakeAndHotkeyTransferred</code> event.
          </p>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v434 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
