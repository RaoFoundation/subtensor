import Link from 'next/link';
import type { MDXComponents } from 'mdx/types';
import type { ComponentProps, ReactNode } from 'react';
import { CopyCodeButton } from './copy';
import { EvmAddressDomains } from './docs/evm-address-domains';
import { EvmMoneyFlows } from './docs/evm-money-flows';
import { ConvictionLockChart } from './docs/conviction-lock-chart';
import { ConvictionModeComparison } from './docs/conviction-mode-comparison';
import { ConvictionSubnetScenario } from './docs/conviction-subnet-scenario';
import { EmissionFlowDiagram } from './docs/emission-flow-diagram';
import { EmissionNetworkSnapshot } from './docs/emission-network-snapshot';
import { HyperparamActivityCutoffChart } from './docs/hyperparam-activity-cutoff-chart';
import { HyperparamBondsPenaltyChart } from './docs/hyperparam-bonds-penalty-chart';
import { HyperparamBondsResetChart } from './docs/hyperparam-bonds-reset-chart';
import { HyperparamBurnController } from './docs/hyperparam-burn-controller';
import { HyperparamChildkeyTakeRange } from './docs/hyperparam-childkey-take-range';
import { HyperparamConsensusSigmoid } from './docs/hyperparam-consensus-sigmoid';
import { HyperparamMaxValidatorsChart } from './docs/hyperparam-max-validators-chart';
import { HyperparamLegacyAdjustment } from './docs/hyperparam-legacy-adjustment';
import { HyperparamLiquidAlpha } from './docs/hyperparam-liquid-alpha';
import { HyperparamOwnerCut } from './docs/hyperparam-owner-cut';
import { HyperparamPowDifficulty } from './docs/hyperparam-pow-difficulty';
import { HyperparamPowToggleDeadEnd } from './docs/hyperparam-pow-toggle-dead-end';
import { HyperparamRegistrationGate } from './docs/hyperparam-registration-gate';
import { HyperparamRegsCapChart } from './docs/hyperparam-regs-cap-chart';
import { HyperparamRegsPerBlockChart } from './docs/hyperparam-regs-per-block-chart';
import { HyperparamServingRateLimitChart } from './docs/hyperparam-serving-rate-limit-chart';
import { HyperparamTempoTimeline } from './docs/hyperparam-tempo-timeline';
import { HyperparamTransfersEnabledFlow } from './docs/hyperparam-transfers-enabled-flow';
import { HyperparamUidLifecycle } from './docs/hyperparam-uid-lifecycle';
import { HyperparamWeightsRateLimitChart } from './docs/hyperparam-weights-rate-limit-chart';
import { HyperparamWeightsRules } from './docs/hyperparam-weights-rules';
import { HyperparamWeightsVersionGate } from './docs/hyperparam-weights-version-gate';
import { HyperparamYuma3Chart } from './docs/hyperparam-yuma3-chart';
import { RegistrationBurnTimeline } from './docs/registration-burn-timeline';
import { RootProportionExplainer } from './docs/root-proportion-explainer';
import { SubnetEmissionShareChart } from './docs/subnet-emission-share-chart';
import { TaoHalvingChart } from './docs/tao-halving-chart';
import { YumaConsensusDemo } from './docs/yuma-consensus-demo';
import { cn } from '@/lib/cn';

function heading(Tag: 'h2' | 'h3' | 'h4') {
  return function Heading({ id, children, ...props }: ComponentProps<typeof Tag>) {
    if (!id) return <Tag {...props}>{children}</Tag>;
    return (
      <Tag id={id} {...props}>
        <a href={`#${id}`}>{children}</a>
      </Tag>
    );
  };
}

function Anchor({ href, ...props }: ComponentProps<'a'>) {
  if (href && href.startsWith('/')) {
    return <Link href={href} {...props} />;
  }
  return <a href={href} {...props} />;
}

function Pre(props: ComponentProps<'pre'>) {
  return (
    <div className="bt-codeblock">
      <pre {...props} />
      <CopyCodeButton />
    </div>
  );
}

export function Cards({ children }: { children: ReactNode }) {
  return <div className="grid gap-px sm:grid-cols-2 border border-line bg-line not-prose my-6">{children}</div>;
}

export function Card({
  title,
  description,
  href,
}: {
  title: string;
  description?: string;
  href: string;
}) {
  return (
    <Link
      href={href}
      className={cn(
        'block bg-bg p-5 transition-colors hover:bg-panel',
      )}
    >
      <p className="bt-label mb-2">{title}</p>
      {description && (
        <p className="text-[0.8125rem] leading-relaxed text-mute">{description}</p>
      )}
    </Link>
  );
}

export function Callout({
  type = 'note',
  children,
}: {
  type?: 'note' | 'warning';
  children: ReactNode;
}) {
  return (
    <aside
      className={cn(
        'not-prose my-6 border-s-2 py-1 ps-4 text-[0.875rem] leading-relaxed text-mute',
        type === 'warning' ? 'border-fg' : 'border-line',
      )}
    >
      {children}
    </aside>
  );
}

export function getMDXComponents(components?: MDXComponents) {
  return {
    h2: heading('h2'),
    h3: heading('h3'),
    h4: heading('h4'),
    a: Anchor,
    pre: Pre,
    Cards,
    Card,
    Callout,
    TaoHalvingChart,
    SubnetEmissionShareChart,
    YumaConsensusDemo,
    EmissionNetworkSnapshot,
    EmissionFlowDiagram,
    RootProportionExplainer,
    RegistrationBurnTimeline,
    ConvictionLockChart,
    ConvictionSubnetScenario,
    ConvictionModeComparison,
    EvmAddressDomains,
    EvmMoneyFlows,
    HyperparamActivityCutoffChart,
    HyperparamBondsPenaltyChart,
    HyperparamBondsResetChart,
    HyperparamBurnController,
    HyperparamChildkeyTakeRange,
    HyperparamConsensusSigmoid,
    HyperparamMaxValidatorsChart,
    HyperparamLegacyAdjustment,
    HyperparamLiquidAlpha,
    HyperparamOwnerCut,
    HyperparamPowDifficulty,
    HyperparamPowToggleDeadEnd,
    HyperparamRegistrationGate,
    HyperparamRegsCapChart,
    HyperparamRegsPerBlockChart,
    HyperparamServingRateLimitChart,
    HyperparamTempoTimeline,
    HyperparamTransfersEnabledFlow,
    HyperparamUidLifecycle,
    HyperparamWeightsRateLimitChart,
    HyperparamWeightsRules,
    HyperparamWeightsVersionGate,
    HyperparamYuma3Chart,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}
