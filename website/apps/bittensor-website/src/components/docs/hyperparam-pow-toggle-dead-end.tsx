'use client';

import { useState } from 'react';
import { ExplainerPanel } from './explainer-panel';

export function HyperparamPowToggleDeadEnd() {
  const [attempts, setAttempts] = useState(0);

  return (
    <ExplainerPanel
      title="A toggle wired to a dead end"
      caption="On the current runtime, both sides of this flag are disconnected. The setter refuses every write, and the register extrinsic no longer consults the flag on its way to the burned-registration path."
    >
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="border border-line bg-bg p-3">
          <p className="bt-label text-mute">Trying to set the flag</p>
          <div className="mt-3 font-mono text-[0.75rem] leading-6">
            <p>sudo_set_network_pow_registration_allowed({attempts % 2 === 0 ? 'true' : 'false'})</p>
            <p className="text-mute">&nbsp;&nbsp;&darr;</p>
            <p className="inline-block border border-dashed border-[rgba(41,41,41,0.45)] px-2 py-0.5">
              Err(POWRegistrationDisabled)
            </p>
            {attempts > 0 && (
              <p className="mt-1 text-[0.7rem] text-mute">
                {attempts} attempt{attempts === 1 ? '' : 's'}, {attempts} rejection{attempts === 1 ? '' : 's'} &mdash; the
                extrinsic writes nothing, ever
              </p>
            )}
          </div>
          <button
            type="button"
            onClick={() => setAttempts((n) => n + 1)}
            className="mt-3 border border-line bg-panel px-3 py-1 font-mono text-[0.75rem] hover:bg-bg"
          >
            try to flip it
          </button>
        </div>

        <div className="border border-line bg-bg p-3">
          <p className="bt-label text-mute">What registration does meanwhile</p>
          <div className="mt-3 font-mono text-[0.75rem] leading-6">
            <p>register(block, nonce, work, &hellip;)</p>
            <p className="text-mute">&nbsp;&nbsp;&darr; work args ignored</p>
            <p>do_register() &mdash; burned path</p>
            <p className="mt-2 text-mute">
              network_pow_registration_allowed
              <br />
              &nbsp;&nbsp;&#8618; read by nothing on this route
            </p>
          </div>
          <p className="mt-3 text-[0.7rem] text-mute">
            The stored value (cleared to the default by migration) is still reported in the metagraph as
            pow_registration_allowed, but no code path branches on it.
          </p>
        </div>
      </div>
    </ExplainerPanel>
  );
}
