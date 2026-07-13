'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  BarElement,
  LineElement,
  PointElement,
  Tooltip,
  Legend,
} from 'chart.js';
import type { ChartData, ChartOptions } from 'chart.js';
import { Chart } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

ChartJS.register(
  CategoryScale,
  LinearScale,
  BarElement,
  LineElement,
  PointElement,
  Tooltip,
  Legend,
);

/** A fleet of validators, each stuck on the version key its software sends.
 * The key is an opaque u64; these values just illustrate a release history. */
const VALIDATORS = [
  { label: 'val A', versionKey: 0 },
  { label: 'val B', versionKey: 990 },
  { label: 'val C', versionKey: 1000 },
  { label: 'val D', versionKey: 1010 },
  { label: 'val E', versionKey: 1020 },
  { label: 'val F', versionKey: 1030 },
];

/** Mirrors the chain's check_version_key: passes when the subnet's
 * WeightsVersionKey is 0 (gate disabled) or the submitted key is >= it. */
function passesGate(versionKey: number, required: number): boolean {
  return required === 0 || versionKey >= required;
}

export function HyperparamWeightsVersionGate() {
  const [required, setRequired] = useState(1010);

  const accepted = VALIDATORS.filter((v) => passesGate(v.versionKey, required));

  const data = useMemo<ChartData<'bar' | 'line', number[], string>>(
    () => ({
      labels: VALIDATORS.map((v) => v.label),
      datasets: [
        {
          type: 'bar' as const,
          label: 'submitted version_key',
          data: VALIDATORS.map((v) => v.versionKey),
          backgroundColor: VALIDATORS.map((v) =>
            passesGate(v.versionKey, required)
              ? 'rgba(41, 41, 41, 0.75)'
              : 'rgba(41, 41, 41, 0.12)',
          ),
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1,
        },
        {
          type: 'line' as const,
          label: 'weights_version',
          data: VALIDATORS.map(() => required),
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1,
          borderDash: [4, 4],
          pointRadius: 0,
        },
      ],
    }),
    [required],
  );

  const options = useMemo<ChartOptions<'bar' | 'line'>>(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: { display: false },
        tooltip: {
          filter: (item) => item.dataset.type !== 'line',
          callbacks: {
            label: (ctx) => {
              const validator = VALIDATORS[ctx.dataIndex];
              if (required === 0) return `accepted — gate disabled (weights_version = 0)`;
              return passesGate(validator.versionKey, required)
                ? `accepted — key ${validator.versionKey} ≥ ${required}`
                : `IncorrectWeightVersionKey — key ${validator.versionKey} < ${required}`;
            },
          },
        },
      },
      scales: {
        x: {
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: { font: { family: 'FiraCode, monospace', size: 10 } },
        },
        y: {
          min: 0,
          max: 1100,
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: { font: { family: 'FiraCode, monospace', size: 10 } },
          title: { display: true, text: 'version_key sent', font: { size: 11 } },
        },
      },
    }),
    [required],
  );

  return (
    <ExplainerPanel
      title="Version gate"
      caption="Each bar is a validator submitting weights with the version key its software sends. Bars reaching the dashed line (key ≥ weights_version) pass; the rest fail with IncorrectWeightVersionKey until they upgrade. At 0 the gate is disabled and everything passes — including version_key 0."
    >
      <div className="h-52">
        <Chart type="bar" data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="weights_version"
          value={required === 0 ? '0 (gate disabled)' : String(required)}
          hint="opaque u64; meaning is the owner's convention"
        />
        <ExplainerStat
          label="Accepted"
          value={`${accepted.length} / ${VALIDATORS.length} validators`}
          hint="dark bars clear the gate"
        />
        <ExplainerStat
          label="Rejected with"
          value="IncorrectWeightVersionKey"
          hint="key below the subnet's gate"
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="weights_version (required key)"
          value={required}
          min={0}
          max={1040}
          step={10}
          display={required === 0 ? 'disabled' : String(required)}
          onChange={setRequired}
        />
      </div>
    </ExplainerPanel>
  );
}
