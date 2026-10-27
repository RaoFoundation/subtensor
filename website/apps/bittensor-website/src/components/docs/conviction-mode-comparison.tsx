'use client';

import { useMemo, useRef, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Tooltip,
  Legend,
  type Plugin,
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
import {
  AXIS_BORDER,
  GRAPH_FONT,
  GRID,
  INK,
  INK_FAINT,
  axisTitle,
  baseTicks,
} from './chart-theme';

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

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ lockedMass, horizon });
  drawState.current = { lockedMass, horizon };

  // Direct in-plot series labels instead of a legend, in the uppercase
  // FiraCode style of the v431 release graphs.
  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'modeComparisonAnnotations',
      afterDatasetsDraw(chart) {
        const { lockedMass, horizon } = drawState.current;
        const { ctx, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        ctx.save();
        ctx.font = GRAPH_FONT;
        ctx.textAlign = 'left';

        // Perpetual conviction rises toward mass; label above the curve.
        const xMain = horizon * 0.55;
        const perpetualY = rollForwardLock(lockedMass, 0, xMain, {perpetual: true}).conviction;
        ctx.fillStyle = INK;
        ctx.fillText('PERPETUAL', xScale.getPixelForValue(xMain) + 4, yScale.getPixelForValue(perpetualY) - 8);

        // Decaying conviction peaks then falls; label below the curve.
        const decayingY = rollForwardLock(lockedMass, 0, xMain, {perpetual: false}).conviction;
        ctx.fillStyle = INK_FAINT;
        ctx.fillText('DECAYING', xScale.getPixelForValue(xMain) + 4, yScale.getPixelForValue(decayingY) + 16);

        // Freed locked mass of the decaying lock, labelled at the left where
        // its steep descent is clear of the two conviction curves.
        const xMass = horizon * 0.12;
        const massY = rollForwardLock(lockedMass, 0, xMass, {perpetual: false}).lockedMass;
        ctx.fillText('DECAYING MASS', xScale.getPixelForValue(xMass) + 6, yScale.getPixelForValue(massY) - 8);

        ctx.restore();
      },
    }),
    [],
  );

  const data = useMemo(
    () => ({
      datasets: [
        {
          label: 'Perpetual — conviction',
          data: chart.labels.map((dt, i) => ({x: dt, y: chart.perpetual[i].conviction})),
          borderColor: INK,
          borderWidth: 1.5,
          pointRadius: 0,
          tension: 0.25,
        },
        {
          label: 'Decaying — conviction',
          data: chart.labels.map((dt, i) => ({x: dt, y: chart.decaying[i].conviction})),
          borderColor: 'rgba(41, 41, 41, 0.5)',
          borderWidth: 1.5,
          borderDash: [5, 3],
          pointRadius: 0,
          tension: 0.25,
        },
        {
          label: 'Decaying — locked mass',
          data: chart.labels.map((dt, i) => ({x: dt, y: chart.decaying[i].lockedMass})),
          borderColor: INK_FAINT,
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
        legend: {display: false},
        tooltip: {
          callbacks: {
            title: (items: {parsed: {x: number}}[]) =>
              `+${formatBlocks(items[0]?.parsed.x ?? 0)}`,
            label: (ctx: {dataset: {label?: string}; parsed: {y: number}}) =>
              `${ctx.dataset.label}: ${formatAlpha(ctx.parsed.y)}`,
          },
        },
      },
      scales: {
        x: {
          type: 'linear' as const,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({
            callback: (v: number | string) => formatBlocks(Number(v)),
          }),
          title: axisTitle('blocks since lock'),
        },
        y: {
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({
            maxTicksLimit: 5,
            callback: (v: number | string) => formatAlpha(Number(v)),
          }),
          title: axisTitle('conviction (α)'),
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
      caption={
        <>
          Same 500k α lock on a subnet hotkey. Perpetual: mass stays, conviction approaches
          mass. Decaying:{' '}
          <a
            href="/code/pallets/subtensor/src/staking/lock.rs#L371-L423"
            className="underline"
          >
            mass frees on UnlockRate
          </a>
          ; conviction peaks then falls.
        </>
      }
    >
      <div className="h-48">
        <Line data={data} options={options} plugins={[annotationPlugin]} />
      </div>

      <div className="mt-6 grid grid-cols-2 gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-3">
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

      <div className="mt-6 grid gap-x-8 gap-y-5 border-t border-line pt-4 pb-1 sm:grid-cols-2">
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
