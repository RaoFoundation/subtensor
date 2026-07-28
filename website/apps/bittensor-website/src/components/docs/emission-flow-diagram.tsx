'use client';

import { useMemo } from 'react';
import { ExplainerPanel, ExplainerStat } from './explainer-panel';
import { useEmissionSnapshot } from '@/hooks/use-emission-snapshot';
import { alphaEmissionPerBlock, alphaIssuance } from '@/lib/emission-snapshot';
import { alphaOutSplit, formatPct, formatTao, rootProportion } from '@/lib/emission-math';
import { ACCENT, INK } from './chart-theme';

function FlowBar({label, pct, detail, accent = false}: {label: string; pct: number; detail?: string; accent?: boolean}) {
  return (
    <div>
      <div className="mb-1.5 flex items-baseline justify-between gap-2">
        <span className="bt-label text-mute">{label}</span>
        <span className="font-mono text-xs" style={{color: accent ? ACCENT : INK}}>
          {formatPct(pct)}
        </span>
      </div>
      <div className="h-[3px] bg-[rgba(41,41,41,0.08)]">
        <div
          className="h-full transition-all duration-300"
          style={{width: `${pct * 100}%`, backgroundColor: accent ? ACCENT : INK}}
        />
      </div>
      {detail && <p className="mt-1 text-[0.6875rem] text-mute">{detail}</p>}
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
      <div className="grid gap-x-10 gap-y-6 md:grid-cols-2">
        <div className="border-t border-line pt-3">
          <p className="bt-label mb-3 text-mute">01 · Per block (coinbase)</p>
          <ul className="space-y-2 text-[0.8125rem]">
            <li>{formatTao(snapshot.blockEmissionTao)}/block minted → price-EMA shares</li>
            <li>SN{featured.netuid} receives {formatTao(featured.taoPerBlock, 4)}</li>
            <li>Accrues {alphaOut.toFixed(4)} α_out for next epoch</li>
          </ul>
        </div>

        <div className="border-t border-line pt-3">
          <p className="bt-label mb-3 text-mute">02 · At epoch (Yuma)</p>
          <ul className="space-y-2 text-[0.8125rem]">
            <li>Distribute miner half via incentive ranks</li>
            <li>Pay validator dividends + delegate take</li>
            <li>
              Root slice {rootGateOpen ? '→ claimable' : '→ recycled'} ({formatPct(rootProp)} of validator half)
            </li>
          </ul>
        </div>
      </div>

      <div className="mt-8 space-y-5 border-t border-line pt-4">
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
            accent
          />
        )}
      </div>

      <div className="mt-6 grid grid-cols-2 gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-3">
        <ExplainerStat label="α_out / block" value={`${alphaOut.toFixed(4)} α`} />
        <ExplainerStat label="≈ per tempo (360 blocks)" value={`${(alphaOut * 360).toFixed(2)} α`} hint="Default tempo" />
        <ExplainerStat label="Miner pool / tempo" value={`${(split.miner * 360).toFixed(1)} α`} />
      </div>
    </ExplainerPanel>
  );
}
