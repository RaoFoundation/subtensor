'use client';

import { useState } from 'react';
import { ExplainerPanel, ExplainerStat } from './explainer-panel';

const WINDOWS = 12;
const ATTEMPTS_PER_WINDOW = 2;

// Initial history: gate open, closed for a stretch, reopened.
const INITIAL_GATE = Array.from({ length: WINDOWS }, (_, i) => i < 4 || i >= 8);

export function HyperparamRegistrationGate() {
  const [gate, setGate] = useState<boolean[]>(INITIAL_GATE);

  const openWindows = gate.filter(Boolean).length;
  const accepted = openWindows * ATTEMPTS_PER_WINDOW;
  const rejected = (WINDOWS - openWindows) * ATTEMPTS_PER_WINDOW;

  return (
    <ExplainerPanel
      title="The registration_allowed gate"
      caption="Both burned_register and the legacy PoW register land in do_register, which checks get_network_registration_allowed at step 3 before anything else about the caller matters. Click a stretch of blocks to flip the flag there: every attempt under a closed gate fails with SubNetRegistrationDisabled, no matter how much TAO the caller offers."
    >
      <div className="flex items-baseline justify-between">
        <p className="bt-label text-mute">registration_allowed over time (click to toggle)</p>
        <p className="font-mono text-[0.7rem] text-mute">blocks &rarr;</p>
      </div>

      <div className="mt-2 flex gap-1">
        {gate.map((open, i) => (
          <button
            key={i}
            type="button"
            aria-label={`Blocks ${i + 1}: registration_allowed ${open ? 'true, click to close' : 'false, click to open'}`}
            onClick={() => setGate((g) => g.map((v, j) => (j === i ? !v : v)))}
            className="group flex min-w-0 flex-1 flex-col gap-1"
          >
            <span
              className={
                open
                  ? 'h-2 bg-[rgb(41,41,41)]'
                  : 'h-2 border border-dashed border-[rgba(41,41,41,0.45)] bg-transparent'
              }
            />
            <span className="flex flex-col items-center gap-0.5 border border-line bg-bg py-1.5 font-mono text-[0.75rem] leading-none group-hover:bg-panel">
              {Array.from({ length: ATTEMPTS_PER_WINDOW }).map((_, k) => (
                <span key={k} className={open ? '' : 'text-mute'}>
                  {open ? '\u2713' : '\u2717'}
                </span>
              ))}
            </span>
          </button>
        ))}
      </div>

      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 font-mono text-[0.7rem] text-mute">
        <span>&#9632; gate open &middot; &#9633; gate closed</span>
        <span>{'\u2713'} UID assigned &middot; {'\u2717'} Err(SubNetRegistrationDisabled)</span>
      </div>

      <div className="mt-5 grid gap-4 sm:grid-cols-3">
        <ExplainerStat
          label="Attempts accepted"
          value={`${accepted}`}
          hint="gate open: burned and legacy PoW paths both proceed"
        />
        <ExplainerStat
          label="Attempts rejected"
          value={`${rejected}`}
          hint="SubNetRegistrationDisabled, regardless of burn offered"
        />
        <ExplainerStat
          label="Existing neurons"
          value="unaffected"
          hint="the flag only stops new registrations; pruning and immunity continue"
        />
      </div>
    </ExplainerPanel>
  );
}
