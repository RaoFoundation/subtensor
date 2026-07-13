'use client';

import { useState } from 'react';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

const OWNER_CUT_FRACTION = 11_796 / 65_535; // SubnetOwnerCut default, ≈18%

function formatAlpha(value: number): string {
  return `${value.toFixed(1)} α`;
}

function ToggleRow({
  label,
  checked,
  onChange,
  highlighted,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  highlighted: boolean;
}) {
  return (
    <button
      type="button"
      onClick={() => onChange(!checked)}
      aria-pressed={checked}
      className={`flex w-full items-center justify-between gap-3 border bg-bg px-3 py-2 text-left ${
        highlighted ? 'border-[var(--bt-fg)]' : 'border-line'
      }`}
    >
      <span className="bt-label text-mute">{label}</span>
      <span className="font-mono text-xs">{checked ? 'true' : 'false'}</span>
    </button>
  );
}

function BarSegment({
  fraction,
  label,
  background,
}: {
  fraction: number;
  label: string;
  background: string;
}) {
  return (
    <div
      className="flex items-center justify-center overflow-hidden whitespace-nowrap transition-all duration-500"
      style={{width: `${fraction * 100}%`, background}}
      title={label}
    >
      {fraction > 0.08 && (
        <span className="px-1 font-mono text-[0.625rem] text-white mix-blend-difference">
          {label}
        </span>
      )}
    </div>
  );
}

export function HyperparamOwnerCut({ focus }: { focus?: string }) {
  const [tempoAlpha, setTempoAlpha] = useState(360);
  const [ownerCutEnabled, setOwnerCutEnabled] = useState(true);
  const [autoLockEnabled, setAutoLockEnabled] = useState(false);

  const ownerCut = ownerCutEnabled ? tempoAlpha * OWNER_CUT_FRACTION : 0;
  const ownerLocked = autoLockEnabled ? ownerCut : 0;
  const ownerLiquid = ownerCut - ownerLocked;
  const remainder = tempoAlpha - ownerCut;
  const miners = remainder / 2;
  const validators = remainder / 2;

  const total = tempoAlpha > 0 ? tempoAlpha : 1;

  return (
    <ExplainerPanel
      title="One tempo's alpha emission split"
      caption="alpha_out accrued over one tempo, divided at the epoch. Owner cut is 18% (SubnetOwnerCut) when owner_cut_enabled; the remainder splits 50/50 between miners and validators + stakers (root proportion omitted for clarity)."
    >
      <div className="flex h-10 w-full border border-line">
        {ownerLiquid > 0 && (
          <BarSegment
            fraction={ownerLiquid / total}
            label={`owner ${formatAlpha(ownerLiquid)}`}
            background="rgb(41, 41, 41)"
          />
        )}
        {ownerLocked > 0 && (
          <BarSegment
            fraction={ownerLocked / total}
            label={`owner (locked) ${formatAlpha(ownerLocked)}`}
            background="repeating-linear-gradient(45deg, rgb(41, 41, 41), rgb(41, 41, 41) 4px, rgb(90, 90, 90) 4px, rgb(90, 90, 90) 8px)"
          />
        )}
        <BarSegment
          fraction={miners / total}
          label={`miners ${formatAlpha(miners)}`}
          background="rgba(41, 41, 41, 0.45)"
        />
        <BarSegment
          fraction={validators / total}
          label={`validators + stakers ${formatAlpha(validators)}`}
          background="rgba(41, 41, 41, 0.18)"
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Owner cut"
          value={formatAlpha(ownerCut)}
          hint={
            ownerCut === 0
              ? 'Disabled — redistributed to participants'
              : autoLockEnabled
                ? 'Staked to owner, then conviction-locked'
                : 'Staked to owner hotkey, liquid'
          }
        />
        <ExplainerStat label="Miners" value={formatAlpha(miners)} hint="50% of the remainder" />
        <ExplainerStat
          label="Validators + stakers"
          value={formatAlpha(validators)}
          hint="50% of the remainder"
        />
      </div>

      <div className="mt-5 grid gap-3 sm:grid-cols-2">
        <ToggleRow
          label="owner_cut_enabled"
          checked={ownerCutEnabled}
          onChange={setOwnerCutEnabled}
          highlighted={focus === 'owner_cut_enabled'}
        />
        <ToggleRow
          label="owner_cut_auto_lock_enabled"
          checked={autoLockEnabled}
          onChange={setAutoLockEnabled}
          highlighted={focus === 'owner_cut_auto_lock_enabled'}
        />
      </div>

      <div className="mt-5">
        <ExplainerSlider
          label="alpha_out this tempo"
          value={tempoAlpha}
          min={0}
          max={720}
          step={10}
          display={formatAlpha(tempoAlpha)}
          onChange={setTempoAlpha}
        />
      </div>
    </ExplainerPanel>
  );
}
