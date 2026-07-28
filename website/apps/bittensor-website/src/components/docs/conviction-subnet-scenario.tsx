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
import { ExplainerPanel, ExplainerSlider, ExplainerStat, ExplainerToggle } from './explainer-panel';
import {
  MATURITY_RATE_BLOCKS,
  ONE_YEAR_BLOCKS,
  convictionOwnershipThreshold,
  formatAlpha,
  formatBlocks,
  formatPct,
  rollForwardLock,
} from '@/lib/emission-math';
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

/** Interpolated x-intervals where the total-conviction series is at or above the threshold. */
function aboveThresholdSegments(
  labels: number[],
  total: number[],
  threshold: number,
): {from: number; to: number}[] {
  const segments: {from: number; to: number}[] = [];
  let start: number | null = null;
  for (let i = 0; i < labels.length; i++) {
    const above = total[i] >= threshold;
    if (above && start === null) {
      if (i === 0) {
        start = labels[0];
      } else {
        const t = (threshold - total[i - 1]) / (total[i] - total[i - 1]);
        start = labels[i - 1] + t * (labels[i] - labels[i - 1]);
      }
    } else if (!above && start !== null) {
      const t = (threshold - total[i - 1]) / (total[i] - total[i - 1]);
      segments.push({from: start, to: labels[i - 1] + t * (labels[i] - labels[i - 1])});
      start = null;
    }
  }
  if (start !== null) segments.push({from: start, to: labels[labels.length - 1]});
  return segments;
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
    const segments = aboveThresholdSegments(labels, total, threshold);
    // The red dot marks where total conviction first crosses the gate
    // (the subnet is already past one year of age in this scenario).
    const crossing = segments.length > 0 ? segments[0].from : null;
    return {labels, alice, bob, carol, total, segments, crossing};
  }, [bobLock, carolLock, carolPerpetual, horizon, threshold]);

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

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({
    segments: chart.segments,
    threshold,
    horizon,
    bobLock,
    carolLock,
    carolPerpetual,
  });
  drawState.current = {
    segments: chart.segments,
    threshold,
    horizon,
    bobLock,
    carolLock,
    carolPerpetual,
  };

  // Region tint where the ownership gate holds, plus direct in-plot labels —
  // the treatment of the v431 release conviction graph.
  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'subnetScenarioAnnotations',
      beforeDatasetsDraw(chart) {
        const { segments } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        if (!xScale) return;

        ctx.save();
        ctx.font = GRAPH_FONT;

        for (const segment of segments) {
          const left = xScale.getPixelForValue(segment.from);
          const right = xScale.getPixelForValue(segment.to);
          ctx.fillStyle = ACCENT_REGION;
          ctx.fillRect(left, chartArea.top, right - left, chartArea.height);
          if (right - left > 90) {
            ctx.fillStyle = ACCENT;
            ctx.textAlign = 'center';
            // Keep the two-line label inside the plot when the region is
            // clipped by the right edge.
            const cx = Math.min((left + right) / 2, chartArea.right - 44);
            ctx.fillText('OWNERSHIP', cx, chartArea.top + 14);
            ctx.fillText('CONTESTABLE', cx, chartArea.top + 28);
          }
        }

        ctx.restore();
      },
      afterDatasetsDraw(chart) {
        const { threshold, horizon, bobLock, carolLock, carolPerpetual } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        ctx.save();
        ctx.font = GRAPH_FONT;
        ctx.textAlign = 'left';

        // Threshold label on the dashed gate line.
        ctx.fillStyle = ACCENT;
        ctx.fillText('10% THRESHOLD', chartArea.left + 6, yScale.getPixelForValue(threshold) - 6);

        // Direct series labels instead of a legend.
        const totalAt = (dt: number) =>
          rollForwardLock(EXAMPLE_SUBNET.owner.lockedMass, 0, dt, {perpetual: true, ownerLock: true}).conviction +
          rollForwardLock(bobLock, 0, dt, {perpetual: true}).conviction +
          rollForwardLock(carolLock, 0, dt, {perpetual: carolPerpetual}).conviction;

        // Below the curve: the total rises left-to-right, so the gap under
        // the label's anchor point only grows along the text.
        const xMain = horizon * 0.35;
        ctx.fillStyle = INK;
        ctx.fillText(
          'TOTAL CONVICTION',
          xScale.getPixelForValue(xMain) + 4,
          yScale.getPixelForValue(totalAt(xMain)) + 16,
        );

        const xSide = horizon * 0.68;
        const bobY = rollForwardLock(bobLock, 0, xSide, {perpetual: true}).conviction;
        const carolY = rollForwardLock(carolLock, 0, xSide, {perpetual: carolPerpetual}).conviction;
        ctx.fillStyle = INK_FAINT;
        ctx.fillText('BOB', xScale.getPixelForValue(xSide) + 4, yScale.getPixelForValue(bobY) - 6);
        ctx.fillText('CAROL', xScale.getPixelForValue(xSide) + 4, yScale.getPixelForValue(carolY) - 6);

        ctx.restore();
      },
    }),
    [],
  );

  const data = useMemo(
    () => ({
      datasets: [
        {
          label: 'Total conviction',
          data: chart.labels.map((dt, i) => ({x: dt, y: chart.total[i]})),
          borderColor: INK,
          borderWidth: 2,
          pointRadius: 0,
          tension: 0.3,
        },
        {
          label: 'Bob (validator)',
          data: chart.labels.map((dt, i) => ({x: dt, y: chart.bob[i]})),
          borderColor: 'rgba(41, 41, 41, 0.55)',
          borderWidth: 1,
          pointRadius: 0,
          tension: 0.3,
        },
        {
          label: 'Carol (staker)',
          data: chart.labels.map((dt, i) => ({x: dt, y: chart.carol[i]})),
          borderColor: INK_FAINT,
          borderWidth: 1,
          borderDash: [4, 3],
          pointRadius: 0,
          tension: 0.3,
        },
        {
          label: '10% threshold',
          data: [
            {x: 0, y: threshold},
            {x: horizon, y: threshold},
          ],
          borderColor: ACCENT,
          borderWidth: 1,
          borderDash: [6, 4],
          pointRadius: 0,
        },
        {
          label: 'Gate crossed',
          // Red dot exactly where total conviction crosses the threshold.
          data: chart.crossing === null ? [] : [{x: chart.crossing, y: threshold}],
          borderColor: ACCENT,
          backgroundColor: ACCENT,
          showLine: false,
          pointRadius: 4,
          pointStyle: 'circle' as const,
        },
      ],
    }),
    [chart, threshold, horizon],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      interaction: {mode: 'index' as const, intersect: false},
      plugins: {
        legend: {display: false},
        tooltip: {
          callbacks: {
            title: (items: {parsed: {x: number}}[]) =>
              `Block +${Math.round(items[0]?.parsed.x ?? 0).toLocaleString('en-US')}`,
            label: (ctx: {dataset: {label?: string}; parsed: {y: number}}) =>
              `${ctx.dataset.label}: ${formatAlpha(ctx.parsed.y)}`,
          },
        },
      },
      scales: {
        x: {
          type: 'linear' as const,
          min: 0,
          max: horizon,
          grid: {color: GRID},
          border: {color: AXIS_BORDER},
          ticks: baseTicks({
            callback: (v: number | string) => formatBlocks(Number(v)),
          }),
          title: axisTitle('blocks since locks started'),
        },
        y: {
          // Headroom above the tallest curve so the in-plot region label
          // drawn at the top of the chart stays clear of the series.
          min: 0,
          grace: '15%' as const,
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
    [horizon],
  );

  return (
    <ExplainerPanel
      title={`Example: Subnet ${EXAMPLE_SUBNET.netuid} (${EXAMPLE_SUBNET.name})`}
      caption={
        <>
          Fictional numbers for illustration. Three coldkeys lock toward different hotkeys;
          total conviction must reach 10% of SubnetAlphaOut before{' '}
          <a
            href="/code/pallets/subtensor/src/staking/lock.rs#L1160-L1377"
            className="underline"
          >
            ownership can transfer
          </a>
          .
        </>
      }
    >
      <div className="mb-6 grid grid-cols-2 gap-x-8 gap-y-4 border-b border-line pb-4 sm:grid-cols-4">
        <ExplainerStat label="SubnetAlphaOut" value={formatAlpha(EXAMPLE_SUBNET.alphaOut)} />
        <ExplainerStat label="10% threshold" value={formatAlpha(threshold)} />
        <ExplainerStat label="Subnet age" value={formatBlocks(EXAMPLE_SUBNET.ageBlocks)} />
        <ExplainerStat
          label="Ownership gate"
          value={ownershipReady ? 'Open' : 'Closed'}
          accent={ownershipReady}
        />
      </div>

      <div className="h-52">
        <Line data={data} options={options} plugins={[annotationPlugin]} />
      </div>

      <div className="mt-6 grid grid-cols-2 gap-x-8 gap-y-4 border-t border-line pt-4 lg:grid-cols-4">
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

      <div className="mt-6 border-t border-line pt-4 pb-1">
        <ExplainerToggle
          label="Carol's mode (set-perpetual-lock)"
          options={[
            { id: 'decaying', label: 'decaying' },
            { id: 'perpetual', label: 'perpetual' },
          ]}
          value={carolPerpetual ? 'perpetual' : 'decaying'}
          onChange={(id) => setCarolPerpetual(id === 'perpetual')}
        />
        <div className="mt-6 grid gap-x-8 gap-y-5 sm:grid-cols-3">
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
        </div>
      </div>
    </ExplainerPanel>
  );
}
