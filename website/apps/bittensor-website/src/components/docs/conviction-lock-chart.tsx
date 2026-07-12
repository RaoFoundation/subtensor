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
import { MATURITY_RATE_BLOCKS, formatBlocks, formatPct, perpetualConviction } from '@/lib/emission-math';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Tooltip, Legend);

export function ConvictionLockChart() {
  const [lockedMass, setLockedMass] = useState(1000);
  const [startConviction, setStartConviction] = useState(0);
  const [horizon, setHorizon] = useState(MATURITY_RATE_BLOCKS);

  const points = useMemo(() => {
    const steps = 80;
    return Array.from({length: steps + 1}, (_, i) => {
      const dt = (horizon * i) / steps;
      return {x: dt, y: perpetualConviction(lockedMass, startConviction, dt)};
    });
  }, [lockedMass, startConviction, horizon]);

  const data = useMemo(
    () => ({
      datasets: [
        {
          label: 'Conviction',
          data: points.map((p) => ({x: p.x, y: p.y})),
          borderColor: 'rgb(41, 41, 41)',
          borderWidth: 1.5,
          pointRadius: 0,
          tension: 0.2,
        },
        {
          label: 'Locked mass',
          data: points.map((p) => ({x: p.x, y: lockedMass})),
          borderColor: 'rgba(110, 110, 110, 0.6)',
          borderDash: [4, 4],
          borderWidth: 1,
          pointRadius: 0,
        },
      ],
    }),
    [points, lockedMass],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      plugins: {legend: {display: false}},
      scales: {
        x: {
          type: 'linear' as const,
          title: {display: true, text: 'Blocks since lock', font: {size: 11}},
          ticks: {
            callback: (v: number | string) => formatBlocks(Number(v)),
            maxTicksLimit: 6,
            font: {size: 10},
          },
        },
        y: {
          title: {display: true, text: 'α', font: {size: 11}},
          grid: {color: 'rgba(41, 41, 41, 0.06)'},
        },
      },
    }),
    [],
  );

  const atTau = perpetualConviction(lockedMass, startConviction, MATURITY_RATE_BLOCKS);
  const pctAtTau = lockedMass > 0 ? atTau / lockedMass : 0;

  return (
    <ExplainerPanel
      title="Perpetual conviction lock"
      caption="c₁ = m − (m − c₀)·e^(−Δt/τ). Locked mass stays fixed; conviction approaches mass asymptotically (~63% at 1τ)."
    >
      <div className="h-44">
        <Line data={data} options={options} />
      </div>

      <div className="mt-4 grid gap-3 sm:grid-cols-3">
        <ExplainerStat label="At 1τ (~130d scale)" value={formatPct(pctAtTau)} hint={`τ = ${MATURITY_RATE_BLOCKS.toLocaleString()} blocks`} />
        <ExplainerStat label="Locked mass (m)" value={`${lockedMass.toLocaleString()} α`} />
        <ExplainerStat label="Starting conviction (c₀)" value={`${startConviction.toLocaleString()} α`} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <ExplainerSlider
          label="Locked mass"
          value={lockedMass}
          min={100}
          max={5000}
          step={100}
          display={`${lockedMass.toLocaleString()} α`}
          onChange={setLockedMass}
        />
        <ExplainerSlider
          label="Starting conviction"
          value={startConviction}
          min={0}
          max={lockedMass}
          step={50}
          display={`${startConviction.toLocaleString()} α`}
          onChange={setStartConviction}
        />
        <ExplainerSlider
          label="Chart horizon"
          value={horizon}
          min={100_000}
          max={2_000_000}
          step={50_000}
          display={formatBlocks(horizon)}
          onChange={setHorizon}
        />
      </div>
    </ExplainerPanel>
  );
}
