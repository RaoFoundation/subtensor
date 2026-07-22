'use client';

import {LiumCollateralDiagram} from './collateral-lium-diagram';
import {ExplainerPanel, ExplainerStat} from './explainer-panel';

export function LiumCollateralExample() {
  return (
    <ExplainerPanel
      title='Lium-style GPU marketplace'
      tag='per-machine deposits'
      caption={
        'Miners back each machine with alpha collateral. add_collateral funds the bond; ' +
        'set_min_collateral parks a floor so the drain cannot undercut the deposit. ' +
        'An honest wind-down clears the floor and keeps earning; pulling a rented machine ' +
        'zeroes the score and strands what is left.'
      }
    >
      <LiumCollateralDiagram className='h-auto w-full' />
      <div className='mt-6 grid gap-4 sm:grid-cols-3'>
        <ExplainerStat
          label='Owner settings'
          value='p = 50%, k = 1'
          hint='modest reg bond + fast deposit drain'
        />
        <ExplainerStat
          label='Published policy'
          value='25α / machine'
          hint='validators read collateral_locked on the metagraph'
        />
        <ExplainerStat
          label='4 machines'
          value='100α floor'
          hint='add then set-min; incentive refills shortfalls'
          accent
        />
      </div>
    </ExplainerPanel>
  );
}
