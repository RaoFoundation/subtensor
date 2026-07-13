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
} from 'chart.js';
import type { ChartData } from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip);

const INTERVAL_BLOCKS = 360; // one root epoch (tempo) — the counter resets here
const SAMPLE_EVERY = 4;

export function HyperparamRegsCapChart() {
  const [target, setTarget] = useState(2);
  const [attemptsPerInterval, setAttemptsPerInterval] = useState(10);

  const cap = 3 * target;

  const sim = useMemo(() => {
    const xs: number[] = [];
    const attempted: number[] = [];
    const admitted: number[] = [];
    let capHitBlock: number | null = null;

    for (let b = 0; b <= INTERVAL_BLOCKS; b += SAMPLE_EVERY) {
      // Attempts arrive evenly spread across the interval.
      const att = Math.floor((b * attemptsPerInterval) / INTERVAL_BLOCKS);
      const adm = Math.min(att, cap);
      if (capHitBlock === null && att >= cap && cap > 0) capHitBlock = b;
      xs.push(b);
      attempted.push(att);
      admitted.push(adm);
    }

    const totalAttempted = attemptsPerInterval;
    const totalAdmitted = Math.min(totalAttempted, cap);
    return { xs, attempted, admitted, capHitBlock, rejected: totalAttempted - totalAdmitted };
  }, [attemptsPerInterval, cap]);

  const data = useMemo(() => {
    const datasets: ChartData<'line', number[]>['datasets'] = [
      {
        label: 'admitted',
        data: sim.admitted,
        borderColor: 'rgb(41, 41, 41)',
        backgroundColor: 'rgba(41, 41, 41, 0.08)',
        fill: true,
        stepped: true,
        pointRadius: 0,
        borderWidth: 1.5,
      },
      {
        label: 'attempted',
        data: sim.attempted,
        borderColor: 'rgba(41, 41, 41, 0.35)',
        borderDash: [3, 3],
        stepped: true,
        pointRadius: 0,
        borderWidth: 1,
        fill: false,
      },
      {
        label: 'cap (3 × target)',
        data: sim.xs.map(() => cap),
        borderColor: 'rgb(41, 41, 41)',
        borderDash: [6, 4],
        pointRadius: 0,
        borderWidth: 1.5,
        fill: false,
      },
    ];
    return {
      labels: sim.xs.map((b) => `${b}`),
      datasets,
    };
  }, [sim, cap]);

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: { mode: 'index' as const, intersect: false },
      plugins: {
        legend: { display: false },
        tooltip: {
          callbacks: {
            title: (items: { dataIndex: number }[]) => {
              const idx = items[0]?.dataIndex ?? 0;
              return `block ${sim.xs[idx] ?? 0} of interval`;
            },
            label: (ctx: { dataset: { label?: string }; parsed: { y: number } }) =>
              `${ctx.dataset.label}: ${ctx.parsed.y}`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          ticks: { maxTicksLimit: 8, font: { family: 'FiraCode, monospace', size: 10 } },
          title: { display: true, text: 'blocks into interval', font: { size: 11 } },
        },
        y: {
          grid: { color: 'rgba(41, 41, 41, 0.06)' },
          beginAtZero: true,
          ticks: {
            maxTicksLimit: 6,
            precision: 0,
            font: { family: 'FiraCode, monospace', size: 10 },
          },
          title: { display: true, text: 'registrations', font: { size: 11 } },
        },
      },
    }),
    [sim.xs],
  );

  return (
    <ExplainerPanel
      title="The 3× hard cap"
      caption="Registrations accumulating over one interval (the root subnet's epoch). Attempts (dotted) arrive evenly; the chain admits them (solid) only until the count reaches 3 × target_regs_per_interval (dashed) — everything after that is rejected with TooManyRegistrationsThisInterval until the counter resets at the epoch boundary."
    >
      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat label="Cap per interval" value={`${cap}`} hint={`3 × target (${target})`} />
        <ExplainerStat
          label="Cap hit"
          value={
            sim.capHitBlock !== null
              ? `~block ${sim.capHitBlock}`
              : 'never'
          }
          hint={sim.capHitBlock !== null ? 'later attempts are rejected' : 'demand fits under the cap'}
        />
        <ExplainerStat label="Rejected" value={`${Math.max(sim.rejected, 0)}`} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <div className="border-l-2 border-[var(--bt-fg)] pl-3">
          <ExplainerSlider
            label="target_regs_per_interval"
            value={target}
            min={1}
            max={16}
            step={1}
            display={`${target} (cap ${3 * target})`}
            onChange={setTarget}
          />
        </div>
        <ExplainerSlider
          label="registration attempts / interval"
          value={attemptsPerInterval}
          min={0}
          max={60}
          step={1}
          display={`${attemptsPerInterval}`}
          onChange={setAttemptsPerInterval}
        />
      </div>
    </ExplainerPanel>
  );
}
