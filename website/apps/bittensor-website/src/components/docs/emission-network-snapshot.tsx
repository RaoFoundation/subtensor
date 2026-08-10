'use client';

import {ExplainerPanel, ExplainerStat} from './explainer-panel';
import {useEmissionSnapshot} from '@/hooks/use-emission-snapshot';
import {formatSnapshotAge} from '@/lib/emission-snapshot';
import {formatPct, formatTao} from '@/lib/emission-math';

export function EmissionNetworkSnapshot() {
  const {snapshot, loading} = useEmissionSnapshot();
  const gateCaption =
    snapshot.emissionGateSource === 'chain_storage'
      ? 'The gate settings and midpoint come from current chain storage.'
      : 'This snapshot was captured before these emission rules became active, so it uses the default gate settings.';

  return (
    <ExplainerPanel
      title='Finney mainnet snapshot'
      caption={`${gateCaption} Fetched ${formatSnapshotAge(snapshot.fetchedAt)}.`}
    >
      <div className='grid grid-cols-2 gap-x-8 gap-y-5 lg:grid-cols-4'>
        <ExplainerStat
          label='Block emission'
          value={loading ? '…' : `${formatTao(snapshot.blockEmissionTao)} / block`}
          hint='Calculated from total issuance (halving-adjusted)'
        />
        <ExplainerStat
          label='Total issuance'
          value={loading ? '…' : formatTao(snapshot.totalIssuanceTao, 2)}
          hint='SubtensorModule.TotalIssuance (halving input)'
        />
        <ExplainerStat
          label='Σ EMA prices'
          value={loading ? '…' : snapshot.emaPriceSum.toFixed(3)}
          hint={
            snapshot.rootDividendGateOpen
              ? 'Root dividends active (eligible Σ EMA > 1.0)'
              : 'Root gate closed (eligible Σ EMA ≤ 1.0)'
          }
          accent={!loading && !snapshot.rootDividendGateOpen}
        />
        <ExplainerStat
          label='Root pool TAO'
          value={loading ? '…' : formatTao(snapshot.rootTao, 0)}
          hint='Subnet 0 reserves'
        />
      </div>

      <p className='mt-6 border-t border-line pt-4 text-[0.8125rem] leading-relaxed text-mute'>
        This preview starts with each eligible subnet&apos;s{' '}
        <strong className='font-medium text-fg'>SubnetMovingPrice</strong> (EMA of spot alpha price,
        capped at 1.0), turns it into a share of the total, then applies the emission gate.{' '}
        <code>MinerBurned</code> is not part of this cross-subnet allocation. Top modeled recipient:{' '}
        <strong className='font-medium text-fg'>
          SN{snapshot.topSubnets[0]?.netuid} {snapshot.topSubnets[0]?.name}
        </strong>{' '}
        at {loading ? '…' : formatPct(snapshot.topSubnets[0]?.taoShare ?? 0)} (
        {loading ? '…' : formatTao(snapshot.topSubnets[0]?.taoPerBlock ?? 0, 4)}/block).
      </p>
    </ExplainerPanel>
  );
}
