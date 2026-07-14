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
import { MATURITY_RATE_BLOCKS, formatBlocks, formatPct, perpetualConviction } from '@/lib/emission-math';
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

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ lockedMass, startConviction, horizon });
  drawState.current = { lockedMass, startConviction, horizon };

  // Direct in-plot labels instead of a legend: uppercase FiraCode annotations
  // beside the conviction curve and the locked-mass reference line.
  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'convictionLockAnnotations',
      afterDatasetsDraw(chart) {
        const { lockedMass, startConviction, horizon } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        ctx.save();
        ctx.font = GRAPH_FONT;
        ctx.textAlign = 'left';

        // Locked-mass line label, tucked above the dashed line at the left.
        const yMass = yScale.getPixelForValue(lockedMass);
        ctx.fillStyle = INK_FAINT;
        ctx.fillText('LOCKED MASS (M)', chartArea.left + 6, yMass - 6);

        // Conviction curve label, below the curve where it has risen away
        // from the mass line's label zone.
        const xLabel = horizon * 0.45;
        const yCurve = yScale.getPixelForValue(
          perpetualConviction(lockedMass, startConviction, xLabel),
        );
        ctx.fillStyle = INK;
        ctx.fillText('CONVICTION', xScale.getPixelForValue(xLabel) + 4, yCurve + 16);

        ctx.restore();
      },
    }),
    [],
  );

  const data = useMemo(
    () => ({
      datasets: [
        {
          label: 'Conviction',
          data: points.map((p) => ({x: p.x, y: p.y})),
          borderColor: INK,
          borderWidth: 1.5,
          pointRadius: 0,
          tension: 0.2,
        },
        {
          label: 'Locked mass',
          data: points.map((p) => ({x: p.x, y: lockedMass})),
          borderColor: 'rgba(41, 41, 41, 0.5)',
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
          ticks: baseTicks({maxTicksLimit: 5}),
          title: axisTitle('conviction (α)'),
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
        <Line data={data} options={options} plugins={[annotationPlugin]} />
      </div>

      <div className="mt-6 grid grid-cols-2 gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-3">
        <ExplainerStat label="At 1τ (~130d scale)" value={formatPct(pctAtTau)} hint={`τ = ${MATURITY_RATE_BLOCKS.toLocaleString()} blocks`} />
        <ExplainerStat label="Locked mass (m)" value={`${lockedMass.toLocaleString()} α`} />
        <ExplainerStat label="Starting conviction (c₀)" value={`${startConviction.toLocaleString()} α`} />
      </div>

      <div className="mt-6 grid gap-x-8 gap-y-5 border-t border-line pt-4 pb-1 sm:grid-cols-2">
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
