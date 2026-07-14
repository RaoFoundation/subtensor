'use client';

import { useMemo, useRef, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Filler,
  Tooltip,
  Legend,
  type Plugin,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import {
  ACCENT,
  AXIS_BORDER,
  GRAPH_FONT,
  GRID,
  INK,
  INK_FAINT,
  axisTitle,
  baseTicks,
} from './chart-theme';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Filler, Tooltip, Legend);

const SAMPLE_POINTS = 100;
const KAPPA_U16_MAX = 65535;
const RHO_GHOSTS = [2, 40];

// Matches sigmoid_safe in pallets/subtensor/src/epoch/math.rs:
// 1 / (1 + exp(-rho * (input - kappa)))
function trustSigmoid(input: number, rho: number, kappa: number): number {
  return 1 / (1 + Math.exp(-rho * (input - kappa)));
}

function sigmoidPoints(rho: number, kappa: number): {x: number; y: number}[] {
  return Array.from({length: SAMPLE_POINTS + 1}, (_, i) => {
    const x = i / SAMPLE_POINTS;
    return {x, y: trustSigmoid(x, rho, kappa)};
  });
}

const CAPTIONS: Record<string, string> = {
  rho: 'rho is the temperature of the trust sigmoid, sigmoid_safe in epoch/math.rs: trust = 1 / (1 + e^(−rho × (x − kappa))). The dashed ghost curves fix rho at 2 and 40 — slide rho between them and watch the curve snap from a gentle ramp into a near-step at kappa.',
  kappa:
    'kappa is the midpoint of the trust sigmoid, sigmoid_safe in epoch/math.rs: trust = 1 / (1 + e^(−rho × (x − kappa))). The dashed marker is the majority threshold — alignment crossing kappa flips trust through 0.5, from mostly-distrusted to mostly-trusted. Slide kappa to move the crossing.',
};

const DEFAULT_CAPTION =
  'The classic Yuma trust curve, sigmoid_safe in epoch/math.rs: trust = 1 / (1 + e^(−rho × (x − kappa))). kappa is the midpoint, rho the steepness. The live epoch computes consensus as a kappa-weighted median; this sigmoid is the formulation rho parameterizes.';

export function HyperparamConsensusSigmoid({ focus }: { focus?: string }) {
  const [rho, setRho] = useState(10);
  const [kappa, setKappa] = useState(0.5);

  const label = (name: string, hint: string) =>
    (focus === name ? '▸ ' : '') + `${name} (${hint})`;

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ kappa, focus });
  drawState.current = { kappa, focus };

  // Direct in-plot labels replacing any legend: uppercase FiraCode annotations.
  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'sigmoidAnnotations',
      afterDatasetsDraw(chart) {
        const { kappa, focus } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        ctx.save();
        ctx.font = GRAPH_FONT;

        if (focus === 'rho') {
          // Label each ghost curve where the two steepnesses have separated.
          ctx.fillStyle = INK_FAINT;
          ctx.textAlign = 'left';
          const xGentle = 0.04;
          ctx.fillText(
            `RHO = ${RHO_GHOSTS[0]}`,
            xScale.getPixelForValue(xGentle) + 2,
            yScale.getPixelForValue(trustSigmoid(xGentle, RHO_GHOSTS[0], kappa)) - 8,
          );
          const xSteep = Math.min(kappa + 0.05, 0.9);
          ctx.fillText(
            `RHO = ${RHO_GHOSTS[1]}`,
            xScale.getPixelForValue(xSteep) + 4,
            yScale.getPixelForValue(trustSigmoid(xSteep, RHO_GHOSTS[1], kappa)) + 4,
          );
        }

        if (focus === 'kappa') {
          const xPx = xScale.getPixelForValue(kappa);
          const onLeft = kappa > 0.5;
          ctx.textAlign = onLeft ? 'right' : 'left';
          const dx = onLeft ? -6 : 6;
          ctx.fillStyle = INK;
          ctx.fillText(`KAPPA = ${kappa.toFixed(2)}`, xPx + dx, chartArea.bottom - 8);
          // The curve sits below 0.5 left of kappa and above it to the right,
          // so tuck the crossing label into whichever side is clear.
          ctx.fillStyle = ACCENT;
          ctx.fillText('TRUST 0.5', xPx + dx, yScale.getPixelForValue(0.5) + (onLeft ? -8 : 14));
        }

        ctx.restore();
      },
    }),
    [],
  );

  const datasets = useMemo(() => {
    const main = {
      label: 'Trust',
      data: sigmoidPoints(rho, kappa),
      borderColor: INK,
      backgroundColor: 'rgba(41, 41, 41, 0.03)',
      fill: true,
      tension: 0,
      pointRadius: 0,
      borderWidth: 1.75,
      order: 0,
    };

    if (focus === 'rho') {
      // Ghost curves bracketing the rho range make the steepness sweep visible.
      const ghosts = RHO_GHOSTS.map((ghostRho) => ({
        label: `rho = ${ghostRho}`,
        data: sigmoidPoints(ghostRho, kappa),
        borderColor: 'rgba(41, 41, 41, 0.25)',
        borderDash: [4, 4],
        borderWidth: 1,
        fill: false,
        tension: 0,
        pointRadius: 0,
        order: 1,
      }));
      return [main, ...ghosts];
    }

    if (focus === 'kappa') {
      // Vertical marker at the majority threshold plus the 0.5 crossing point.
      const threshold = {
        label: 'kappa threshold',
        data: [
          {x: kappa, y: 0},
          {x: kappa, y: 1},
        ],
        borderColor: INK_FAINT,
        borderDash: [4, 4],
        borderWidth: 1,
        fill: false,
        tension: 0,
        pointRadius: 0,
        order: 1,
      };
      // Highlight point: the trust flip through 0.5 is the moment kappa controls.
      const crossing = {
        label: 'majority crossing',
        data: [{x: kappa, y: 0.5}],
        borderColor: ACCENT,
        backgroundColor: ACCENT,
        showLine: false,
        pointRadius: 4,
        pointStyle: 'rectRot' as const,
        order: 2,
      };
      return [main, threshold, crossing];
    }

    return [main];
  }, [rho, kappa, focus]);

  const data = useMemo(() => ({datasets}), [datasets]);

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: {mode: 'nearest' as const, axis: 'x' as const, intersect: false},
      plugins: {
        legend: {display: false},
        tooltip: {
          callbacks: {
            title: (items: {parsed: {x: number}}[]) =>
              `Alignment ${(items[0]?.parsed.x ?? 0).toFixed(2)}`,
            label: (ctx: {parsed: {y: number}; dataset: {label?: string}}) => {
              const name = ctx.dataset.label ?? 'trust';
              if (name === 'kappa threshold') return `majority threshold at ${kappa.toFixed(2)}`;
              if (name === 'majority crossing') return 'trust flips through 0.5 here';
              const prefix = name.startsWith('rho') ? `${name}: ` : '';
              return `${prefix}trust ${ctx.parsed.y.toFixed(4)}`;
            },
          },
        },
      },
      scales: {
        x: {
          type: 'linear' as const,
          min: 0,
          max: 1,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks(),
          title: axisTitle('consensus alignment (stake fraction)'),
        },
        y: {
          min: 0,
          max: 1,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({maxTicksLimit: 5}),
          title: axisTitle('trust'),
        },
      },
    }),
    [kappa],
  );

  return (
    <ExplainerPanel
      title="rho / kappa trust sigmoid"
      caption={(focus && CAPTIONS[focus]) || DEFAULT_CAPTION}
    >
      <div className="h-52">
        <Line data={data} options={options} plugins={[annotationPlugin]} />
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
