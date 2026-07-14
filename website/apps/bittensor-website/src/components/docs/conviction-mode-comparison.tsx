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
  formatAlpha,
  formatBlocks,
  formatPct,
  rollForwardLock,
} from '@/lib/emission-math';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Tooltip, Legend);

export function ConvictionModeComparison() {
  const [lockedMass, setLockedMass] = useState(500_000);
  const [horizon, setHorizon] = useState(MATURITY_RATE_BLOCKS * 1.5);

  const chart = useMemo(() => {
    const steps = 70;
    const labels = Array.from({length: steps + 1}, (_, i) => (horizon * i) / steps);
    const perpetual = labels.map((dt) => rollForwardLock(lockedMass, 0, dt, {perpetual: true}));
    const decaying = labels.map((dt) => rollForwardLock(lockedMass, 0, dt, {perpetual: false}));
    return {labels, perpetual, decaying};
  }, [lockedMass, horizon]);

  const data = useMemo(
    () => ({
      labels: chart.labels.map((b) => String(Math.round(b))),
      datasets: [
        {
          label: 'Perpetual — conviction',
          data: chart.perpetual.map((p) => p.conviction),
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1.5,
          pointRadius: 0,
          tension: 0.25,
        },
        {
          label: 'Decaying — conviction',
          data: chart.decaying.map((p) => p.conviction),
          borderColor: 'rgba(41, 41, 41, 0.5)',
          borderWidth: 1.5,
          borderDash: [5, 3],
          pointRadius: 0,
          tension: 0.25,
        },
        {
          label: 'Decaying — locked mass',
          data: chart.decaying.map((p) => p.lockedMass),
          borderColor: 'rgba(110, 110, 110, 0.45)',
          borderWidth: 1,
          pointRadius: 0,
          tension: 0.25,
        },
      ],
    }),
    [chart],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: {
          position: 'bottom' as const,
          labels: {boxWidth: 10, font: {size: 10}},
        },
      },
      scales: {
        x: {ticks: {maxTicksLimit: 6, font: {size: 10}}},
        y: {
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
          ticks: {callback: (v: number | string) => formatAlpha(Number(v)), font: {size: 10}},
        },
      },
    }),
    [],
  );

  const atTau = rollForwardLock(lockedMass, 0, MATURITY_RATE_BLOCKS, {perpetual: true});
  const decayPeak = chart.decaying.reduce(
    (best, p) => (p.conviction > best.conviction ? p : best),
    chart.decaying[0],
  );

  return (
    <ExplainerPanel
      title="Perpetual vs decaying lock"
      caption="Same 500k α lock on a subnet hotkey. Perpetual: mass stays, conviction approaches mass. Decaying: mass frees on UnlockRate; conviction peaks then falls."
    >
      <div className="h-48">
        <Line data={data} options={options} />
      </div>

      <div className="mt-4 grid gap-3 sm:grid-cols-3">
        <ExplainerStat
          label="Perpetual at 1τ"
          value={formatAlpha(atTau.conviction)}
          hint={`${formatPct(atTau.conviction / lockedMass)} of mass`}
        />
        <ExplainerStat
          label="Decaying peak conviction"
          value={formatAlpha(decayPeak.conviction)}
          hint="Rises while mass decays, then falls"
        />
        <ExplainerStat label="Locked mass (start)" value={formatAlpha(lockedMass)} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <ExplainerSlider
          label="Locked mass"
          value={lockedMass}
          min={50_000}
          max={1_000_000}
          step={25_000}
          display={formatAlpha(lockedMass)}
          onChange={setLockedMass}
        />
        <ExplainerSlider
          label="Chart horizon"
          value={horizon}
          min={200_000}
          max={2_500_000}
          step={50_000}
          display={formatBlocks(horizon)}
          onChange={setHorizon}
        />
      </div>
    </ExplainerPanel>
  );
}
