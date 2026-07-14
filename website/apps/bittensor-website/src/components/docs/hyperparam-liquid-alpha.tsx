'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
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
  ACCENT_REGION,
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
// Chain floor for alpha_low/alpha_high: u16::MAX / 40 = 1638 ≈ 0.025.
const ALPHA_FLOOR = 0.025;

// Mirrors Pallet::alpha_sigmoid in pallets/subtensor/src/epoch/run_epoch.rs:
// sigmoid = 1 / (1 + e^(-(steepness/100) * (diff - 0.5))), alpha clamped to [low, high].
function alphaSigmoid(diff: number, low: number, high: number, steepness: number): number {
  const sigmoid = 1 / (1 + Math.exp((-steepness / 100) * (diff - 0.5)));
  const alpha = low + sigmoid * (high - low);
  return Math.min(Math.max(alpha, low), high);
}

const BASE_CAPTION =
  'Matches alpha_sigmoid in the epoch code. Deviation is weight − consensus when buying bond, bond − weight when selling. Higher alpha moves bonds faster. Requires yuma3_enabled; when liquid alpha is off, every pair uses the flat 1 − bonds_moving_avg / 1e6.';

const FOCUS_CAPTIONS: Record<string, string> = {
  liquid_alpha_enabled: `${BASE_CAPTION} The toggle below flips automatically — watch the sigmoid collapse to the flat line — until you take over.`,
  bonds_moving_avg:
    'With liquid alpha off, alpha_sigmoid never runs: every validator–miner pair smooths at the single flat rate 1 − bonds_moving_avg / 1,000,000, the solid line here. Drag the slider to move the line; re-enable liquid alpha to see the flat rate become the dashed fallback under the sigmoid.',
  alpha_low: `${BASE_CAPTION} The shaded band marks rates below alpha_low — the curve can never enter it, so alpha_low is the guaranteed floor for in-consensus pairs.`,
  alpha_high: `${BASE_CAPTION} The shaded band marks rates above alpha_high — the curve can never enter it, so alpha_high caps how fast even the most deviant pair's bonds move.`,
  alpha_sigmoid_steepness: `${BASE_CAPTION} The steepness slider sweeps on its own so you can watch the transition sharpen from a gentle ramp into a near step at deviation 0.5 — grab any control to stop it.`,
};

export function HyperparamLiquidAlpha({ focus }: { focus?: string }) {
  // For the bonds_moving_avg page, start with liquid alpha off so the flat rate is the chart.
  const [enabled, setEnabled] = useState(focus !== 'bonds_moving_avg');
  const [alphaLow, setAlphaLow] = useState(0.7);
  const [alphaHigh, setAlphaHigh] = useState(0.9);
  const [steepness, setSteepness] = useState(focus === 'alpha_sigmoid_steepness' ? 100 : 1000);
  const [bondsMovingAvg, setBondsMovingAvg] = useState(0.9);
  const [demoRunning, setDemoRunning] = useState(
    focus === 'liquid_alpha_enabled' || focus === 'alpha_sigmoid_steepness',
  );
  const stopDemo = () => setDemoRunning(false);

  const steepnessDir = useRef(1);
  useEffect(() => {
    if (!demoRunning) return;
    if (focus === 'liquid_alpha_enabled') {
      const id = setInterval(() => setEnabled((e) => !e), 2600);
      return () => clearInterval(id);
    }
    if (focus === 'alpha_sigmoid_steepness') {
      const id = setInterval(() => {
        setSteepness((s) => {
          let next = s + steepnessDir.current * 100;
          if (next >= 3000) {
            steepnessDir.current = -1;
            next = 3000;
          } else if (next <= 100) {
            steepnessDir.current = 1;
            next = 100;
          }
          return next;
        });
      }, 100);
      return () => clearInterval(id);
    }
    return undefined;
  }, [demoRunning, focus]);

  // Flat EMA rate used when liquid alpha is off: 1 - bonds_moving_avg / 1e6.
  const flatAlpha = 1 - bondsMovingAvg;

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ enabled, flatAlpha, alphaLow, alphaHigh, focus });
  drawState.current = { enabled, flatAlpha, alphaLow, alphaHigh, focus };

  // Direct in-plot labels (no legend): uppercase FiraCode annotations for the
  // curves, plus ACCENT text inside the unreachable region tints.
  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'liquidAlphaAnnotations',
      afterDatasetsDraw(chart) {
        const { enabled, flatAlpha, alphaLow, alphaHigh, focus } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        const mainMeta = chart.getDatasetMeta(0);
        const labelIdx = Math.round(0.68 * SAMPLE_POINTS);
        const mainPoint = mainMeta?.data?.[labelIdx];

        ctx.save();
        ctx.font = GRAPH_FONT;

        if (mainPoint) {
          ctx.fillStyle = INK;
          ctx.textAlign = 'left';
          ctx.fillText(
            enabled ? 'PER-PAIR EMA RATE' : 'FLAT EMA RATE',
            mainPoint.x + 4,
            mainPoint.y - 8,
          );
        }

        if (enabled) {
          ctx.fillStyle = INK_FAINT;
          ctx.textAlign = 'left';
          ctx.fillText(
            'FLAT RATE IF DISABLED',
            chartArea.left + 6,
            yScale.getPixelForValue(flatAlpha) - 6,
          );
        }

        const centerX = (chartArea.left + chartArea.right) / 2;
        if (enabled && focus === 'alpha_low') {
          ctx.fillStyle = ACCENT;
          ctx.textAlign = 'center';
          const midY = yScale.getPixelForValue(alphaLow / 2);
          ctx.fillText('BELOW ALPHA_LOW', centerX, midY - 3);
          ctx.fillText('UNREACHABLE', centerX, midY + 11);
        }
        if (enabled && focus === 'alpha_high') {
          ctx.fillStyle = ACCENT;
          ctx.textAlign = 'center';
          ctx.fillText(
            'ABOVE ALPHA_HIGH — UNREACHABLE',
            centerX,
            yScale.getPixelForValue((1 + alphaHigh) / 2) + 3,
          );
        }

        ctx.restore();
      },
    }),
    [],
  );

  // The chain forbids alpha_low > alpha_high, so the sliders drag each other.
  const changeLow = (v: number) => {
    stopDemo();
    setAlphaLow(v);
    if (v > alphaHigh) setAlphaHigh(v);
  };
  const changeHigh = (v: number) => {
    stopDemo();
    setAlphaHigh(v);
    if (v < alphaLow) setAlphaLow(v);
  };

  const curve = useMemo(() => {
    const xs = Array.from({ length: SAMPLE_POINTS + 1 }, (_, i) => i / SAMPLE_POINTS);
    const ys = xs.map((x) => (enabled ? alphaSigmoid(x, alphaLow, alphaHigh, steepness) : flatAlpha));
    return { xs, ys };
  }, [enabled, alphaLow, alphaHigh, steepness, flatAlpha]);

  const data = useMemo(() => {
    const emphasizeFlat = focus === 'bonds_moving_avg';
    const datasets = [
      {
        label: enabled ? 'per-pair EMA rate (liquid alpha)' : 'flat EMA rate (bonds_moving_avg)',
        data: curve.ys,
        borderColor: INK,
        backgroundColor: 'rgba(41, 41, 41, 0.03)',
        fill: true,
        tension: 0,
        pointRadius: 0,
        borderWidth: !enabled && emphasizeFlat ? 2.5 : 1.75,
      },
      ...(enabled
        ? [
            {
              label: 'flat rate if disabled',
              data: curve.xs.map(() => flatAlpha),
              borderColor: emphasizeFlat ? 'rgba(41, 41, 41, 0.7)' : INK_FAINT,
              borderDash: [4, 4],
              fill: false,
              tension: 0,
              pointRadius: 0,
              borderWidth: emphasizeFlat ? 2 : 1,
            },
          ]
        : []),
      // Blocked-region tint: the curve can never enter these bands.
      ...(enabled && focus === 'alpha_low'
        ? [
            {
              label: 'below alpha_low (unreachable)',
              data: curve.xs.map(() => alphaLow),
              borderColor: INK_FAINT,
              borderDash: [2, 3],
              backgroundColor: ACCENT_REGION,
              fill: 'start' as const,
              tension: 0,
              pointRadius: 0,
              borderWidth: 1,
            },
          ]
        : []),
      ...(enabled && focus === 'alpha_high'
        ? [
            {
              label: 'above alpha_high (unreachable)',
              data: curve.xs.map(() => alphaHigh),
              borderColor: INK_FAINT,
              borderDash: [2, 3],
              backgroundColor: ACCENT_REGION,
              fill: 'end' as const,
              tension: 0,
              pointRadius: 0,
              borderWidth: 1,
            },
          ]
        : []),
    ];
    return { labels: curve.xs.map((x) => x.toFixed(2)), datasets };
  }, [curve, enabled, flatAlpha, focus, alphaLow, alphaHigh]);

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
              return `deviation ${curve.xs[idx]?.toFixed(2)}`;
            },
            label: (ctx: { parsed: { y: number } }) => `alpha ${ctx.parsed.y.toFixed(3)}`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks(),
          title: axisTitle('deviation from consensus (combined_diff)'),
        },
        y: {
          min: 0,
          max: 1,
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          ticks: baseTicks({ maxTicksLimit: 5 }),
          title: axisTitle('bonds EMA rate (alpha)'),
        },
      },
    }),
    [curve.xs],
  );

  const focusClass = (name: string) => (focus === name ? 'border border-line bg-bg p-3' : '');

  const stats: { label: string; value: string; hint: string }[] =
    focus === 'bonds_moving_avg'
      ? [
          {
            label: 'Flat EMA rate',
            value: flatAlpha.toFixed(3),
            hint: `1 − ${Math.round(bondsMovingAvg * 1_000_000).toLocaleString()} / 1,000,000`,
          },
          {
            label: 'Bond kept per epoch',
            value: bondsMovingAvg.toFixed(3),
            hint: 'share of last epoch’s bond that survives',
          },
          {
            label: 'Epochs to close ~90% of a gap',
            value: flatAlpha > 0 ? Math.ceil(Math.log(0.1) / Math.log(1 - flatAlpha)).toString() : '∞',
            hint: 'how long conviction takes to pay off',
          },
        ]
      : [
          {
            label: 'In consensus (diff = 0)',
            value: curve.ys[0]?.toFixed(3) ?? '—',
            hint:
              focus === 'alpha_low'
                ? 'sits at the alpha_low floor'
                : 'EMA rate for weights matching consensus',
          },
          {
            label: 'Max deviation (diff = 1)',
            value: curve.ys[SAMPLE_POINTS]?.toFixed(3) ?? '—',
            hint:
              focus === 'alpha_high'
                ? 'approaches the alpha_high ceiling'
                : 'EMA rate at full deviation',
          },
          {
            label: 'Flat rate when disabled',
            value: flatAlpha.toFixed(3),
            hint: `1 − ${Math.round(bondsMovingAvg * 1_000_000).toLocaleString()} / 1,000,000`,
          },
        ];

  return (
    <ExplainerPanel
      title="Liquid alpha: per-weight bonds EMA rate"
      caption={(focus && FOCUS_CAPTIONS[focus]) || BASE_CAPTION}
    >
      <div className="h-64">
        <Line data={data} options={options} plugins={[annotationPlugin]} />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        {stats.map((s) => (
          <ExplainerStat key={s.label} label={s.label} value={s.value} hint={s.hint} />
        ))}
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <div className={focusClass('liquid_alpha_enabled')}>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => {
                stopDemo();
                setEnabled(e.target.checked);
              }}
              className="accent-[var(--bt-fg)]"
            />
            <span className="bt-label text-mute">liquid_alpha_enabled</span>
            <span className="font-mono text-xs">{enabled ? 'true' : 'false'}</span>
            {focus === 'liquid_alpha_enabled' && demoRunning && (
              <span className="text-[0.7rem] text-mute">(auto-toggling — click to take over)</span>
            )}
          </label>
        </div>
        <div className={focusClass('bonds_moving_avg')}>
          <ExplainerSlider
            label="bonds_moving_avg"
            value={bondsMovingAvg}
            min={0}
            max={0.995}
            step={0.005}
            display={`${Math.round(bondsMovingAvg * 1_000_000).toLocaleString()} (${bondsMovingAvg.toFixed(3)})`}
            onChange={(v) => {
              stopDemo();
              setBondsMovingAvg(v);
            }}
          />
        </div>
        <div className={focusClass('alpha_low')}>
          <ExplainerSlider
            label="alpha_low"
            value={alphaLow}
            min={ALPHA_FLOOR}
            max={1}
            step={0.005}
            display={`${Math.round(alphaLow * 65535).toLocaleString()} (${alphaLow.toFixed(3)})`}
            onChange={changeLow}
          />
        </div>
        <div className={focusClass('alpha_high')}>
          <ExplainerSlider
            label="alpha_high"
            value={alphaHigh}
            min={ALPHA_FLOOR}
            max={1}
            step={0.005}
            display={`${Math.round(alphaHigh * 65535).toLocaleString()} (${alphaHigh.toFixed(3)})`}
            onChange={changeHigh}
          />
        </div>
        <div className={focusClass('alpha_sigmoid_steepness')}>
          <ExplainerSlider
            label="alpha_sigmoid_steepness"
            value={steepness}
            min={-3000}
            max={3000}
            step={50}
            display={`${steepness} (slope ${(steepness / 100).toFixed(1)}; negative is root-only)${
              focus === 'alpha_sigmoid_steepness' && demoRunning ? ' — sweeping' : ''
            }`}
            onChange={(v) => {
              stopDemo();
              setSteepness(v);
            }}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
