'use client';

import { useState } from 'react';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

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

  return (
    <ExplainerPanel
      title="Allowed childkey-take window"
      caption="The shaded band is the range set-childkey-take accepts: from the effective floor — max(global min_childkey_take, this subnet's floor) — up to the global max of 11796/65535 ≈ 18%. The marker is one childkey's stored take; when the floor rises past it, every read clamps the take up to the floor, and re-submitting the old value is rejected with InvalidChildkeyTake."
    >
      <div className="relative h-14 border border-line bg-bg">
        {/* allowed window */}
        <div
          className="absolute inset-y-0 transition-all duration-300"
          style={{
            left: `${pct(effectiveFloor)}%`,
            width: `${pct(GLOBAL_MAX - effectiveFloor)}%`,
            backgroundColor: 'rgba(41, 41, 41, 0.12)',
          }}
        />
        {/* floor edge */}
        <div
          className="absolute inset-y-0 w-[2px] -translate-x-1/2 bg-[rgb(41,41,41)] transition-all duration-300"
          style={{ left: `${pct(effectiveFloor)}%` }}
          title={`effective floor ${formatPct(effectiveFloor)}`}
        />
        {/* global max edge */}
        <div
          className="absolute inset-y-0 w-0 border-l-2 border-dashed"
          style={{ left: `${pct(GLOBAL_MAX)}%`, borderColor: 'rgba(41, 41, 41, 0.5)' }}
          title={`global max ${formatPct(GLOBAL_MAX)}`}
        />
        {/* stored take marker (hollow) */}
        <div
          className="absolute top-2 h-3 w-3 -translate-x-1/2 rounded-full border-2 border-[rgb(41,41,41)] bg-bg transition-all duration-300"
          style={{ left: `${pct(storedTake)}%` }}
          title={`stored take ${formatPct(storedTake)}`}
        />
        {/* effective take marker (filled), pushed up to the floor when below it */}
        <div
          className="absolute bottom-2 h-3 w-3 -translate-x-1/2 rounded-full bg-[rgb(41,41,41)] transition-all duration-300"
          style={{ left: `${pct(effectiveTake)}%` }}
          title={`effective take ${formatPct(effectiveTake)}`}
        />
      </div>
      <div className="relative mt-1 h-4 font-mono text-[0.625rem] text-mute">
        {[0, 0.05, 0.1, 0.15].map((tick) => (
          <span
            key={tick}
            className={tick === 0 ? 'absolute' : 'absolute -translate-x-1/2'}
            style={{ left: `${pct(tick)}%` }}
          >
            {Math.round(tick * 100)}%
          </span>
        ))}
        <span className="absolute -translate-x-1/2" style={{ left: `${pct(GLOBAL_MAX)}%` }}>
          max
        </span>
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
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
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-2">
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
    </ExplainerPanel>
  );
}
