'use client';

import { useMemo } from 'react';
import { ExplainerPanel, ExplainerStat } from './explainer-panel';
import { useEmissionSnapshot } from '@/hooks/use-emission-snapshot';
import { alphaEmissionPerBlock, alphaIssuance } from '@/lib/emission-snapshot';
import { alphaOutSplit, formatPct, formatTao, rootProportion } from '@/lib/emission-math';

function FlowBar({label, pct, detail}: {label: string; pct: number; detail?: string}) {
  return (
    <div>
      <div className="mb-1 flex items-baseline justify-between gap-2">
        <span className="bt-label text-mute">{label}</span>
        <span className="font-mono text-xs">{formatPct(pct)}</span>
      </div>
      <div className="h-2 bg-line">
        <div className="h-full bg-fg transition-all duration-300" style={{width: `${pct * 100}%`}} />
      </div>
      {detail && <p className="mt-1 text-[0.75rem] text-mute">{detail}</p>}
    </div>
  );
}

export function EmissionFlowDiagram() {
  const {snapshot} = useEmissionSnapshot();
  const featured = snapshot.featuredSubnet;
  const alphaOut = alphaEmissionPerBlock(featured);
  const rootProp = rootProportion(snapshot.rootTao, alphaIssuance(featured), snapshot.taoWeight);
  const split = alphaOutSplit(alphaOut, rootProp);
  const total = alphaOut || 1;
  const rootGateOpen = snapshot.rootDividendGateOpen;

  const segments = useMemo(() => {
    const owner = split.owner / total;
    const miner = split.miner / total;
    const validators = split.validators / total;
    const root = rootGateOpen ? split.root / total : 0;
    const recycled = rootGateOpen ? 0 : split.root / total;
    const validatorNet = validators + (rootGateOpen ? 0 : split.root / total);

    return {owner, miner, validators: validatorNet, root, recycled};
  }, [split, total, rootGateOpen]);

  return (
    <ExplainerPanel
      title={`Per-tempo alpha split — SN${featured.netuid} ${featured.name}`}
      caption={`Finney snapshot via TMC: ${alphaOut.toFixed(4)} α_out/block. Root gate ${rootGateOpen ? 'open' : 'closed'} (Σ EMA = ${snapshot.emaPriceSum.toFixed(2)}).`}
    >
      <div className="grid gap-6 md:grid-cols-[1fr_auto_1fr] md:items-center">
        <div className="border border-line bg-bg p-4">
          <p className="bt-label mb-3 text-mute">Per block (coinbase)</p>
          <ul className="space-y-2 text-[0.8125rem]">
            <li>{formatTao(snapshot.blockEmissionTao)}/block minted → price-EMA shares</li>
            <li>SN{featured.netuid} receives {formatTao(featured.taoPerBlock, 4)} τ</li>
            <li>Accrues {alphaOut.toFixed(4)} α_out for next epoch</li>
          </ul>
        </div>

        <div className="hidden text-center text-2xl text-mute md:block">→</div>

        <div className="border border-line bg-bg p-4">
          <p className="bt-label mb-3 text-mute">At epoch (Yuma)</p>
          <ul className="space-y-2 text-[0.8125rem]">
            <li>Distribute miner half via incentive ranks</li>
            <li>Pay validator dividends + delegate take</li>
            <li>
              Root slice {rootGateOpen ? '→ claimable' : '→ recycled'} ({formatPct(rootProp)} of validator half)
            </li>
          </ul>
        </div>
      </div>

      <div className="mt-6 space-y-3">
        <FlowBar label="Subnet owner" pct={segments.owner} detail="SubnetOwnerCut ≈ 18%" />
        <FlowBar label="Miners" pct={segments.miner} detail="50% of remainder → Yuma incentive" />
        <FlowBar label="Validators + stakers" pct={segments.validators} detail="Validator half minus root slice" />
        {rootGateOpen ? (
          <FlowBar label="Root TAO stakers" pct={segments.root} detail="root_proportion × validator half" />
        ) : (
          <FlowBar
            label="Recycled (gate closed)"
            pct={segments.recycled}
            detail={`Σ subnet EMA prices = ${snapshot.emaPriceSum.toFixed(2)} ≤ 1.0`}
          />
        )}
      </div>

      <div className="mt-4 grid gap-3 sm:grid-cols-3">
        <ExplainerStat label="α_out / block" value={`${alphaOut.toFixed(4)} α`} />
        <ExplainerStat label="≈ per tempo (360 blocks)" value={`${(alphaOut * 360).toFixed(2)} α`} hint="Default tempo" />
        <ExplainerStat label="Miner pool / tempo" value={`${(split.miner * 360).toFixed(1)} α`} />
      </div>
    </ExplainerPanel>
  );
}
