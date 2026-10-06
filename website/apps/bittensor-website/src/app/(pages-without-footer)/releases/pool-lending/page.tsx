import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'Pool Reserves and Native Lending',
  description:
    'A proposed migration preserves opening price and local sensitivity, creating separate ' +
    'lending reserves for custodial alpha shorts and transferable TAO loans.',
  alternates: {canonical: '/releases/pool-lending'},
};

export default function PoolLendingRelease() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>Pool Reserves and Native Lending</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Proposed network upgrade · Not yet deployed
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>One pool migration, two uses of liquidity</h2>
          <p>
            This upgrade puts balances outside subnet pools&apos; trading range to work as lending
            inventory. Existing pools move to a translated ellipse while preserving their opening
            prices and local price sensitivity. The migration uses the baseline ellipse for every
            pool, without additional tightening of its depth.
          </p>
          <p>
            V1 defers the proposed minimum 1% ending-price movement for a 500-TAO-equivalent trade.
            It does not expose a pool-depth tuning call. Very small trades preserve their opening
            price response; finite trades differ because the curve has a different shape. The
            ellipse still has finite buy and sell endpoints, so complete swaps must fit its
            remaining range and any caller price limits.
          </p>
          <p>
            The migration transfers globally unreachable alpha and TAO balances into separate
            vaults. It adjusts the ellipse centers by the same amounts, preserving the rebuilt swap
            quotes. The assets can be extracted independently, without paired liquidity withdrawal
            or token creation. Borrowing is enabled only after the complete migration succeeds; an
            incomplete migration leaves it disabled. Governance can pause new loans without
            disabling repayments.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Fixed principal, collateral coupons</h2>
          <p>
            A short posts TAO collateral, borrows alpha and sells it through the AMM. The resulting
            TAO remains locked. Closing buys back the original alpha debt, or the owner supplies
            that alpha from the saved hotkey, and returns the remaining TAO. A long posts existing
            alpha collateral and receives freely transferable TAO. Closing repays the original TAO
            debt and returns the remaining alpha.
          </p>
          <ul className={styles.list}>
            <li>
              <strong>25% initial LTV.</strong> Historical lending prices and complete,
              fee-inclusive swap quotes bound each opening; vault inventory must fund it. Shorts
              also require the caller’s minimum net TAO proceeds.
            </li>
            <li>
              <strong>100% nominal annual interest on opening loan value.</strong> Coupons are fixed
              in collateral units, accrue per block and are collected weekly. A 250-TAO opening loan
              pays about 4.79 TAO per seven days, not 1,000 TAO annually merely because it has 1,000
              TAO collateral. There is no additional opening or closing fee.
            </li>
            <li>
              <strong>Both short and long coupons burn TAO.</strong> Short coupons move to the
              reserve account and then directly to the canonical inaccessible TAO burn address,
              without an AMM swap or mature price reference. Long coupons sell alpha for TAO only
              when the full fee-inclusive quote returns at least 98% of the mature lending EMA’s
              fair output after fees and price impact. Bounded chunking permits smaller acceptable
              sales. Each sale atomically burns exactly the TAO received; a failed sale or burn
              retains the backed pending coupon. Favorable prices remain allowed.
            </li>
            <li>
              <strong>No price-triggered liquidation.</strong> Collateral exhaustion forfeits the
              position and retains assets still held in custody. Unrecovered principal is recorded
              as a loss. The mechanism accepts credit losses.
            </li>
            <li>
              <strong>10% aggregate borrowing cap per asset.</strong> The denominator is available
              vault inventory plus outstanding principal, excluding AMM reserves, borrower
              collateral, locked proceeds and pending interest. Interest burns do not replenish
              inventory or enlarge the cap; principal repayments replenish the original borrowed
              asset.
            </li>
          </ul>
          <p>
            The canonical burn transfers TAO to its inaccessible burn address while both the
            currency and Subtensor total-issuance counters stay unchanged. Tiny amounts that cannot
            be credited remain an explicit recycling exception, recorded as{' '}
            <code>DustForfeited</code>.
          </p>
          <p>
            Debt never falls merely because interest was collected. Each coldkey can hold one
            position per subnet. The lending reference is a dedicated geometric EMA with a 24-hour
            half-life, fixed before the current block&apos;s trades and updated with clipped
            observations. This limits abrupt valuation changes without claiming that manipulation is
            impossible.
          </p>
          <p>
            V1 bounds processing to 256 funded subnet vaults, 256 open positions across the chain
            and 128 per subnet. When vault capacity is full, additional pools keep their reserves in
            the AMM until a retired subnet vault frees a slot for automatic admission. Borrowers
            must close before changing their own keys. Nominated positions follow actual validator
            hotkey stake migrations.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Subnet deregistration</h2>
          <p>
            Deregistration freezes interest. Existing beta-basket holdings convert to funded root
            cash first; endpoint-blocked holdings receive funded redemption into their original
            fund. Lending positions settle without AMM swaps. Pending TAO coupons are burned before
            remaining reserve inventory enters the funded dissolution pot. Pending alpha coupons
            stay ordinary vault stake through global settlement; only their actual funded TAO
            redemption receipts are burned. Unfunded alpha produces no TAO burn. Longs accumulate
            their escrow&apos;s actual TAO redemptions, then settle once after all payouts: debt is
            recovered, any surplus is returned and any shortfall is recorded. TAO transferred
            elsewhere is not assumed to remain in custody. Tiny refunds that cannot recreate a
            reaped account are explicitly recycled and recorded as <code>DustForfeited</code>.
            Cleanup finishes before the subnet identifier can be reused.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Three CLI commands</h2>
          <pre className={styles.code_block}>
            {`btcli lending open --netuid 64 --side short --collateral 1000 -w mywallet
btcli lending close --netuid 64 -w mywallet
btcli lending list --netuid 64`}
          </pre>
          <p>
            Use the SDK shipped with the lending runtime. Open and close take full runtime quotes
            and default to a 1% margin on caller protections. Failed quotes stop submission, and
            failed bounds roll back the complete transaction. The{' '}
            <Link href='/docs/guides/pool-lending' className={styles.inline_link}>
              pool lending guide
            </Link>{' '}
            explains long collateral, direct alpha repayment, reserve accounting and settlement in
            detail.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
