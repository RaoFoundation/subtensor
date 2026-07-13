'use client';

import { useMemo, useState } from 'react';
import { ExplainerPanel, ExplainerSlider, ExplainerStat } from './explainer-panel';

// Chain default is 10 (MinNonImmuneUids); scaled down to suit the small demo grid.
const MIN_NON_IMMUNE = 2;
const INITIAL_MAX_UIDS = 32;
const DEFAULT_IMMUNITY = 6;
const DEFAULT_OWNER_LIMIT = 1;

type Slot = {
  id: number;
  registeredAt: number;
  emission: number;
  isOwner: boolean;
};

type SimEvent =
  | { kind: 'append'; uid: number }
  | { kind: 'replace'; uid: number }
  | { kind: 'trim'; removed: number }
  | { kind: 'fail' };

type Sim = {
  tick: number;
  nextId: number;
  slots: Slot[];
  lastEvent: SimEvent | null;
};

// Deterministic per-neuron earning rate so server and client render identically.
function rateOf(id: number): number {
  const hash = (Math.imul(id + 1, 2654435761) >>> 0) % 887;
  return 0.1 + (hash / 887) * 0.9;
}

function makeInitialSim(): Sim {
  const count = INITIAL_MAX_UIDS;
  const tick = count + 2;
  const slots: Slot[] = Array.from({ length: count }, (_, i) => ({
    id: i,
    registeredAt: i,
    emission: Math.round(rateOf(i) * (tick - i) * 10) / 10,
    // Two owner hotkeys so the owner_immune_neuron_limit slider visibly matters.
    isOwner: i === 0 || i === 5,
  }));
  return { tick, nextId: count, slots, lastEvent: null };
}

// Mirrors get_immune_owner_tuples: owner hotkeys sorted by registration block
// (earliest first), truncated to the limit.
function ownerImmuneUids(slots: Slot[], limit: number): Set<number> {
  const owners = slots
    .map((slot, uid) => ({ uid, slot }))
    .filter(({ slot }) => slot.isOwner)
    .sort((a, b) => a.slot.registeredAt - b.slot.registeredAt || a.uid - b.uid);
  return new Set(owners.slice(0, limit).map(({ uid }) => uid));
}

type Candidate = { uid: number; emission: number; registeredAt: number };

function beats(a: Candidate, b: Candidate | null): boolean {
  if (b === null) return true;
  if (a.emission !== b.emission) return a.emission < b.emission;
  if (a.registeredAt !== b.registeredAt) return a.registeredAt < b.registeredAt;
  return a.uid < b.uid;
}

// Mirrors get_neuron_to_prune: lowest emission wins, ties broken by older
// registration then lower uid; owner-immune uids are skipped entirely; falls
// back to immune uids when few non-immune uids remain.
function pruneTarget(
  slots: Slot[],
  tick: number,
  immunityPeriod: number,
  ownerImmune: Set<number>,
): number | null {
  let bestFree: Candidate | null = null;
  let bestImmune: Candidate | null = null;
  let freeCount = 0;
  slots.forEach((slot, uid) => {
    if (ownerImmune.has(uid)) return;
    const candidate: Candidate = { uid, emission: slot.emission, registeredAt: slot.registeredAt };
    if (tick - slot.registeredAt < immunityPeriod) {
      if (beats(candidate, bestImmune)) bestImmune = candidate;
    } else {
      freeCount += 1;
      if (beats(candidate, bestFree)) bestFree = candidate;
    }
  });
  if (freeCount > MIN_NON_IMMUNE && bestFree !== null) return (bestFree as Candidate).uid;
  return bestImmune !== null ? (bestImmune as Candidate).uid : null;
}

function focusCaption(focus: string | undefined): string {
  switch (focus) {
    case 'immunity_period':
      return ' Focused on immunity_period: each immune slot shows how many registrations remain before its protection expires and it joins the prunable pool.';
    case 'max_allowed_uids':
      return ' Focused on max_allowed_uids: the grid itself is the capacity — empty slots (+) absorb registrations for free, and once the boundary is hit every entrant must evict someone.';
    case 'owner_immune_neuron_limit':
      return ' Focused on owner_immune_neuron_limit: O marks the owner hotkeys inside the limit (never pruned, oldest registrations first); o marks owner hotkeys beyond it, which compete like everyone else.';
    default:
      return '';
  }
}

function eventMessage(event: SimEvent | null): string {
  if (event === null) return 'Press a register button to add a neuron.';
  switch (event.kind) {
    case 'append':
      return `Appended as uid ${event.uid} — the subnet was not yet full.`;
    case 'replace':
      return `Subnet full — pruned uid ${event.uid} (lowest emission among prunable uids) and reused its slot.`;
    case 'trim':
      return `Trimmed ${event.removed} lowest-emission non-immune neuron${event.removed === 1 ? '' : 's'} (like btcli sudo trim).`;
    case 'fail':
      return 'Registration failed: every candidate is immune (NoNeuronIdAvailable).';
    default: {
      const exhaustive: never = event;
      return exhaustive;
    }
  }
}

export function HyperparamUidLifecycle({ focus }: { focus?: string }) {
  const [maxUids, setMaxUids] = useState(INITIAL_MAX_UIDS);
  const [immunityPeriod, setImmunityPeriod] = useState(DEFAULT_IMMUNITY);
  const [ownerLimit, setOwnerLimit] = useState(DEFAULT_OWNER_LIMIT);
  const [sim, setSim] = useState<Sim>(makeInitialSim);

  const register = (asOwner: boolean) => {
    setSim((prev) => {
      const tick = prev.tick + 1;
      // Every incumbent accrues emission at its own rate before the new
      // registration is processed, so weak neurons drift to the bottom.
      const grown = prev.slots.map((slot) => ({
        ...slot,
        emission: Math.round((slot.emission + rateOf(slot.id)) * 10) / 10,
      }));
      const entrant: Slot = { id: prev.nextId, registeredAt: tick, emission: 0, isOwner: asOwner };
      if (grown.length < maxUids) {
        return {
          tick,
          nextId: prev.nextId + 1,
          slots: [...grown, entrant],
          lastEvent: { kind: 'append', uid: grown.length },
        };
      }
      const target = pruneTarget(grown, tick, immunityPeriod, ownerImmuneUids(grown, ownerLimit));
      if (target === null) {
        return { ...prev, tick, slots: grown, lastEvent: { kind: 'fail' } };
      }
      return {
        tick,
        nextId: prev.nextId + 1,
        slots: grown.map((slot, uid) => (uid === target ? entrant : slot)),
        lastEvent: { kind: 'replace', uid: target },
      };
    });
  };

  // Lowering capacity below the filled count trims the lowest emitters while
  // preserving immune uids, mirroring trim_to_max_allowed_uids.
  const changeMaxUids = (value: number) => {
    setMaxUids(value);
    setSim((prev) => {
      if (prev.slots.length <= value) return prev;
      const ownerSet = ownerImmuneUids(prev.slots, ownerLimit);
      const removed = prev.slots
        .map((slot, uid) => ({ uid, slot }))
        .filter(({ uid, slot }) => !ownerSet.has(uid) && prev.tick - slot.registeredAt >= immunityPeriod)
        .sort((a, b) => a.slot.emission - b.slot.emission)
        .slice(0, prev.slots.length - value)
        .map(({ uid }) => uid);
      if (removed.length === 0) return prev;
      const removeSet = new Set(removed);
      return {
        ...prev,
        slots: prev.slots.filter((_, uid) => !removeSet.has(uid)),
        lastEvent: { kind: 'trim', removed: removed.length },
      };
    });
  };

  const reset = () => {
    setSim(makeInitialSim());
    setMaxUids(INITIAL_MAX_UIDS);
    setImmunityPeriod(DEFAULT_IMMUNITY);
    setOwnerLimit(DEFAULT_OWNER_LIMIT);
  };

  const ownerImmune = useMemo(
    () => ownerImmuneUids(sim.slots, ownerLimit),
    [sim.slots, ownerLimit],
  );

  // Exact preview of what the next registration would prune.
  const nextPrune = useMemo(() => {
    if (sim.slots.length < maxUids) return null;
    const tick = sim.tick + 1;
    const grown = sim.slots.map((slot) => ({ ...slot, emission: slot.emission + rateOf(slot.id) }));
    return pruneTarget(grown, tick, immunityPeriod, ownerImmuneUids(grown, ownerLimit));
  }, [sim, maxUids, immunityPeriod, ownerLimit]);

  const newImmuneCount = sim.slots.filter(
    (slot, uid) => !ownerImmune.has(uid) && sim.tick - slot.registeredAt < immunityPeriod,
  ).length;
  const maxEmission = Math.max(...sim.slots.map((slot) => slot.emission), 0.001);
  const lastTouched =
    sim.lastEvent && (sim.lastEvent.kind === 'append' || sim.lastEvent.kind === 'replace')
      ? sim.lastEvent.uid
      : null;

  const focusClass = (name: string) =>
    focus === name ? 'border-l-2 border-[var(--bt-fg)] pl-3' : '';

  const buttonClass =
    'border border-line bg-bg px-3 py-1.5 font-mono text-xs hover:bg-panel';

  return (
    <ExplainerPanel
      title="UID lifecycle playground"
      caption={`A subnet's slot grid, shaded by emission. When it is full, registering prunes the lowest-emission prunable uid. I = inside immunity_period, O = owner-immune, dashed outline = next prune target. Pruning falls back onto immune uids when ${MIN_NON_IMMUNE} or fewer non-immune uids remain (chain default: 10).${focusCaption(focus)}`}
    >
      <div
        className={
          'grid grid-cols-8 gap-1' +
          (focus === 'max_allowed_uids' && sim.slots.length >= maxUids
            ? ' outline outline-1 outline-offset-2 outline-[var(--bt-fg)]'
            : '')
        }
      >
        {Array.from({ length: maxUids }, (_, uid) => {
          const slot = sim.slots[uid];
          if (!slot) {
            return (
              <div
                key={uid}
                className="flex h-9 items-center justify-center border border-dashed border-line font-mono text-[0.625rem] text-mute"
              >
                {focus === 'max_allowed_uids' ? '+' : ''}
              </div>
            );
          }
          const isOwnerImmune = ownerImmune.has(uid);
          const isNewImmune = !isOwnerImmune && sim.tick - slot.registeredAt < immunityPeriod;
          const immunityLeft = isNewImmune ? immunityPeriod - (sim.tick - slot.registeredAt) : 0;
          const isOwnerBeyondLimit = slot.isOwner && !isOwnerImmune;
          const shade = slot.emission / maxEmission;
          const status = isOwnerImmune
            ? ', owner-immune'
            : isNewImmune
              ? `, immune (${immunityLeft} reg${immunityLeft === 1 ? '' : 's'} left)`
              : '';
          // Focus-specific badge: immunity focus counts down the remaining
          // protection; owner focus also marks owner hotkeys beyond the limit.
          const badge =
            focus === 'owner_immune_neuron_limit' && isOwnerBeyondLimit
              ? 'o'
              : isOwnerImmune
                ? 'O'
                : isNewImmune
                  ? focus === 'immunity_period'
                    ? `I${immunityLeft}`
                    : 'I'
                  : null;
          return (
            <div
              key={uid}
              className={
                'relative flex h-9 items-center justify-center border font-mono text-[0.625rem] ' +
                (uid === nextPrune ? 'border-dashed border-[var(--bt-fg)] ' : 'border-line ') +
                (uid === lastTouched ? 'ring-1 ring-[var(--bt-fg)]' : '')
              }
              style={{ backgroundColor: `rgba(41, 41, 41, ${0.04 + shade * 0.56})` }}
              title={`uid ${uid} — emission ${slot.emission.toFixed(1)}${status}`}
            >
              <span className={shade > 0.55 ? 'text-white' : ''}>{uid}</span>
              {badge !== null && (
                <span
                  className={
                    'absolute right-0.5 top-0 text-[0.5rem] ' + (shade > 0.55 ? 'text-white' : '')
                  }
                >
                  {badge}
                </span>
              )}
            </div>
          );
        })}
      </div>

      <p className="mt-3 font-mono text-xs text-mute">{eventMessage(sim.lastEvent)}</p>

      <div className="mt-3 flex flex-wrap gap-2">
        <button type="button" onClick={() => register(false)} className={buttonClass}>
          Register miner
        </button>
        <button type="button" onClick={() => register(true)} className={buttonClass}>
          Register owner hotkey
        </button>
        <button type="button" onClick={reset} className={buttonClass + ' text-mute'}>
          Reset
        </button>
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-4">
        <ExplainerStat label="Filled / capacity" value={`${sim.slots.length} / ${maxUids}`} />
        <ExplainerStat
          label="Immune (new)"
          value={String(newImmuneCount)}
          hint="Within immunity_period"
        />
        <ExplainerStat
          label="Owner-immune"
          value={`${ownerImmune.size} / ${ownerLimit}`}
          hint="Never pruned"
        />
        <ExplainerStat
          label="Next pruned"
          value={nextPrune === null ? '—' : `uid ${nextPrune}`}
          hint={sim.slots.length < maxUids ? 'Subnet not full' : 'Lowest emission, prunable'}
        />
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <div className={focusClass('max_allowed_uids')}>
          <ExplainerSlider
            label="max_allowed_uids"
            value={maxUids}
            min={16}
            max={48}
            step={4}
            display={`${maxUids} slots`}
            onChange={changeMaxUids}
          />
        </div>
        <div className={focusClass('immunity_period')}>
          <ExplainerSlider
            label="immunity_period"
            value={immunityPeriod}
            min={0}
            max={16}
            step={1}
            display={`${immunityPeriod} regs`}
            onChange={setImmunityPeriod}
          />
        </div>
        <div className={focusClass('owner_immune_neuron_limit')}>
          <ExplainerSlider
            label="owner_immune_neuron_limit"
            value={ownerLimit}
            min={1}
            max={10}
            step={1}
            display={String(ownerLimit)}
            onChange={setOwnerLimit}
          />
        </div>
      </div>
    </ExplainerPanel>
  );
}
