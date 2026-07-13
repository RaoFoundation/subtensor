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

/** Blocks at which the validator tries to submit weights. The validator's
 * previous accepted submission is at block 0, so every attempt has a
 * well-defined gap. */
const ATTEMPT_BLOCKS = [10, 60, 130, 150, 210, 240, 320, 340];
const SECONDS_PER_BLOCK = 12;

interface Attempt {
  block: number;
  gap: number;
  accepted: boolean;
}

/** Mirrors the chain's check_rate_limit: an attempt passes when
 * current_block - last_set >= weights_rate_limit, and an accepted
 * submission resets LastUpdate. */
function simulateAttempts(limit: number): Attempt[] {
  let lastAccepted = 0;
  return ATTEMPT_BLOCKS.map((block) => {
    const gap = block - lastAccepted;
    const accepted = gap >= limit;
    if (accepted) lastAccepted = block;
    return { block, gap, accepted };
  });
}

function formatMinutes(blocks: number): string {
  const minutes = (blocks * SECONDS_PER_BLOCK) / 60;
  return minutes >= 60
    ? `${(minutes / 60).toFixed(1)} h`
    : `${minutes.toFixed(0)} min`;
}

export function HyperparamWeightsRateLimitChart() {
  const [limit, setLimit] = useState(100);

  const attempts = useMemo(() => simulateAttempts(limit), [limit]);
  const acceptedCount = attempts.filter((a) => a.accepted).length;

  const data = useMemo<ChartData<'bar' | 'line', number[], string>>(
    () => ({
      labels: attempts.map((a) => `block ${a.block}`),
      datasets: [
        {
          type: 'bar' as const,
          label: 'blocks since last accepted',
          data: attempts.map((a) => a.gap),
          backgroundColor: attempts.map((a) =>
            a.accepted ? 'rgba(41, 41, 41, 0.75)' : 'rgba(41, 41, 41, 0.12)',
          ),
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1,
        },
        {
          type: 'line' as const,
          label: 'weights_rate_limit',
          data: attempts.map(() => limit),
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1,
          borderDash: [4, 4],
          pointRadius: 0,
        },
      ],
    }),
    [attempts, limit],
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
              const attempt = attempts[ctx.dataIndex];
              return attempt.accepted
                ? `accepted — gap ${attempt.gap} ≥ limit ${limit}`
                : `SettingWeightsTooFast — gap ${attempt.gap} < limit ${limit}`;
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
          suggestedMax: limit + 40,
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: { font: { family: 'FiraCode, monospace', size: 10 } },
          title: { display: true, text: 'blocks since last accepted', font: { size: 11 } },
        },
      },
    }),
    [attempts, limit],
  );

  return (
    <ExplainerPanel
      title="Rate-limit timeline"
      caption="Each bar is a set_weights attempt; its height is the number of blocks since the last accepted submission. Attempts reaching the dashed line (gap ≥ weights_rate_limit) pass and reset the clock; shorter ones fail with SettingWeightsTooFast. A UID that has never submitted always passes."
    >
      <div className="h-52">
        <Chart type="bar" data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="weights_rate_limit"
          value={`${limit} blocks`}
          hint={`≈ ${formatMinutes(limit)} at 12s blocks`}
        />
        <ExplainerStat
          label="Accepted"
          value={`${acceptedCount} / ${attempts.length} attempts`}
          hint="dark bars on the timeline"
        />
        <ExplainerStat
          label="Rejected with"
          value="SettingWeightsTooFast"
          hint="inside the cooldown window"
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="weights_rate_limit"
          value={limit}
          min={10}
          max={200}
          step={10}
          display={`${limit} blocks ≈ ${formatMinutes(limit)}`}
          onChange={setLimit}
        />
      </div>
    </ExplainerPanel>
  );
}
