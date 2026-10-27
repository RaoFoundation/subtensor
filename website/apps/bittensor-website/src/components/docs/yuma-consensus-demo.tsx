'use client';

import { useMemo, useState } from 'react';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import {
  DEFAULT_KAPPA,
  DEFAULT_TAO_WEIGHT,
  formatPct,
  yumaIncentives,
} from '@/lib/emission-math';
import { ACCENT, ACCENT_WASH } from './chart-theme';

const VALIDATORS = ['V1', 'V2', 'V3'] as const;
const MINERS = ['M1', 'M2', 'M3'] as const;

const DEFAULT_WEIGHTS = [
  [0.6, 0.3, 0.1],
  [0.2, 0.5, 0.3],
  [0.1, 0.2, 0.7],
];

const DEFAULT_STAKES = [40, 35, 25];

const CLIP_EPSILON = 1e-9;

export function YumaConsensusDemo() {
  const [weights, setWeights] = useState(DEFAULT_WEIGHTS.map((row) => [...row]));
  const [stakes, setStakes] = useState(DEFAULT_STAKES);
  const [kappa, setKappa] = useState(DEFAULT_KAPPA);

  const {consensus, clipped, incentive} = useMemo(
    () => yumaIncentives(weights, stakes, kappa),
    [weights, stakes, kappa],
  );

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
        <table className="w-full min-w-[30rem] border-collapse text-[0.8125rem]">
          <thead>
            <tr className="border-b border-line">
              <th className="bt-label px-2 py-2 text-left text-mute">Validator</th>
              <th className="bt-label px-2 py-2 text-left text-mute">Stake</th>
              {MINERS.map((m) => (
                <th key={m} className="bt-label px-2 py-2 text-left text-mute">
                  {m}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {VALIDATORS.map((v, vi) => (
              <tr key={v} className="border-b border-line">
                <td className="px-2 py-3 align-top font-mono">{v}</td>
                <td className="w-24 px-2 py-3 align-top">
                  <ExplainerSlider
                    label=""
                    value={stakes[vi]}
                    min={5}
                    max={60}
                    step={1}
                    display={String(stakes[vi])}
                    onChange={(value) => setStake(vi, value)}
                  />
                </td>
                {MINERS.map((_, mi) => {
                  const isClipped = weights[vi][mi] > consensus[mi] + CLIP_EPSILON;
                  return (
                    <td
                      key={mi}
                      className="px-2 py-3 align-top"
                      // Washed cells mark weights above consensus — the ones Yuma clips.
                      style={isClipped ? {backgroundColor: ACCENT_WASH} : undefined}
                    >
                      <ExplainerSlider
                        label=""
                        value={weights[vi][mi]}
                        min={0}
                        max={1}
                        step={0.05}
                        display={weights[vi][mi].toFixed(2)}
                        onChange={(value) => setWeight(vi, mi, value)}
                      />
                    </td>
                  );
                })}
              </tr>
            ))}
            <tr className="border-b border-line">
              <td className="bt-label px-2 py-2 text-mute">Consensus</td>
              <td className="px-2 py-2 text-mute">—</td>
              {consensus.map((c, j) => (
                <td key={j} className="px-2 py-2 font-mono">
                  {c.toFixed(2)}
                </td>
              ))}
            </tr>
            <tr className="border-b border-line">
              <td className="bt-label px-2 py-2 text-mute">Clipped</td>
              <td className="px-2 py-2 text-mute">—</td>
              {MINERS.map((_, mi) => (
                <td key={mi} className="px-2 py-2 font-mono text-mute">
                  {clipped.map((row, vi) => (
                    <span key={vi}>
                      {vi > 0 && ' / '}
                      <span style={row[mi] < weights[vi][mi] - CLIP_EPSILON ? {color: ACCENT} : undefined}>
                        {row[mi].toFixed(2)}
                      </span>
                    </span>
                  ))}
                </td>
              ))}
            </tr>
            <tr className="border-b border-line">
              <td className="bt-label px-2 py-2 text-mute">Incentive</td>
              <td className="px-2 py-2 text-mute">—</td>
              {incentive.map((inc, j) => (
                <td key={j} className="px-2 py-2 font-mono font-medium">
                  {formatPct(inc)}
                </td>
              ))}
            </tr>
          </tbody>
        </table>
      </div>

      <div className="mt-6 grid gap-x-8 gap-y-5 border-t border-line pt-4 sm:grid-cols-2">
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
