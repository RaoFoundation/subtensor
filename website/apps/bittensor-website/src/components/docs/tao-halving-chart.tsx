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
import {
  TOTAL_SUPPLY_TAO,
  blockEmissionTao,
  formatTao,
  halvingThresholdsTao,
} from '@/lib/emission-math';
import { DEFAULT_EMISSION_SNAPSHOT } from '@/lib/emission-snapshot';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip, Legend);

const SAMPLE_POINTS = 120;

export function TaoHalvingChart() {
  const [issuance, setIssuance] = useState(DEFAULT_EMISSION_SNAPSHOT.totalIssuanceTao);

  const chart = useMemo(() => {
    const xs = Array.from({length: SAMPLE_POINTS + 1}, (_, i) => (TOTAL_SUPPLY_TAO * i) / SAMPLE_POINTS);
    const ys = xs.map(blockEmissionTao);
    return {xs, ys};
  }, []);

  const currentEmission = blockEmissionTao(issuance);
  const thresholds = halvingThresholdsTao(6);

  const data = useMemo(
    () => ({
      labels: chart.xs.map((x) => `${(x / 1_000_000).toFixed(1)}M`),
      datasets: [
        {
          label: 'Block emission (τ)',
          data: chart.ys,
          borderColor: 'rgb(41, 41, 41)',
          backgroundColor: 'rgba(41, 41, 41, 0.08)',
          fill: true,
          tension: 0,
          pointRadius: 0,
          borderWidth: 1.5,
        },
      ],
    }),
    [chart],
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
              return `Issuance ${formatTao(chart.xs[idx], 2)}`;
            },
            label: (ctx: {parsed: {y: number}}) => `${ctx.parsed.y.toFixed(4)} τ / block`,
          },
        },
      },
      scales: {
        x: {
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {maxTicksLimit: 8, font: {family: 'FiraCode, monospace', size: 10}},
        },
        y: {
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {font: {family: 'FiraCode, monospace', size: 10}},
          title: {display: true, text: 'τ / block', font: {size: 11}},
        },
      },
    }),
    [chart.xs],
  );

  const nextThreshold = thresholds.find((t) => t > issuance);

  return (
    <ExplainerPanel
      title="TAO halving curve"
      caption={`Matches get_block_emission_for_issuance. Finney issuance today ≈ ${formatTao(DEFAULT_EMISSION_SNAPSHOT.totalIssuanceTao, 2)} → ${formatTao(DEFAULT_EMISSION_SNAPSHOT.blockEmissionTao)}/block.`}
    >
      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat label="At selected issuance" value={formatTao(currentEmission) + ' / block'} />
        <ExplainerStat
          label="Daily at 12s blocks"
          value={formatTao(currentEmission * 7200, 2)}
          hint="7,200 blocks per day"
        />
        <ExplainerStat
          label="Next halving near"
          value={nextThreshold ? formatTao(nextThreshold, 2) + ' issued' : 'Cap reached'}
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="Total issuance"
          value={issuance}
          min={0}
          max={21_000_000}
          step={100_000}
          display={formatTao(issuance, 2)}
          onChange={setIssuance}
        />
      </div>
    </ExplainerPanel>
  );
}
