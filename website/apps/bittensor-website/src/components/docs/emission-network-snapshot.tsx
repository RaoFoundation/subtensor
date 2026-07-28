'use client';

import { ExplainerPanel, ExplainerStat } from './explainer-panel';
import { useEmissionSnapshot } from '@/hooks/use-emission-snapshot';
import { formatSnapshotAge } from '@/lib/emission-snapshot';
import { formatPct, formatTao } from '@/lib/emission-math';

export function EmissionNetworkSnapshot() {
  const {snapshot, loading} = useEmissionSnapshot();

  return (
    <ExplainerPanel
      title="Finney mainnet snapshot"
      caption={`Price-EMA emission (live ${snapshot.emissionMode}). Σ EMA prices from TaoMarketCap — fetched ${formatSnapshotAge(snapshot.fetchedAt)}.`}
    >
      <div className="grid grid-cols-2 gap-x-8 gap-y-5 lg:grid-cols-4">
        <ExplainerStat
          label="Block emission"
          value={loading ? '…' : `${formatTao(snapshot.blockEmissionTao)} / block`}
          hint="SubtensorModule.BlockEmission on finney"
        />
        <ExplainerStat
          label="Total issuance"
          value={loading ? '…' : formatTao(snapshot.totalIssuanceTao, 2)}
          hint="SubtensorModule.TotalIssuance (halving input)"
        />
        <ExplainerStat
          label="Σ EMA prices"
          value={loading ? '…' : snapshot.emaPriceSum.toFixed(3)}
          hint={
            snapshot.rootDividendGateOpen
              ? 'Root dividends active (Σ EMA > 1.0, TMC)'
              : 'Root gate closed (Σ EMA ≤ 1.0, TMC)'
          }
          accent={!loading && !snapshot.rootDividendGateOpen}
        />
        <ExplainerStat
          label="Root pool TAO"
          value={loading ? '…' : formatTao(snapshot.rootTao, 0)}
          hint="Subnet 0 reserves"
        />
      </div>

      <p className="mt-6 border-t border-line pt-4 text-[0.8125rem] leading-relaxed text-mute">
        TAO splits across subnets by <strong className="font-medium text-fg">SubnetMovingPrice</strong>{' '}
        (EMA of spot alpha price, capped at 1.0), minus last-tempo{' '}
        <strong className="font-medium text-fg">MinerBurned</strong> penalties. Top recipient right
        now: <strong className="font-medium text-fg">SN{snapshot.topSubnets[0]?.netuid} {snapshot.topSubnets[0]?.name}</strong>{' '}
        at {loading ? '…' : formatPct(snapshot.topSubnets[0]?.taoShare ?? 0)} (
        {loading ? '…' : formatTao(snapshot.topSubnets[0]?.taoPerBlock ?? 0, 4)}/block).
      </p>
    </ExplainerPanel>
  );
}
