'use client';

import { useMemo, useState } from 'react';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import {
  DEFAULT_KAPPA,
  DEFAULT_TAO_WEIGHT,
  formatPct,
  yumaIncentives,
} from '@/lib/emission-math';

const VALIDATORS = ['V1', 'V2', 'V3'] as const;
const MINERS = ['M1', 'M2', 'M3'] as const;

const DEFAULT_WEIGHTS = [
  [0.6, 0.3, 0.1],
  [0.2, 0.5, 0.3],
  [0.1, 0.2, 0.7],
];

const DEFAULT_STAKES = [40, 35, 25];

function cellColor(value: number, max: number): string {
  const t = max > 0 ? value / max : 0;
  const alpha = 0.12 + t * 0.75;
  return `rgba(41, 41, 41, ${alpha})`;
}

export function YumaConsensusDemo() {
  const [weights, setWeights] = useState(DEFAULT_WEIGHTS.map((row) => [...row]));
  const [stakes, setStakes] = useState(DEFAULT_STAKES);
  const [kappa, setKappa] = useState(DEFAULT_KAPPA);

  const {consensus, clipped, incentive} = useMemo(
    () => yumaIncentives(weights, stakes, kappa),
    [weights, stakes, kappa],
  );

  const maxWeight = Math.max(...weights.flat(), ...clipped.flat(), 0.001);

  const setWeight = (vi: number, mi: number, value: number) => {
    setWeights((prev) => prev.map((row, i) => (i === vi ? row.map((w, j) => (j === mi ? value : w)) : row)));
  };

  const setStake = (vi: number, value: number) => {
    setStakes((prev) => prev.map((s, i) => (i === vi ? value : s)));
  };

  return (
    <ExplainerPanel
      title="Yuma consensus (simplified)"
      caption="Three validators score three miners. Consensus is the stake-weighted median (κ≈0.5); weights above it are clipped before rank → incentive."
    >
      <div className="overflow-x-auto">
        <table className="w-full min-w-[28rem] border-collapse border border-line text-[0.8125rem]">
          <thead>
            <tr className="border-b border-line bg-bg">
              <th className="bt-label px-2 py-2 text-left text-mute">Validator</th>
              <th className="bt-label px-2 py-2 text-left text-mute">Stake</th>
              {MINERS.map((m) => (
                <th key={m} className="bt-label px-2 py-2 text-center text-mute">
                  {m}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {VALIDATORS.map((v, vi) => (
              <tr key={v} className="border-b border-line">
                <td className="px-2 py-2 font-mono">{v}</td>
                <td className="px-2 py-2">
                  <input
                    type="range"
                    min={5}
                    max={60}
                    value={stakes[vi]}
                    onChange={(e) => setStake(vi, Number(e.target.value))}
                    className="w-20 accent-[var(--bt-fg)]"
                  />
                  <span className="ml-1 font-mono text-xs">{stakes[vi]}</span>
                </td>
                {MINERS.map((_, mi) => (
                  <td key={mi} className="px-1 py-1">
                    <div
                      className="rounded-sm px-2 py-1 text-center font-mono text-xs"
                      style={{backgroundColor: cellColor(weights[vi][mi], maxWeight)}}
                    >
                      {weights[vi][mi].toFixed(2)}
                    </div>
                    <input
                      type="range"
                      min={0}
                      max={1}
                      step={0.05}
                      value={weights[vi][mi]}
                      onChange={(e) => setWeight(vi, mi, Number(e.target.value))}
                      className="mt-1 w-full accent-[var(--bt-fg)]"
                    />
                  </td>
                ))}
              </tr>
            ))}
            <tr className="border-b border-line bg-bg">
              <td className="px-2 py-2 font-mono">Consensus</td>
              <td className="px-2 py-2 text-mute">—</td>
              {consensus.map((c, j) => (
                <td key={j} className="px-2 py-2 text-center font-mono">
                  {c.toFixed(2)}
                </td>
              ))}
            </tr>
            <tr className="border-b border-line">
              <td className="px-2 py-2 font-mono">Clipped</td>
              <td className="px-2 py-2 text-mute">—</td>
              {MINERS.map((_, mi) => (
                <td key={mi} className="px-2 py-2 text-center font-mono text-mute">
                  {clipped.map((row) => row[mi].toFixed(2)).join(' / ')}
                </td>
              ))}
            </tr>
            <tr>
              <td className="px-2 py-2 font-mono">Incentive</td>
              <td className="px-2 py-2 text-mute">—</td>
              {incentive.map((inc, j) => (
                <td key={j} className="px-2 py-2 text-center font-mono font-medium">
                  {formatPct(inc)}
                </td>
              ))}
            </tr>
          </tbody>
        </table>
      </div>

      <div className="mt-4 grid gap-4 sm:grid-cols-2">
        <ExplainerSlider
          label="Kappa (stake majority for median)"
          value={kappa}
          min={0.1}
          max={0.9}
          step={0.05}
          display={formatPct(kappa)}
          onChange={setKappa}
        />
        <ExplainerStat
          label="Stake weight formula"
          value={`α + τ × ${DEFAULT_TAO_WEIGHT}`}
          hint="Alpha stake plus root TAO scaled by tao_weight"
        />
      </div>
    </ExplainerPanel>
  );
}
