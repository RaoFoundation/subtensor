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

// Matches sigmoid_safe in pallets/subtensor/src/epoch/math.rs:
// 1 / (1 + exp(-rho * (input - kappa)))
function trustSigmoid(input: number, rho: number, kappa: number): number {
  return 1 / (1 + Math.exp(-rho * (input - kappa)));
}

export function HyperparamConsensusSigmoid({ focus }: { focus?: string }) {
  const [rho, setRho] = useState(10);
  const [kappa, setKappa] = useState(0.5);

  const label = (name: string, hint: string) =>
    (focus === name ? '▸ ' : '') + `${name} (${hint})`;

  const xs = useMemo(
    () => Array.from({length: SAMPLE_POINTS + 1}, (_, i) => i / SAMPLE_POINTS),
    [],
  );
  const ys = useMemo(() => xs.map((x) => trustSigmoid(x, rho, kappa)), [xs, rho, kappa]);

  const data = useMemo(
    () => ({
      labels: xs.map((x) => x.toFixed(2)),
      datasets: [
        {
          label: 'Trust',
          data: ys,
          borderColor: 'rgb(41, 41, 41)',
          backgroundColor: 'rgba(41, 41, 41, 0.08)',
          fill: true,
          tension: 0,
          pointRadius: 0,
          borderWidth: 1.5,
        },
      ],
    }),
    [xs, ys],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: {mode: 'index' as const, intersect: false},
      plugins: {
        legend: {display: false},
        tooltip: {
          callbacks: {
            title: (items: {dataIndex: number}[]) => {
              const idx = items[0]?.dataIndex ?? 0;
              return `Alignment ${(xs[idx] ?? 0).toFixed(2)}`;
            },
            label: (ctx: {parsed: {y: number}}) => `trust ${ctx.parsed.y.toFixed(4)}`,
          },
        },
      },
      scales: {
        x: {
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
    [xs],
  );

  return (
    <ExplainerPanel
      title="rho / kappa trust sigmoid"
      caption="The classic Yuma trust curve, sigmoid_safe in epoch/math.rs: trust = 1 / (1 + e^(−rho × (x − kappa))). kappa is the midpoint, rho the steepness. The live epoch computes consensus as a kappa-weighted median; this sigmoid is the formulation rho parameterizes."
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
