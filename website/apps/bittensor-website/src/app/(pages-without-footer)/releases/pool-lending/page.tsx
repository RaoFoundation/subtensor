import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'Pool Reserves and Native Lending',
  description:
    'A proposed migration preserves opening price and local sensitivity, creating separate ' +
    'lending reserves for freely usable alpha borrowing and transferable TAO loans.',
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
            A short posts TAO collateral and receives real alpha on its selected hotkey. That alpha
            can be sold, transferred, used or pledged elsewhere; opening makes no AMM sale and locks
            no sale proceeds. Closing returns the total alpha debt from the saved hotkey and refunds
            remaining TAO. The owner can instead request a buyback using only remaining collateral.
            A long posts existing alpha collateral and receives freely transferable TAO. Closing
            repays the total TAO debt and returns the remaining alpha.
          </p>
          <ul className={styles.list}>
            <li>
              <strong>25% initial LTV.</strong> Historical lending prices and complete,
              fee-inclusive simulated swap quotes bound each opening; vault inventory must fund it.
              Shorts protect the delivered alpha and its opening TAO valuation separately.
            </li>
            <li>
              <strong>Alpha loans also require funded-claim coverage.</strong> The combined alpha
              debt&apos;s conservative immediate deregistration value, including payout rounding,
              cannot exceed 25% of remaining TAO collateral. The bound overestimates the possible
              payout pot and counts only guaranteed alpha claims after the proposed withdrawal.
              Legacy subnets protect only protocol alpha because their ordinary payout excludes pool
              alpha; newer subnets also protect active and latent pool alpha and remaining unloaned
              vault alpha. Missing bounds or a zero protected claim count with a positive pot refuse
              borrowing. Every other alpha loan must remain fully covered by its own collateral
              after accrued interest. Quotes and opening enforce the same checks.
            </li>
            <li>
              <strong>TAO loans also require funded redemption backing.</strong> A new loan cannot
              exceed 25% of its alpha collateral&apos;s conservative funded deregistration value
              after the withdrawal. This assumes all active AMM TAO could be sold out and counts
              only unloaned TAO vault inventory. Every existing TAO loan must remain fully covered
              by its own collateral after accrued interest; one position&apos;s surplus cannot cover
              another&apos;s deficit. The runtime quote and opening enforce the same limits.
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
              collateral and pending interest. Interest burns do not replenish inventory or enlarge
              the cap; principal repayments replenish the original borrowed asset.
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
            position per subnet. Repeating the open command with the same side and hotkey adds
            collateral and debt to that position. Accrued interest is collected first and the
            combined position must pass current opening limits. The new annual coupon adds to
            earlier coupons without repricing them or restarting the weekly schedule. Quotes
            describe the additional borrowing; closing repays the total principal.
          </p>
          <p>
            The lending reference is a dedicated geometric EMA with a 24-hour half-life, fixed
            before the current block&apos;s trades and updated with clipped observations. This
            limits abrupt valuation changes without claiming that manipulation is impossible.
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
            Deregistration freezes interest and both the 24-hour lending EMA and existing 2-hour
            fast EMA. Existing beta-basket holdings convert to funded root cash first;
            endpoint-blocked holdings receive funded redemption into their original fund. Pending
            TAO coupons are burned and unloaned reserve assets return before the ordinary funded
            payout pot and eligible alpha claims are fixed. Alpha holders, including holders of
            freely borrowed alpha, then receive ordinary pro-rata redemption.
          </p>
          <p>
            A short&apos;s fixed alpha debt is valued at the higher of the two frozen EMAs and its
            actual funded redemption value, with conservative rounding. That debt is recovered from
            remaining TAO collateral; any surplus returns to the owner and any shortfall is
            recorded. Longs accumulate their collateral&apos;s actual TAO redemption receipts and
            repay fixed TAO debt from those receipts. Both kinds of principal recovery remain
            outside the already-fixed payout pot and go to global protocol recovery. There are no
            terminal lending swaps or assumed repayments of freely transferred assets.
          </p>
          <p>
            Terminal recovery from a short cannot exceed its remaining TAO collateral. The 25%
            opening market LTV and conservative funded-claim checks constrain admission, while
            subsequent pool or claim changes and interest deductions can still weaken coverage.
            Conservative terminal valuation cannot create missing funds; the design accepts
            unrecovered principal as a loss.
          </p>
          <p>
            Pending alpha coupons stay ordinary vault stake through global settlement; only their
            actual funded TAO receipts are burned. Unfunded alpha produces no TAO burn. Tiny refunds
            that cannot recreate a reaped account are explicitly recycled and recorded as{' '}
            <code>DustForfeited</code>. Cleanup finishes before the subnet identifier can be reused.
          </p>
          <p>
            Alpha collateral receives ordinary funded pro-rata redemption, with no priority over
            other holders and no promised EMA payout. The additional borrowing check constrains
            withdrawals rather than changing settlement. Interest deductions and changes in alpha
            claims can still weaken future coverage, so the design continues to accept credit
            losses.
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
            for the owner and hotkey, and default to a 1% margin on caller protections. The same
            open command can increase a matching position. Failed quotes stop submission, and failed
            bounds roll back the complete transaction. The{' '}
            <Link href='/docs/guides/pool-lending' className={styles.inline_link}>
              pool lending guide
            </Link>{' '}
            explains long collateral, wallet alpha repayment, optional collateral-only buyback,
            reserve accounting and settlement in detail.
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
}
