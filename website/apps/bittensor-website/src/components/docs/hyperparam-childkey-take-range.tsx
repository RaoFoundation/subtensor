'use client';

import { useState } from 'react';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';
import { ACCENT, ACCENT_REGION, AXIS_BORDER, INK, INK_FAINT } from './chart-theme';

// Chain constants: PerU16 fractions where 65535 = 100%.
const GLOBAL_MAX = 11_796 / 65_535; // MaxChildkeyTake default, ≈18%
const GLOBAL_MIN = 0; // MinChildkeyTake default
const SCALE_MAX = 0.2; // axis runs 0–20% so the 18% cap sits inside the bar

function formatPct(fraction: number): string {
  return `${(fraction * 100).toFixed(1)}%`;
}

export function HyperparamChildkeyTakeRange() {
  const [subnetFloorPct, setSubnetFloorPct] = useState(0);
  const [storedTakePct, setStoredTakePct] = useState(3);

  const subnetFloor = subnetFloorPct / 100;
  const storedTake = storedTakePct / 100;
  // Mirrors get_effective_min_childkey_take: max(global, per-subnet).
  const effectiveFloor = Math.max(GLOBAL_MIN, subnetFloor);
  // Mirrors get_childkey_take: stored value clamps up to the effective floor.
  const effectiveTake = Math.max(storedTake, effectiveFloor);
  // Mirrors do_set_childkey_take's ensure: floor <= take <= max, else InvalidChildkeyTake.
  const setWouldPass = storedTake >= effectiveFloor && storedTake <= GLOBAL_MAX;

  const pct = (fraction: number) => (fraction / SCALE_MAX) * 100;
  const windowWide = (GLOBAL_MAX - effectiveFloor) / SCALE_MAX > 0.22;

  return (
    <ExplainerPanel
      title="Allowed childkey-take window"
      caption="The shaded band is the range set-childkey-take accepts: from the effective floor — max(global min_childkey_take, this subnet's floor) — up to the global max of 11796/65535 ≈ 18%. The marker is one childkey's stored take; when the floor rises past it, every read clamps the take up to the floor, and re-submitting the old value is rejected with InvalidChildkeyTake."
    >
      <div className="relative h-14">
        {/* rejected regions outside the window */}
        <div
          className="absolute inset-y-0 left-0 transition-all duration-300"
          style={{ width: `${pct(effectiveFloor)}%`, backgroundColor: ACCENT_REGION }}
        />
        <div
          className="absolute inset-y-0 right-0"
          style={{ width: `${100 - pct(GLOBAL_MAX)}%`, backgroundColor: ACCENT_REGION }}
        />
        {/* allowed window */}
        <div
          className="absolute inset-y-0 transition-all duration-300"
          style={{
            left: `${pct(effectiveFloor)}%`,
            width: `${pct(GLOBAL_MAX - effectiveFloor)}%`,
            backgroundColor: 'rgba(41, 41, 41, 0.06)',
          }}
        >
          {windowWide && (
            <span
              className="absolute left-1/2 top-1.5 -translate-x-1/2 font-mono text-[0.625rem] uppercase tracking-[0.08em]"
              style={{ color: INK_FAINT }}
            >
              allowed
            </span>
          )}
        </div>
        {/* floor edge */}
        <div
          className="absolute inset-y-0 w-[2px] -translate-x-1/2 transition-all duration-300"
          style={{ left: `${pct(effectiveFloor)}%`, backgroundColor: INK }}
          title={`effective floor ${formatPct(effectiveFloor)}`}
        />
        {/* global max edge */}
        <div
          className="absolute inset-y-0 w-0 border-l border-dashed"
          style={{ left: `${pct(GLOBAL_MAX)}%`, borderColor: 'rgba(41, 41, 41, 0.5)' }}
          title={`global max ${formatPct(GLOBAL_MAX)}`}
        />
        {/* stored take marker (hollow); rejected when outside the window */}
        <div
          className="absolute top-3 h-3 w-3 -translate-x-1/2 rounded-full border-2 bg-bg transition-all duration-300"
          style={{ left: `${pct(storedTake)}%`, borderColor: setWouldPass ? INK : ACCENT }}
          title={`stored take ${formatPct(storedTake)}`}
        />
        {/* effective take marker (filled), pushed up to the floor when below it */}
        <div
          className="absolute bottom-3 h-3 w-3 -translate-x-1/2 rounded-full transition-all duration-300"
          style={{ left: `${pct(effectiveTake)}%`, backgroundColor: INK }}
          title={`effective take ${formatPct(effectiveTake)}`}
        />
        {/* baseline */}
        <div className="absolute inset-x-0 bottom-0 border-b" style={{ borderColor: AXIS_BORDER }} />
      </div>
      <div className="relative mt-2 h-4 font-mono text-[0.625rem] text-mute">
        {[0, 0.05, 0.1, 0.15].map((tick) => (
          <span
            key={tick}
            className={tick === 0 ? 'absolute' : 'absolute -translate-x-1/2'}
            style={{ left: `${pct(tick)}%` }}
          >
            {Math.round(tick * 100)}%
          </span>
        ))}
        <span className="absolute -translate-x-1/2 uppercase tracking-[0.08em]" style={{ left: `${pct(GLOBAL_MAX)}%` }}>
          max
        </span>
      </div>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">
        <span>&#9675; stored take</span>
        <span>&#9679; take as read</span>
      </div>

      <div className="mt-8 border-t border-line pt-4">
        <div className="grid grid-cols-2 gap-x-8 gap-y-4 sm:grid-cols-3">
          <ExplainerStat
            label="Effective floor"
            value={formatPct(effectiveFloor)}
            hint="max(global min, subnet floor)"
          />
          <ExplainerStat
            label="Childkey take (as read)"
            value={formatPct(effectiveTake)}
            hint={
              effectiveTake > storedTake
                ? `Clamped up from stored ${formatPct(storedTake)}`
                : 'Stored value, above the floor'
            }
          />
          <ExplainerStat
            label="Re-setting stored value"
            value={setWouldPass ? 'accepted' : 'rejected'}
            hint={setWouldPass ? 'Inside the allowed window' : 'InvalidChildkeyTake'}
            accent={!setWouldPass}
          />
        </div>
      </div>

      <div className="mt-8 border-t border-line pt-4 pb-1">
        <div className="grid gap-x-8 gap-y-5 sm:grid-cols-2">
          <ExplainerSlider
            label="min_childkey_take (subnet floor)"
            value={subnetFloorPct}
            min={0}
            max={18}
            step={1}
            display={formatPct(subnetFloorPct / 100)}
            onChange={setSubnetFloorPct}
          />
          <ExplainerSlider
            label="example childkey's stored take"
            value={storedTakePct}
            min={0}
            max={18}
            step={1}
            display={formatPct(storedTakePct / 100)}
            onChange={setStoredTakePct}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
