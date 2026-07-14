'use client';

import type { ReactNode } from 'react';
import { ExplainerPanel, ExplainerStat } from './explainer-panel';

function Domain({
  title,
  example,
  signs,
  children,
}: {
  title: string;
  example: string;
  signs: string;
  children: ReactNode;
}) {
  return (
    <div className="border-t border-line pt-3">
      <p className="font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">{title}</p>
      <p className="mt-2 font-mono text-xs break-all">{example}</p>
      <p className="mt-2 text-[0.8125rem] text-mute">{signs}</p>
      <div className="mt-3 space-y-1 text-[0.8125rem]">{children}</div>
    </div>
  );
}

function Arrow({ label }: { label?: string }) {
  return (
    <div className="flex flex-col items-center justify-center px-2 text-mute">
      <span className="text-xl">→</span>
      {label && (
        <span className="mt-1 text-center font-mono text-[0.625rem] uppercase tracking-[0.08em] leading-tight">
          {label}
        </span>
      )}
    </div>
  );
}

function Mapping({
  title,
  commands,
  children,
}: {
  title: string;
  commands: string;
  children: ReactNode;
}) {
  return (
    <div className="border-t border-line pt-3">
      <p className="font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute">{title}</p>
      <p className="mt-2 text-[0.8125rem] leading-relaxed">{children}</p>
      <p className="mt-2 font-mono text-[0.75rem] text-mute">{commands}</p>
    </div>
  );
}

export function EvmAddressDomains() {
  return (
    <ExplainerPanel
      title="Two signing domains, two address mappings"
      caption="Conversions are deterministic but never carry private keys — a btcli wallet cannot sign EVM txs, and MetaMask cannot sign extrinsics."
    >
      <div className="grid gap-4 lg:grid-cols-[1fr_auto_1fr] lg:items-stretch">
        <Domain title="Native (ss58)" example="5GrwvaEF…" signs="sr25519 / ed25519 extrinsics">
          <p>Coldkeys, hotkeys, neurons</p>
          <p>Used by <code className="text-xs">btcli tx …</code></p>
        </Domain>
        <Arrow label="same chain" />
        <Domain title="EVM (h160)" example="0x742d35Cc…" signs="secp256k1 EVM transactions">
          <p>MetaMask, Hardhat, contracts</p>
          <p>Used by <code className="text-xs">eth_sendRawTransaction</code></p>
        </Domain>
      </div>

      <div className="mt-8 grid gap-x-8 gap-y-6 md:grid-cols-2">
        <Mapping
          title="Hashed mirror (fund an EVM account)"
          commands="btcli evm mirror · btcli evm fund"
        >
          Every h160 has an ss58 <strong>mirror</strong>:{' '}
          <code className="text-xs">ss58(blake2(&quot;evm:&quot; ++ h160))</code>. Transfer TAO to
          the mirror and it appears as that EVM account&apos;s balance.
        </Mapping>
        <Mapping
          title="Truncated mapping (claim a MetaMask deposit)"
          commands="btcli evm deposit-address · btcli evm claim-deposit"
        >
          Every ss58 account controls one h160: the <strong>first 20 bytes</strong> of its public
          key. Send TAO from MetaMask to that address, then claim it on the native side.
        </Mapping>
      </div>

      <div className="mt-6 grid grid-cols-2 gap-x-8 gap-y-4 border-t border-line pt-4 sm:grid-cols-3">
        <ExplainerStat label="Native decimals" value="1 TAO = 1e9 rao" />
        <ExplainerStat label="EVM decimals" value="1 TAO = 1e18 wei" hint="Same funds, different display scale" />
        <ExplainerStat label="Key rule" value="Never mixed" hint="Fund with mirror; claim with truncated" />
      </div>
    </ExplainerPanel>
  );
}
