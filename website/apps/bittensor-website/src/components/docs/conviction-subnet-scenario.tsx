'use client';

import { useMemo, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Tooltip,
  Legend,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import {
  MATURITY_RATE_BLOCKS,
  ONE_YEAR_BLOCKS,
  convictionOwnershipThreshold,
  formatAlpha,
  formatBlocks,
  formatPct,
  rollForwardLock,
} from '@/lib/emission-math';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Tooltip, Legend);

/** Fictional subnet 7 ("Atlas") — round numbers for teaching, not live chain state. */
export const EXAMPLE_SUBNET = {
  netuid: 7,
  name: 'Atlas',
  alphaOut: 8_000_000,
  ageBlocks: ONE_YEAR_BLOCKS + 500_000,
  owner: {name: 'Alice (owner)', lockedMass: 250_000, perpetual: true, ownerLock: true},
  validator: {name: 'Bob (validator)', lockedMass: 600_000, perpetual: true, ownerLock: false},
  staker: {name: 'Carol (staker)', lockedMass: 200_000, perpetual: false, ownerLock: false},
} as const;

function seriesForLocker(
  lockedMass: number,
  perpetual: boolean,
  ownerLock: boolean,
  horizon: number,
  steps: number,
): number[] {
  const points: number[] = [];
  for (let i = 0; i <= steps; i++) {
    const dt = (horizon * i) / steps;
    points.push(
      rollForwardLock(lockedMass, 0, dt, {perpetual, ownerLock}).conviction,
    );
  }
  return points;
}

export function ConvictionSubnetScenario() {
  const [elapsed, setElapsed] = useState(400_000);
  const [bobLock, setBobLock] = useState<number>(EXAMPLE_SUBNET.validator.lockedMass);
  const [carolLock, setCarolLock] = useState<number>(EXAMPLE_SUBNET.staker.lockedMass);
  const [carolPerpetual, setCarolPerpetual] = useState(false);

  const threshold = convictionOwnershipThreshold(EXAMPLE_SUBNET.alphaOut);
  const horizon = MATURITY_RATE_BLOCKS * 2;

  const chart = useMemo(() => {
    const steps = 60;
    const labels = Array.from({length: steps + 1}, (_, i) => (horizon * i) / steps);
    const alice = seriesForLocker(
      EXAMPLE_SUBNET.owner.lockedMass,
      true,
      true,
      horizon,
      steps,
    );
    const bob = seriesForLocker(bobLock, true, false, horizon, steps);
    const carol = seriesForLocker(carolLock, carolPerpetual, false, horizon, steps);
    const total = labels.map((_, i) => alice[i] + bob[i] + carol[i]);
    return {labels, alice, bob, carol, total};
  }, [bobLock, carolLock, carolPerpetual, horizon]);

  const now = useMemo(() => {
    const alice = rollForwardLock(EXAMPLE_SUBNET.owner.lockedMass, 0, elapsed, {
      perpetual: true,
      ownerLock: true,
    });
    const bob = rollForwardLock(bobLock, 0, elapsed, {perpetual: true});
    const carol = rollForwardLock(carolLock, 0, elapsed, {perpetual: carolPerpetual});
    const total = alice.conviction + bob.conviction + carol.conviction;
    const leader =
      [
        {name: 'Alice', c: alice.conviction},
        {name: 'Bob', c: bob.conviction},
        {name: 'Carol', c: carol.conviction},
      ].sort((a, b) => b.c - a.c)[0] ?? {name: '—', c: 0};

    return {alice, bob, carol, total, leader};
  }, [elapsed, bobLock, carolLock, carolPerpetual]);

  const ownershipReady =
    EXAMPLE_SUBNET.ageBlocks >= ONE_YEAR_BLOCKS && now.total >= threshold;

  const data = useMemo(
    () => ({
      labels: chart.labels.map((b) => String(Math.round(b))),
      datasets: [
        {
          label: 'Total conviction',
          data: chart.total,
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 2,
          pointRadius: 0,
          tension: 0.3,
        },
        {
          label: 'Bob (validator)',
          data: chart.bob,
          borderColor: 'rgba(41, 41, 41, 0.55)',
          borderWidth: 1,
          pointRadius: 0,
          tension: 0.3,
        },
        {
          label: 'Carol (staker)',
          data: chart.carol,
          borderColor: 'rgba(110, 110, 110, 0.7)',
          borderWidth: 1,
          borderDash: [4, 3],
          pointRadius: 0,
          tension: 0.3,
        },
        {
          label: '10% threshold',
          data: chart.labels.map(() => threshold),
          borderColor: 'rgba(110, 110, 110, 0.35)',
          borderWidth: 1,
          borderDash: [6, 4],
          pointRadius: 0,
        },
      ],
    }),
    [chart, threshold],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: {mode: 'index' as const, intersect: false},
      plugins: {
        legend: {
          display: true,
          position: 'bottom' as const,
          labels: {boxWidth: 10, font: {size: 10, family: 'FiraCode, monospace'}},
        },
        tooltip: {
          callbacks: {
            title: (items: {label: string}[]) => `Block +${items[0]?.label ?? ''}`,
            label: (ctx: {dataset: {label?: string}; parsed: {y: number}}) =>
              `${ctx.dataset.label}: ${formatAlpha(ctx.parsed.y)}`,
          },
        },
      },
      scales: {
        x: {
          title: {display: true, text: 'Blocks since locks started', font: {size: 11}},
          ticks: {maxTicksLimit: 6, font: {size: 10}},
        },
        y: {
          title: {display: true, text: 'Conviction (α)', font: {size: 11}},
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {
            callback: (v: number | string) => formatAlpha(Number(v)),
            font: {size: 10},
          },
        },
      },
    }),
    [],
  );

  return (
    <ExplainerPanel
      title={`Example: Subnet ${EXAMPLE_SUBNET.netuid} (${EXAMPLE_SUBNET.name})`}
      caption="Fictional numbers for illustration. Three coldkeys lock toward different hotkeys; total conviction must reach 10% of SubnetAlphaOut before ownership can transfer."
    >
      <div className="mb-4 grid gap-px border border-line bg-line sm:grid-cols-4">
        {[
          {label: 'SubnetAlphaOut', value: formatAlpha(EXAMPLE_SUBNET.alphaOut)},
          {label: '10% threshold', value: formatAlpha(threshold)},
          {label: 'Subnet age', value: formatBlocks(EXAMPLE_SUBNET.ageBlocks)},
          {label: 'Ownership gate', value: ownershipReady ? 'Open' : 'Closed'},
        ].map((item) => (
          <div key={item.label} className="bg-bg px-3 py-2">
            <p className="bt-label text-mute">{item.label}</p>
            <p className="mt-1 font-mono text-sm">{item.value}</p>
          </div>
        ))}
      </div>

      <div className="h-52">
        <Line data={data} options={options} />
      </div>

      <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <ExplainerStat
          label="Alice (owner hotkey)"
          value={formatAlpha(now.alice.conviction)}
          hint="Owner locks: conviction = locked mass instantly"
        />
        <ExplainerStat label="Bob (validator)" value={formatAlpha(now.bob.conviction)} hint="Perpetual lock" />
        <ExplainerStat
          label="Carol (staker)"
          value={formatAlpha(now.carol.conviction)}
          hint={carolPerpetual ? 'Perpetual' : 'Decaying — mass frees over time'}
        />
        <ExplainerStat
          label="Total / threshold"
          value={`${formatAlpha(now.total)} / ${formatAlpha(threshold)}`}
          hint={ownershipReady ? `Leader: ${now.leader.name}` : `${formatPct(now.total / threshold)} of gate`}
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <ExplainerSlider
          label="Simulate elapsed time"
          value={elapsed}
          min={0}
          max={horizon}
          step={10_000}
          display={formatBlocks(elapsed)}
          onChange={setElapsed}
        />
        <ExplainerSlider
          label="Bob's locked mass"
          value={bobLock}
          min={100_000}
          max={1_000_000}
          step={50_000}
          display={formatAlpha(bobLock)}
          onChange={setBobLock}
        />
        <ExplainerSlider
          label="Carol's locked mass"
          value={carolLock}
          min={50_000}
          max={400_000}
          step={25_000}
          display={formatAlpha(carolLock)}
          onChange={setCarolLock}
        />
        <label className="flex items-center gap-2 border border-line bg-bg px-3 py-2 text-[0.8125rem]">
          <input
            type="checkbox"
            checked={carolPerpetual}
            onChange={(e) => setCarolPerpetual(e.target.checked)}
            className="accent-[var(--bt-fg)]"
          />
          Carol uses perpetual mode (via set-perpetual-lock)
        </label>
      </div>
    </ExplainerPanel>
  );
}
