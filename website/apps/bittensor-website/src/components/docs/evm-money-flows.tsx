'use client';

import { ExplainerPanel } from './explainer-panel';

type FlowStep = { label: string; detail: string };

function FlowCard({
  title,
  command,
  signer,
  gas,
  steps,
  note,
}: {
  title: string;
  command: string;
  signer: string;
  gas: string;
  steps: FlowStep[];
  note?: string;
}) {
  return (
    <div className="border border-line bg-bg p-4">
      <div className="mb-3 flex flex-wrap items-start justify-between gap-2">
        <p className="bt-label">{title}</p>
        <code className="text-[0.6875rem] text-mute">{command}</code>
      </div>
      <div className="mb-3 flex flex-wrap gap-2 text-[0.6875rem]">
        <span className="border border-line px-2 py-0.5 text-mute">signs: {signer}</span>
        <span className="border border-line px-2 py-0.5 text-mute">gas: {gas}</span>
      </div>
      <ol className="space-y-2">
        {steps.map((step, index) => (
          <li key={step.label} className="flex gap-3 text-[0.8125rem]">
            <span className="font-mono text-xs text-mute">{index + 1}.</span>
            <div>
              <p>{step.label}</p>
              <p className="text-mute">{step.detail}</p>
            </div>
          </li>
        ))}
      </ol>
      {note && <p className="mt-3 text-[0.75rem] text-mute">{note}</p>}
    </div>
  );
}

export function EvmMoneyFlows() {
  return (
    <ExplainerPanel
      title="Four ways TAO crosses the ss58 ↔ EVM seam"
      caption="Pick the path that matches who holds the keys. The two “withdraw” names are easy to confuse — read the signer and gas columns."
    >
      <div className="grid gap-4 lg:grid-cols-2">
        <FlowCard
          title="Fund an EVM key from your coldkey"
          command="btcli evm fund"
          signer="coldkey (substrate)"
          gas="substrate fee only"
          steps={[
            { label: 'Create or import an EVM key', detail: 'btcli evm key new' },
            { label: 'Coldkey transfers TAO to the key’s ss58 mirror', detail: 'Wraps fund_evm_key intent' },
            { label: 'Balance shows in MetaMask / btcli evm balance', detail: '18-decimal EVM view' },
          ]}
        />
        <FlowCard
          title="Send between EVM accounts"
          command="btcli evm send"
          signer="stored EVM key"
          gas="EVM gas (wei)"
          steps={[
            { label: 'Pick source key (--evm-key)', detail: 'Defaults to wallet default key' },
            { label: 'Ordinary value transfer to another 0x address', detail: 'Like Ethereum send' },
          ]}
        />
        <FlowCard
          title="EVM key → any ss58 address"
          command="btcli evm send-to-ss58"
          signer="stored EVM key"
          gas="EVM gas (wei)"
          steps={[
            { label: 'Call BalanceTransfer precompile with msg.value', detail: 'Destination needs no setup' },
            { label: 'TAO credits the ss58 account natively', detail: 'Not the same as claim-deposit' },
          ]}
          note="Formerly named evm withdraw — renamed to avoid confusion with claim-deposit."
        />
        <FlowCard
          title="MetaMask deposit → coldkey"
          command="btcli evm claim-deposit"
          signer="coldkey (substrate)"
          gas="substrate fee only"
          steps={[
            { label: 'Show your deposit address', detail: 'btcli evm deposit-address' },
            { label: 'Send TAO from MetaMask to that 0x address', detail: 'Credits the truncated mirror' },
            { label: 'Pull funds into the coldkey', detail: 'Also: btcli tx evm-withdraw' },
          ]}
          note="Uses the truncated mapping (first 20 bytes of pubkey), not the hashed mirror."
        />
      </div>
    </ExplainerPanel>
  );
}
