'use client';

import { useMemo, useState } from 'react';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import { useEmissionSnapshot } from '@/hooks/use-emission-snapshot';
import { alphaIssuance } from '@/lib/emission-snapshot';
import {
  blockEmissionTao,
  formatPct,
  formatTao,
  rootProportion,
} from '@/lib/emission-math';

export function RootProportionExplainer() {
  const {snapshot} = useEmissionSnapshot();
  const featured = snapshot.featuredSubnet;

  const [rootTao, setRootTao] = useState(snapshot.rootTao);
  const [alphaIssuanceVal, setAlphaIssuanceVal] = useState(alphaIssuance(featured));
  const [price, setPrice] = useState(featured.spotPrice);

  const alphaEmission = useMemo(() => blockEmissionTao(alphaIssuanceVal), [alphaIssuanceVal]);
  const taoEmission = featured.taoPerBlock;

  const rootProp = rootProportion(rootTao, alphaIssuanceVal, snapshot.taoWeight);
  const injectionCap = rootProp * alphaEmission;
  const naiveAlphaIn = taoEmission > 0 && price > 0 ? taoEmission / price : 0;
  const alphaIn = Math.min(naiveAlphaIn, injectionCap);
  const taoIn = alphaIn * price;
  const excessTao = Math.max(0, taoEmission - taoIn);

  const chart = useMemo(() => {
    const points = 40;
    return Array.from({length: points + 1}, (_, i) => {
      const issuance = (12_000_000 * i) / points;
      return rootProportion(rootTao, issuance, snapshot.taoWeight);
    });
  }, [rootTao, snapshot.taoWeight]);

  return (
    <ExplainerPanel
      title={`Root proportion — SN${featured.netuid} ${featured.name}`}
      caption="Finney snapshot for Targon: mature subnet with high alpha issuance; injection cap binds and routes excess TAO to pool buybacks."
    >
      <div className="grid grid-cols-2 gap-x-8 gap-y-4 sm:grid-cols-3">
        <ExplainerStat label="root_proportion" value={formatPct(rootProp)} />
        <ExplainerStat label="Injection cap (α/block)" value={`${injectionCap.toFixed(4)} α`} />
        <ExplainerStat
          label="Excess TAO → buyback"
          value={formatTao(excessTao, 4)}
          hint={`${formatTao(taoEmission, 4)} tao_in − cap`}
          accent={excessTao > 0}
        />
      </div>

      <div className="mt-6 grid grid-cols-2 gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-3">
        <ExplainerStat
          label="Pool reserves"
          value={`${formatTao(featured.taoIn, 0)} · ${featured.alphaIn.toLocaleString()} α`}
        />
        <ExplainerStat label="Alpha issuance" value={`${(alphaIssuanceVal / 1_000_000).toFixed(2)}M α`} />
        <ExplainerStat label="Spot price" value={`${featured.spotPrice.toFixed(4)} τ/α`} />
      </div>

      <div className="mt-6 border-t border-line pt-4">
        <div className="flex h-16 items-end gap-px border-b border-line">
          {chart.map((v, i) => (
            <div
              key={i}
              className="flex-1 bg-fg/70"
              style={{height: `${Math.max(v * 100, 2)}%`, opacity: 0.15 + v * 0.85}}
              title={formatPct(v)}
            />
          ))}
        </div>
        <p className="mt-2 font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">
          root_proportion vs alpha issuance (root TAO held fixed at finney level)
        </p>
      </div>

      <div className="mt-6 grid gap-x-8 gap-y-5 border-t border-line pt-4 sm:grid-cols-2">
        <ExplainerSlider
          label="Root subnet TAO (τ)"
          value={rootTao}
          min={1_000_000}
          max={8_000_000}
          step={100_000}
          display={formatTao(rootTao, 0)}
          onChange={setRootTao}
        />
        <ExplainerSlider
          label="Alpha issuance (α)"
          value={alphaIssuanceVal}
          min={500_000}
          max={8_000_000}
          step={100_000}
          display={`${(alphaIssuanceVal / 1_000_000).toFixed(2)}M α`}
          onChange={setAlphaIssuanceVal}
        />
        <ExplainerSlider
          label="Spot alpha price (τ/α)"
          value={price}
          min={0.02}
          max={0.15}
          step={0.001}
          display={`${price.toFixed(4)} τ/α`}
          onChange={setPrice}
        />
        <ExplainerStat
          label="SN4 tao_in this block"
          value={formatTao(taoEmission, 4)}
          hint="From price-EMA share"
        />
      </div>

      <div className="mt-6 grid gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-2">
        <ExplainerStat
          label="Price-neutral target"
          value={`alpha_in = tao_in / price → ${naiveAlphaIn.toFixed(4)} α`}
        />
        <ExplainerStat
          label="After cap"
          value={`→ ${alphaIn.toFixed(4)} α injected (${formatTao(taoIn, 4)})`}
        />
      </div>
    </ExplainerPanel>
  );
}
