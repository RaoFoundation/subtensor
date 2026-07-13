'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Filler,
  Tooltip,
  Legend,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip, Legend);

const SAMPLE_POINTS = 100;
const KAPPA_U16_MAX = 65535;
const RHO_GHOSTS = [2, 40];

// Matches sigmoid_safe in pallets/subtensor/src/epoch/math.rs:
// 1 / (1 + exp(-rho * (input - kappa)))
function trustSigmoid(input: number, rho: number, kappa: number): number {
  return 1 / (1 + Math.exp(-rho * (input - kappa)));
}

function sigmoidPoints(rho: number, kappa: number): {x: number; y: number}[] {
  return Array.from({length: SAMPLE_POINTS + 1}, (_, i) => {
    const x = i / SAMPLE_POINTS;
    return {x, y: trustSigmoid(x, rho, kappa)};
  });
}

const CAPTIONS: Record<string, string> = {
  rho: 'rho is the temperature of the trust sigmoid, sigmoid_safe in epoch/math.rs: trust = 1 / (1 + e^(−rho × (x − kappa))). The dashed ghost curves fix rho at 2 and 40 — slide rho between them and watch the curve snap from a gentle ramp into a near-step at kappa.',
  kappa:
    'kappa is the midpoint of the trust sigmoid, sigmoid_safe in epoch/math.rs: trust = 1 / (1 + e^(−rho × (x − kappa))). The dashed marker is the majority threshold — alignment crossing kappa flips trust through 0.5, from mostly-distrusted to mostly-trusted. Slide kappa to move the crossing.',
};

const DEFAULT_CAPTION =
  'The classic Yuma trust curve, sigmoid_safe in epoch/math.rs: trust = 1 / (1 + e^(−rho × (x − kappa))). kappa is the midpoint, rho the steepness. The live epoch computes consensus as a kappa-weighted median; this sigmoid is the formulation rho parameterizes.';

export function HyperparamConsensusSigmoid({ focus }: { focus?: string }) {
  const [rho, setRho] = useState(10);
  const [kappa, setKappa] = useState(0.5);

  const label = (name: string, hint: string) =>
    (focus === name ? '▸ ' : '') + `${name} (${hint})`;

  const datasets = useMemo(() => {
    const main = {
      label: 'Trust',
      data: sigmoidPoints(rho, kappa),
      borderColor: 'rgb(41, 41, 41)',
      backgroundColor: 'rgba(41, 41, 41, 0.08)',
      fill: true,
      tension: 0,
      pointRadius: 0,
      borderWidth: 1.5,
      order: 0,
    };

    if (focus === 'rho') {
      // Ghost curves bracketing the rho range make the steepness sweep visible.
      const ghosts = RHO_GHOSTS.map((ghostRho) => ({
        label: `rho = ${ghostRho}`,
        data: sigmoidPoints(ghostRho, kappa),
        borderColor: 'rgba(41, 41, 41, 0.25)',
        borderDash: [4, 4],
        borderWidth: 1,
        fill: false,
        tension: 0,
        pointRadius: 0,
        order: 1,
      }));
      return [main, ...ghosts];
    }

    if (focus === 'kappa') {
      // Vertical marker at the majority threshold plus the 0.5 crossing point.
      const threshold = {
        label: 'kappa threshold',
        data: [
          {x: kappa, y: 0},
          {x: kappa, y: 1},
        ],
        borderColor: 'rgba(41, 41, 41, 0.45)',
        borderDash: [6, 4],
        borderWidth: 1.5,
        fill: false,
        tension: 0,
        pointRadius: 0,
        order: 1,
      };
      const crossing = {
        label: 'majority crossing',
        data: [{x: kappa, y: 0.5}],
        borderColor: 'rgb(41, 41, 41)',
        backgroundColor: 'rgb(41, 41, 41)',
        showLine: false,
        pointRadius: 4,
        pointStyle: 'rectRot' as const,
        order: 2,
      };
      return [main, threshold, crossing];
    }

    return [main];
  }, [rho, kappa, focus]);

  const data = useMemo(() => ({datasets}), [datasets]);

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: {mode: 'nearest' as const, axis: 'x' as const, intersect: false},
      plugins: {
        legend: {display: false},
        tooltip: {
          callbacks: {
            title: (items: {parsed: {x: number}}[]) =>
              `Alignment ${(items[0]?.parsed.x ?? 0).toFixed(2)}`,
            label: (ctx: {parsed: {y: number}; dataset: {label?: string}}) => {
              const name = ctx.dataset.label ?? 'trust';
              if (name === 'kappa threshold') return `majority threshold at ${kappa.toFixed(2)}`;
              if (name === 'majority crossing') return 'trust flips through 0.5 here';
              const prefix = name.startsWith('rho') ? `${name}: ` : '';
              return `${prefix}trust ${ctx.parsed.y.toFixed(4)}`;
            },
          },
        },
      },
      scales: {
        x: {
          type: 'linear' as const,
          min: 0,
          max: 1,
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {maxTicksLimit: 11, font: {family: 'FiraCode, monospace', size: 10}},
          title: {display: true, text: 'consensus alignment (stake fraction)', font: {size: 11}},
        },
        y: {
          min: 0,
          max: 1,
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {font: {family: 'FiraCode, monospace', size: 10}},
          title: {display: true, text: 'trust', font: {size: 11}},
        },
      },
    }),
    [kappa],
  );

  return (
    <ExplainerPanel
      title="rho / kappa trust sigmoid"
      caption={(focus && CAPTIONS[focus]) || DEFAULT_CAPTION}
    >
      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Trust at midpoint"
          value={trustSigmoid(kappa, rho, kappa).toFixed(2)}
          hint="always 0.5 at x = kappa"
        />
        <ExplainerStat label="Slope at midpoint" value={(rho / 4).toFixed(2)} hint="rho / 4" />
        <ExplainerStat
          label="kappa raw (u16)"
          value={String(Math.round(kappa * KAPPA_U16_MAX))}
          hint="65535 = 1.0; 32767 ≈ 0.5"
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <ExplainerSlider
          label={label('rho', 'curve steepness')}
          value={rho}
          min={1}
          max={40}
          step={1}
          display={String(rho)}
          onChange={setRho}
        />
        <ExplainerSlider
          label={label('kappa', 'majority threshold')}
          value={kappa}
          min={0.1}
          max={0.9}
          step={0.01}
          display={kappa.toFixed(2)}
          onChange={setKappa}
        />
      </div>
    </ExplainerPanel>
  );
}
