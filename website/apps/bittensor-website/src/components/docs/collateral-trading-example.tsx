'use client';

import {TradingCollateralDiagram} from './collateral-trading-diagram';
import {ExplainerPanel, ExplainerStat} from './explainer-panel';

export function TradingCollateralExample() {
  return (
    <ExplainerPanel
      title='Trading-signals subnet'
      tag='Sortino · tail risk'
      caption={
        'A martingale can post a top Sortino for months, farm emissions, then blow up. ' +
        'Pure burn lets the farmer re-register cheaply. A large lock with a slow drain ' +
        'keeps capital at risk through the detection window so the blow-up strands the bond.'
      }
    >
      <TradingCollateralDiagram className='h-auto w-full' />
      <div className='mt-6 grid gap-4 sm:grid-cols-3'>
        <ExplainerStat label='Owner settings' value='p = 90%, k = 0.2' hint='anti-tail-risk preset' />
        <ExplainerStat
          label='Break-even before ban'
          value='E* ≈ 0.83 · T'
          hint='must farm this much just to recover costs'
          accent
        />
        <ExplainerStat
          label='Honest miner'
          value='τ1 sunk'
          hint='τ9 lock releases as real edge earns incentive'
        />
      </div>
    </ExplainerPanel>
  );
}
