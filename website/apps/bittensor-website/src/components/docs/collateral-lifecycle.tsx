'use client';

import { useMemo, useRef, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Tooltip,
  type Plugin,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { ExplainerPanel, ExplainerSlider, ExplainerStat, ExplainerToggle } from './explainer-panel';
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

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Tooltip);

/** Illustrative registration price, in alpha at a 1:1 alpha price. */
const PRICE = 10;
const TEMPOS = 60;

type Scenario = 'honest' | 'blacklisted';

type SimInput = {
  bond: number;
  drainRatio: number;
  incentive: number;
  floor: number;
  addAmount: number;
  addTempo: number;
  banTempo: number | null; // null = never
};

/**
 * One run of the collateral state machine:
 * - `add_collateral` lands `addAmount` at `addTempo`;
 * - above the floor, each scored tempo releases k × emission;
 * - below the floor, emission is captured into the lock until the floor fills;
 * - a blacklisted hotkey (t >= banTempo) earns nothing: no release, no capture.
 */
function simulate({ bond, drainRatio, incentive, floor, addAmount, addTempo, banTempo }: SimInput) {
  const locked: number[] = [];
  const released: number[] = [];
  let remaining = bond;
  let cumReleased = 0;
  let cumCaptured = 0;
  for (let t = 0; t <= TEMPOS; t += 1) {
    if (addAmount > 0 && t === addTempo) {
      remaining += addAmount;
    }
    const scoring = banTempo === null || t < banTempo;
    if (t > 0 && scoring) {
      if (remaining < floor) {
        const captured = Math.min(incentive, floor - remaining);
        remaining += captured;
        cumCaptured += captured;
      } else {
        const release = Math.min(drainRatio * incentive, remaining - floor);
        remaining -= release;
        cumReleased += release;
      }
    }
    locked.push(remaining);
    released.push(cumReleased);
  }
  return { locked, released, cumCaptured };
}

export function CollateralLifecycle() {
  const [lockShare, setLockShare] = useState(80); // p, percent
  const [drainRatio, setDrainRatio] = useState(0.5); // k
  const [incentive, setIncentive] = useState(0.2); // alpha per tempo
  const [floor, setFloor] = useState(0); // set_min_collateral, alpha
  const [addAmount, setAddAmount] = useState(0); // add_collateral, alpha
  const [addTempo, setAddTempo] = useState(15);
  const [scenario, setScenario] = useState<Scenario>('honest');
  const [banTempo, setBanTempo] = useState(15);

  const bond = (lockShare / 100) * PRICE;
  const burned = PRICE - bond;

  const sim = useMemo(
    () =>
      simulate({
        bond,
        drainRatio,
        incentive,
        floor,
        addAmount,
        addTempo,
        banTempo: scenario === 'blacklisted' ? banTempo : null,
      }),
    [bond, drainRatio, incentive, floor, addAmount, addTempo, scenario, banTempo],
  );

  // Honest-run projection for the "drained to floor after" stat, so the
  // figure stays meaningful while the blacklist scenario is toggled on.
  const honest = useMemo(
    () =>
      simulate({
        bond,
        drainRatio,
        incentive,
        floor,
        addAmount,
        addTempo,
        banTempo: null,
      }),
    [bond, drainRatio, incentive, floor, addAmount, addTempo],
  );

  const drainTarget = Math.min(floor, bond + addAmount);
  const drainSearchFrom = addAmount > 0 ? addTempo : 0;
  const drainedAtTempo = honest.locked.findIndex(
    (value, index) => index >= drainSearchFrom && value - drainTarget < 0.005,
  );

  // The plugin is registered once at chart creation, so it reads live values
  // through a ref instead of closing over state that would go stale.
  const drawState = useRef({ sim, scenario, banTempo, floor, addAmount, addTempo });
  drawState.current = { sim, scenario, banTempo, floor, addAmount, addTempo };

  const annotationPlugin = useMemo<Plugin<'line'>>(
    () => ({
      id: 'collateralLifecycleAnnotations',
      afterDatasetsDraw(chart) {
        const { sim, scenario, banTempo, floor, addAmount, addTempo } = drawState.current;
        const { ctx, chartArea, scales } = chart;
        const xScale = scales.x;
        const yScale = scales.y;
        if (!xScale || !yScale) return;

        ctx.save();
        ctx.font = GRAPH_FONT;

        // The set_min_collateral floor: a dashed line the lock parks on.
        if (floor > 0) {
          const floorY = yScale.getPixelForValue(floor);
          ctx.strokeStyle = INK_FAINT;
          ctx.setLineDash([4, 4]);
          ctx.beginPath();
          ctx.moveTo(chartArea.left, floorY);
          ctx.lineTo(chartArea.right, floorY);
          ctx.stroke();
          ctx.setLineDash([]);
          ctx.fillStyle = INK_FAINT;
          ctx.textAlign = 'right';
          ctx.fillText('SET_MIN_COLLATERAL FLOOR', chartArea.right - 4, floorY - 5);
        }

        // The add_collateral event: mark the jump.
        if (addAmount > 0) {
          const addX = xScale.getPixelForValue(addTempo);
          const addY = yScale.getPixelForValue(sim.locked[addTempo] ?? 0);
          ctx.fillStyle = INK;
          ctx.beginPath();
          ctx.arc(addX, addY, 3.5, 0, Math.PI * 2);
          ctx.fill();
          ctx.textAlign = addX > chartArea.right - 150 ? 'right' : 'left';
          ctx.fillText(
            `ADD_COLLATERAL +${addAmount.toFixed(1)}α`,
            addX + (addX > chartArea.right - 150 ? -8 : 8),
            addY - 8,
          );
        }

        // Tint the stranded region and mark the blacklist event.
        if (scenario === 'blacklisted') {
          const banX = xScale.getPixelForValue(banTempo);
          ctx.fillStyle = ACCENT_REGION;
          ctx.fillRect(banX, chartArea.top, chartArea.right - banX, chartArea.bottom - chartArea.top);
          ctx.strokeStyle = ACCENT;
          ctx.setLineDash([3, 3]);
          ctx.beginPath();
          ctx.moveTo(banX, chartArea.top);
          ctx.lineTo(banX, chartArea.bottom);
          ctx.stroke();
          ctx.setLineDash([]);
          ctx.fillStyle = ACCENT;
          const align = banX > chartArea.right - 150 ? 'right' : 'left';
          ctx.textAlign = align;
          ctx.fillText(
            'VALIDATORS STOP SCORING',
            banX + (align === 'left' ? 6 : -6),
            chartArea.top + 12,
          );
        }

        // Direct series labels replacing the legend.
        const lockedIdx = Math.floor(sim.locked.length * 0.06);
        ctx.textAlign = 'left';
        ctx.fillStyle = INK;
        ctx.fillText(
          'LOCKED COLLATERAL',
          xScale.getPixelForValue(lockedIdx) + 6,
          yScale.getPixelForValue(sim.locked[lockedIdx] ?? 0) - 8,
        );
        const releasedIdx = Math.floor(sim.released.length * 0.35);
        ctx.fillStyle = INK_FAINT;
        ctx.fillText(
          'RELEASED TO FREE STAKE',
          xScale.getPixelForValue(releasedIdx) + 6,
          yScale.getPixelForValue(sim.released[releasedIdx] ?? 0) + 14,
        );

        ctx.restore();
      },
    }),
    [],
  );

  const data = useMemo(
    () => ({
      labels: sim.locked.map((_, i) => String(i)),
      datasets: [
        {
          label: 'Locked collateral',
          data: sim.locked,
          borderColor: INK,
          pointRadius: 0,
          borderWidth: 1.5,
          tension: 0.1,
        },
        {
          label: 'Released to free stake',
          data: sim.released,
          borderColor: INK_FAINT,
          pointRadius: 0,
          borderWidth: 1.5,
          tension: 0.1,
        },
      ],
    }),
    [sim],
  );

  const options = useMemo(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      plugins: {
        legend: { display: false },
        tooltip: {
          callbacks: {
            label: (ctx: { dataset: { label?: string }; parsed: { y: number } }) =>
              `${ctx.dataset.label}: ${ctx.parsed.y.toFixed(2)} α`,
          },
        },
      },
      scales: {
        x: {
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          title: axisTitle('Tempo'),
          ticks: baseTicks(),
        },
        y: {
          grid: { color: GRID },
          border: { color: AXIS_BORDER },
          title: axisTitle('Alpha'),
          ticks: baseTicks({ maxTicksLimit: 5 }),
        },
      },
    }),
    [],
  );

  const stranded = sim.locked[sim.locked.length - 1] ?? 0;
  const floorHolds = floor > 0;
  const drainedStat = drainedAtTempo >= 0 ? `${drainedAtTempo} tempos` : 'beyond window';

  return (
    <ExplainerPanel
      title="Miner collateral over a mining career"
      tag={`price ${PRICE} α`}
      caption={
        'Registration splits the price: (1 − p) burned, p locked to the hotkey. Each scored ' +
        'tempo releases k × emission above the floor; add_collateral tops the lock up ' +
        'mid-career, and set_min_collateral parks it at a floor that earned emission ' +
        'refills. If validators blacklist the hotkey (off chain, by not scoring it), ' +
        'emission stops and the remainder strands.'
      }
    >
      <div className="h-44">
        <Line data={data} options={options} plugins={[annotationPlugin]} />
      </div>

      <div className="mt-4 grid gap-3 sm:grid-cols-3">
        <ExplainerStat
          label="At registration"
          value={`${burned.toFixed(1)} α burned + ${bond.toFixed(1)} α locked`}
          hint={`p = ${lockShare}% of a ${PRICE} α price${
            addAmount > 0 ? ` (+${addAmount.toFixed(1)} α added at tempo ${addTempo})` : ''
          }`}
        />
        <ExplainerStat
          label={floorHolds ? 'Drained to floor after' : 'Fully released after'}
          value={drainedStat}
          hint={
            floorHolds
              ? `the floor holds ${drainTarget.toFixed(1)} α parked; only headroom drains`
              : `bond ÷ (k × emission) at k = ${drainRatio}`
          }
        />
        <ExplainerStat
          label={scenario === 'blacklisted' ? 'Stranded by blacklist' : 'Still locked at window end'}
          value={`${stranded.toFixed(2)} α`}
          hint={
            scenario === 'blacklisted'
              ? sim.cumCaptured > 0
                ? `frozen until validators resume scoring (${sim.cumCaptured.toFixed(2)} α was captured into the lock)`
                : 'frozen until validators resume scoring'
              : floorHolds && Math.abs(stranded - drainTarget) < 0.01
                ? 'parked at the set_min_collateral floor'
                : 'keeps draining as incentive is earned'
          }
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <ExplainerSlider
          label="collateral_lock_share (p)"
          value={lockShare}
          min={0}
          max={95}
          step={5}
          display={`${lockShare}%`}
          onChange={setLockShare}
        />
        <ExplainerSlider
          label="collateral_drain_ratio (k)"
          value={drainRatio}
          min={0.1}
          max={2}
          step={0.1}
          display={`${drainRatio.toFixed(1)} α per α earned`}
          onChange={setDrainRatio}
        />
        <ExplainerSlider
          label="Miner incentive per tempo"
          value={incentive}
          min={0.05}
          max={0.6}
          step={0.05}
          display={`${incentive.toFixed(2)} α`}
          onChange={setIncentive}
        />
        <ExplainerSlider
          label="set_min_collateral (floor)"
          value={floor}
          min={0}
          max={12}
          step={0.5}
          display={floor > 0 ? `${floor.toFixed(1)} α` : 'no floor'}
          onChange={setFloor}
        />
        <ExplainerSlider
          label="add_collateral (top-up)"
          value={addAmount}
          min={0}
          max={10}
          step={0.5}
          display={addAmount > 0 ? `+${addAmount.toFixed(1)} α` : 'none'}
          onChange={setAddAmount}
        />
        {addAmount > 0 && (
          <ExplainerSlider
            label="Top-up lands at tempo"
            value={addTempo}
            min={1}
            max={TEMPOS - 5}
            step={1}
            display={`tempo ${addTempo}`}
            onChange={setAddTempo}
          />
        )}
        <div className="flex flex-col gap-3">
          <ExplainerToggle
            label="Scenario"
            options={[
              { id: 'honest', label: 'Honest miner' },
              { id: 'blacklisted', label: 'Blacklisted', accent: true },
            ]}
            value={scenario}
            onChange={setScenario}
          />
          {scenario === 'blacklisted' && (
            <ExplainerSlider
              label="Blacklisted at tempo"
              value={banTempo}
              min={5}
              max={TEMPOS - 5}
              step={5}
              display={`tempo ${banTempo}`}
              onChange={setBanTempo}
            />
          )}
        </div>
      </div>
    </ExplainerPanel>
  );
}
