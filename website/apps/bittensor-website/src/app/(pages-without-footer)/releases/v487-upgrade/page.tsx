import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';
import layout from './page.module.css';

export const metadata: Metadata = {
  title: 'Release 487 — Gamma Service Credits + Quantum-Resistant Wallets',
  description:
    'V487 introduces Gamma service credits minted by burning subnet alpha, on-chain service ' +
    'discovery, ML-DSA and Falcon wallets, optional key rotation, and updates to proxies, ' +
    'basket dividends, and emission controls.',
  alternates: {canonical: '/releases/v487-upgrade'},
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
          <h1 className={styles.paper_title}>Release 487</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Gamma service credits + quantum-resistant wallets · Upcoming
          </p>
        </section>

        <section className={styles.section} id='gamma'>
          <h2 className={styles.subtitle}>Gamma: prepaid credits for subnet services</h2>
          <p>
            Gamma gives subnets a way to charge for services in dollar-valued credits. Customers
            fund a deposit by burning subnet alpha, use a hotkey as an API key, and authorize a
            service&apos;s spender to charge that deposit for usage. Service demand becomes an alpha
            burn, while customers can budget their usage in dollars.
          </p>
          <p>
            One Gamma is minted for each dollar of alpha burned under the runtime&apos;s pricing
            rules. Credits belong to a specific subnet. They cannot be transferred to another
            coldkey or redeemed for TAO, alpha, or dollars. Each service decides whether to accept
            Gamma and how much to charge.
          </p>
          <p>
            This page covers the proposed{' '}
            <DocLink href='https://github.com/RaoFoundation/subtensor/pull/3294'>
              Release 487
            </DocLink>
            , including{' '}
            <DocLink href='https://github.com/RaoFoundation/subtensor/pull/3300'>Gamma</DocLink> and{' '}
            <DocLink href='https://github.com/RaoFoundation/subtensor/pull/3220'>
              quantum-resistant wallets
            </DocLink>
            . Runtime deployment and feature activation are separate steps.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Find a service, fund an API key</h2>
          <p>
            Subnet owners and validators with a permit can publish a service URL and a billing
            spender on chain. The URL points to a document describing the service&apos;s endpoints
            and payment methods. Customers select its published spender when opening a deposit.
          </p>
          <ol className={styles.list}>
            <li>
              Create a fresh hotkey for service requests. The API key needs no neuron registration
              or funds; keep it separate from staking keys and the coldkey that owns the deposit.
            </li>
            <li>
              Mint with TAO to buy and burn subnet alpha without opening a stake position, or mint
              with stake to burn alpha you already hold.
            </li>
            <li>
              Sign requests with the API key. The designated spender bills the deposit on chain; the
              service handles request authentication and delivery.
            </li>
          </ol>
          <Code
            language='bash'
            code={`# Replace 51 and SERVICE_HOTKEY with the subnet and service you choose
btcli wallet new-hotkey -H API_KEY
btcli query services --netuid 51

# Buy and burn alpha, naming the service and a 20 Gamma daily spend cap
btcli gamma mint --netuid 51 --hotkey API_KEY --amount-tao 10 \\
  --service-hotkey SERVICE_HOTKEY --daily-spend-cap-gamma 20

# Or burn alpha already staked to VALIDATOR
btcli gamma mint-with-stake --netuid 51 --hotkey API_KEY \\
  --origin-hotkey VALIDATOR --amount-alpha 100 \\
  --service-hotkey SERVICE_HOTKEY --daily-spend-cap-gamma 20`}
          />
          <p>
            Mint previews quote the expected credits and apply minimum-output protection, with a 5%
            tolerance by default. The result reports exact decimal amounts for the alpha burned, its
            TAO value, and the Gamma minted, together with the deposit&apos;s spender address and
            optional daily spend cap.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Control the deposit, delegate the billing</h2>
          <p>
            The coldkey owns the credits and can hold up to 64 deposits per subnet. It can refill
            them, move credits between its own deposits on that subnet, change a spender, and close
            empty deposits. Spending a deposit down to zero leaves it open for refills; moving its
            entire balance closes the source.
          </p>
          <p>
            An optional daily spend cap replenishes continuously over 7,200 blocks, up to the cap.
            It does not reset at midnight. A zero cap pauses spending. Set the spender and cap when
            minting to establish the terms from the start; refilling an existing deposit checks any
            explicitly supplied terms against its current configuration.
          </p>
          <Code
            language='bash'
            code={`btcli gamma show --netuid 51 --hotkey API_KEY
btcli gamma set-cap --netuid 51 --hotkey API_KEY --daily-spend-cap-gamma 0
btcli gamma set-spender --netuid 51 --hotkey API_KEY --service-hotkey SERVICE_HOTKEY`}
          />
          <p>
            Providers can bill up to 512 entries in one transaction. Each entry spends its full
            amount or is skipped, with events reporting the result and an optional request reference
            for reconciliation. A <code>GammaSpend</code> proxy lets a provider delegate billing
            without giving the billing key broader account permissions.
          </p>
          <p>
            The spender is trusted to bill for the service: the chain enforces the deposit&apos;s
            balance, spender, and cap, but does not verify service delivery. Spending destroys Gamma
            and pays no TAO or alpha to the spender. The alpha burn happens when the customer funds
            the credits.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>How Gamma is priced and enabled</h2>
          <p>
            Minting values alpha at the lower of its pre-trade spot price and a subnet Gamma price
            that falls immediately with spot but rises gradually. A root-appointed oracle converts
            that TAO value to dollars. Its accepted TAO/USD price has bounded upward movement, and
            minting requires a feed no older than 300 blocks, about one hour.
          </p>
          <Code
            language='text'
            code={`TAO value = alpha burned × min(Gamma price, pre-trade spot price)
Gamma minted = TAO value × accepted TAO/USD price`}
          />
          <p>
            Gamma starts disabled. Root must enable it network-wide, and the subnet must enable its
            own <code>gamma_enabled</code> setting. Turning either switch off stops new mints;
            existing deposits can still be spent, moved, and closed. Dissolving or pruning the
            subnet removes its credits without redemption.
          </p>
          <p>
            The Python SDK and CLI include mint quotes, deposit balances and spending allowances,
            service discovery, and detailed mint and billing results. See the{' '}
            <DocLink href='/docs/guides/gamma'>Gamma guide</DocLink> for the full deposit lifecycle
            and coldkey-swap behavior, and the{' '}
            <DocLink href='/docs/guides/services'>services guide</DocLink> for publishing a service.
          </p>
        </section>

        <section className={styles.section} id='wallets'>
          <h2 className={styles.subtitle}>Quantum-resistant wallets and optional rotation</h2>
          <p>
            V487 adds <strong>ML-DSA-65, Falcon-512, and Falcon-1024</strong> wallets alongside
            Sr25519 and Ed25519. All five schemes support standard fixed keys or optional key
            rotation. Standard post-quantum wallets use familiar SS58 addresses and can receive TAO
            and sign without account registration, a sponsor, or a registration deposit.
          </p>
          <Code
            language='bash'
            code={`# Standard post-quantum wallets
btcli wallet create --crypto-type ms
btcli wallet create --crypto-type falcon512
btcli wallet create --crypto-type falcon1024

# Add --type hashed for rotation; sr and ed support it too
btcli wallet create --crypto-type ms --type hashed
btcli wallet create --crypto-type falcon512 --type hashed
btcli wallet create --crypto-type falcon1024 --type hashed`}
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
                  <td>Standard Sr25519 / Ed25519</td>
                  <td>SS58</td>
                  <td>None</td>
                </tr>
                <tr>
                  <td>Standard ML-DSA / Falcon</td>
                  <td>SS58</td>
                  <td>None</td>
                </tr>
                <tr>
                  <td>Rotating, any of the five schemes</td>
                  <td>
                    <code>tao_</code> receiving address
                  </td>
                  <td>Required; 0.2 TAO permanent reserve by default</td>
                </tr>
              </tbody>
            </table>
          </div>
          <p>
            <code>--crypto-type</code> chooses the signature scheme and <code>--type hashed</code>{' '}
            chooses rotation. Standard is the default account mode. Save the scheme and mode with
            the recovery phrase: using a different choice restores a different account. Existing
            accounts do not change type automatically.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Rotate keys, keep the account</h2>
          <p>
            A rotating wallet keeps its account identity, balances, permissions, and receiving
            addresses while its signing key changes. Rotation occurs when a valid transaction
            reaches dispatch, even if the operation fails. Submission or acceptance into the
            transaction pool alone does not rotate authority. Recovery reads the finalized counter
            and derives the current key from the same recovery phrase.
          </p>
          <p>
            The <code>tao_</code> receiving address contains the public descriptor needed for
            registration. A transfer can register a rotating recipient atomically, with explicit
            sponsorship consent for the sender&apos;s{' '}
            <strong>permanent registration reserve</strong> (default 0.2 TAO), separate from the
            transfer and transaction fees. Sudo can change this shared deposit for future
            registrations; existing reserves remain unchanged. Registration defaults to MEV Shield.
            Register and wait for finalization before publicly sharing a new rotating account or
            assigning it roles.
          </p>
          <p>
            Rotation does not make Sr25519 or Ed25519 signatures quantum-resistant. Choose ML-DSA or
            Falcon for post-quantum signatures. Standard ML-DSA and Falcon recipients need no
            registration or sponsorship.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Wallet permissions and compatibility</h2>
          <p>
            The new wallets integrate with transfers, staking, neuron and validator operations,
            coldkey and hotkey operations, proxies, multisig, MEV Shield, orders, contracts, and
            native EVM calls. Normal fees and operation-specific deposits still apply. Safe
            multisig, derivative, and pure-proxy accounts use addresses that prevent direct
            classical signing from bypassing the intended members or parent, without requiring
            account registration. Their addresses differ from legacy accounts.
          </p>
          <p>
            Existing Sr25519 and Ed25519 wallets keep their addresses and signed-v4 transaction
            format. Metadata-aware clients can decode the new transactions, but signing requires
            compatible wallet software. Hardware wallets and browser extensions need explicit
            support for the selected scheme. ML-DSA and Falcon wallets approve limit orders through
            signed <code>authorize_order</code> transactions; arbitrary message signing and HTTP
            authentication are not supported for these schemes.
          </p>
          <p>
            Ethereum transactions continue to use ECDSA wallets. Associating one with a native
            neuron does not merge the wallets or make ECDSA quantum-resistant. Registered rotating
            accounts can use protected EVM aliases for native calls. Standard EVM gas behavior is
            preserved, including the 21,000 gas limit for plain transfers; alias lookups are metered
            through Frontier&apos;s external resource accounting.
          </p>
          <p>
            See the <DocLink href='/docs/guides/wallet-crypto-types'>wallet guide</DocLink> for
            recovery, safe account modes, registration sponsorship, and supported signing
            interfaces.
          </p>
        </section>

        <section className={styles.section} id='other-changes'>
          <h2 className={styles.subtitle}>Proxies, basket dividends, and emission controls</h2>
          <p>
            A new <strong>Lock proxy</strong> permits only <code>lock_stake</code>,{' '}
            <code>move_lock</code>, and <code>set_perpetual_lock</code>. It can manage conviction
            locks without permission to add, remove, or transfer stake. Locking still affects what
            can be unstaked, and moving a lock between different coldkey owners forfeits conviction.
            Existing <strong>Staking proxies</strong> gain <code>claim_root</code> and{' '}
            <code>claim_root_with_hotkey</code>; <code>RootClaim</code> remains the narrower grant.
          </p>
          <p>
            Restricted proxies can no longer create deferred proxy announcements. Announcements from
            before the upgrade are invalidated, while removal and deposit recovery remain available.{' '}
            <code>NonCritical</code> proxies no longer permit EVM calls.
          </p>
          <p>
            Basket dividend shares use consistent valuation for existing holdings and incoming
            dividends, preserving escrow yield for existing shareholders. Pending dividends settle
            before root stake, basket ownership, or composition changes, with credits retained for
            retry when settlement cannot complete. Settlement weights follow each hotkey&apos;s
            pending credits and holdings and refund to actual work.
          </p>
          <p>
            Subnets can allocate nonzero emission shares to <strong>burn and recycle</strong>, up to
            a combined 33%. Existing owner, validator, miner, and root-share constraints remain in
            force, and partial updates preserve omitted allocations. The{' '}
            <DocLink href='/docs/concepts/emission-weights'>emission weights guide</DocLink> and{' '}
            <DocLink href='/docs/guides/proxies'>proxy guide</DocLink> cover the updated controls.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Rollout and developer updates</h2>
          <p>
            Gamma and wallet activation are separate controls. Gamma needs its global and subnet
            switches plus a fresh oracle price. The root-controlled{' '}
            <code>AdminUtils.sudo_set_hashed_accounts_enabled</code> switch defaults to disabled and
            gates all rotating wallets and all standard ML-DSA and Falcon wallets. Ordinary standard
            Sr25519 and Ed25519 signing is unaffected.
          </p>
          <p>
            Use matching SDK, CLI, and native core releases, refresh metadata after the runtime
            upgrade, and rebuild offline transaction payloads prepared for the previous runtime.
            Obtain fresh fee quotes for the chosen signing and submission route. HTTP-auth receivers
            using Sr25519 or Ed25519 should configure trusted sender schemes with{' '}
            <code>http_auth.configure_verifier</code>; Shield carrier fee estimation now requires
            the actual call.
          </p>
          <p>
            Maintainers can run benchmarks for selected pallets and exact functions, retain
            completed measurements across later failures, and apply only results matching the
            current PR head. The{' '}
            <DocLink href='/docs/internals/benchmarks-and-weights'>benchmark guide</DocLink>{' '}
            explains the selection and patch workflow. Gamma, service, and order weights currently
            include documented local calibration; dedicated reference measurements remain
            outstanding. The new burn/recycle increment does not resolve the separate full-Null
            capacity finding.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/docs/guides/gamma'>Get started with Gamma</Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
}
