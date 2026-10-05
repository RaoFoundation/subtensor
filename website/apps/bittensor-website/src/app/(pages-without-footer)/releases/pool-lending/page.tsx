import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'Tunable Pool Depth and Reserve Lending',
  description:
    'A proposed price-preserving migration calibrates subnet pool depth and creates separate ' +
    'lending reserves for custodial alpha shorts and transferable TAO loans.',
  alternates: {canonical: '/releases/pool-lending'},
};

export default function PoolLendingRelease() {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>Tunable Pool Depth and Reserve Lending</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Proposed network upgrade · Not yet deployed
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>One pool migration, two uses of liquidity</h2>
          <p>
            This upgrade gives subnet pools a tunable price response and puts balances outside their
            trading range to work as lending inventory. Existing pools move to a translated ellipse
            while keeping their opening prices. The initial calibration targets at least a 1% fall
            in ending spot price for a sale of alpha worth 500 TAO at that opening price, excluding
            fees. Pools already sufficiently sensitive keep their baseline response.
          </p>
          <p>
            The target holds at calibration; parameters stay fixed between explicit changes. Pools
            unable to execute the full reference trade retain a safe baseline and report that
            limitation. The ellipse has finite buy and sell endpoints, so complete swaps must fit
            its remaining range and any caller price limits.
          </p>
          <p>
            The migration transfers globally unreachable alpha and TAO balances into separate
            vaults. It adjusts the ellipse centers by the same amounts, preserving the calibrated
            swap quotes. The assets can be extracted independently, without paired liquidity
            withdrawal or token creation. Borrowing is enabled only after the complete migration
            succeeds; an incomplete migration leaves it disabled. Governance can pause new loans
            without disabling repayments.
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
              fee-inclusive swap quotes bound each opening; vault inventory must fund it.
            </li>
            <li>
              <strong>100% nominal annual interest on opening loan value.</strong> Coupons are fixed
              in collateral units, accrue per block and are collected weekly. A 250-TAO opening loan
              pays about 4.79 TAO per seven days, not 1,000 TAO annually merely because it has 1,000
              TAO collateral.
            </li>
            <li>
              <strong>Coupons replenish lending reserves.</strong> Short coupons buy alpha; long
              coupons sell alpha for TAO. The outputs are retained, not burned. Unconverted coupons
              remain accounted for until a swap can execute. TAO dust too small to initialize an
              empty destination account is explicitly recycled and recorded.
            </li>
            <li>
              <strong>No price-triggered liquidation.</strong> Collateral exhaustion forfeits the
              position and retains assets still held in custody. Unrecovered principal is recorded
              as a loss. The mechanism accepts credit losses.
            </li>
            <li>
              <strong>10% aggregate borrowing cap per asset.</strong> The denominator is available
              vault inventory plus outstanding principal, excluding AMM reserves, borrower
              collateral, locked proceeds and pending conversions.
            </li>
          </ul>
          <p>
            Debt never falls merely because interest was collected. Each coldkey can hold one
            position per subnet. The lending reference is a dedicated geometric EMA with a 24-hour
            half-life, fixed before the current block&apos;s trades and updated with clipped
            observations. This limits abrupt valuation changes without claiming that manipulation is
            impossible.
          </p>
          <p>
            V1 bounds processing to 256 funded subnet vaults, 256 open positions across the chain
            and 128 per subnet. Borrowers must close before changing their own keys. Nominated
            positions follow actual validator hotkey stake migrations.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Subnet deregistration</h2>
          <p>
            Deregistration freezes interest. Existing beta-basket holdings convert to funded root
            cash first; endpoint-blocked holdings receive funded redemption into their original
            fund. Lending positions settle without AMM swaps. Remaining lending inventory enters
            the funded dissolution pot. Longs accumulate their escrow&apos;s actual TAO redemptions,
            then settle once after all payouts: debt is recovered, any surplus is returned and
            any shortfall is recorded. TAO transferred elsewhere is not assumed to
            remain in custody. Tiny refunds that cannot recreate a reaped account are explicitly
            recycled and recorded. Cleanup finishes before the subnet identifier can be reused.
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
