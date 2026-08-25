import {ExplainerPanel} from './explainer-panel';
import {INK} from './chart-theme';

function Row({name, value}: {name: string; value: string}) {
  return (
    <div className='flex items-baseline justify-between gap-3 border-t border-line pt-2'>
      <dt className='text-[0.75rem] text-mute'>{name}</dt>
      <dd className='font-mono text-xs' style={{color: INK}}>
        {value}
      </dd>
    </div>
  );
}

export function BetaFundDiagram() {
  return (
    <ExplainerPanel
      title='One validator, one fund'
      caption='The basket holds real tokens. Beta is how you split ownership of that pile. Alice owns 40 of 100 shares, so a claim pays her 40% of whatever the pile is worth in TAO.'
    >
      <div className='grid gap-8 md:grid-cols-2'>
        <div>
          <p className='font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute'>
            In the basket
          </p>
          <dl className='mt-3 space-y-2'>
            <Row name='Subnet 4 tokens' value='some α' />
            <Row name='Subnet 8 tokens' value='some α' />
            <Row name='TAO cash' value='held 1:1' />
          </dl>
          <p className='mt-3 text-[0.75rem] leading-relaxed text-mute'>
            Worth 120 τ if sold today.
          </p>
        </div>
        <div>
          <p className='font-mono text-[0.625rem] uppercase tracking-[0.08em] text-mute'>
            Who owns it
          </p>
          <dl className='mt-3 space-y-2'>
            <Row name='Alice' value='40 β' />
            <Row name='Bob' value='60 β' />
          </dl>
          <p className='mt-3 text-[0.75rem] leading-relaxed text-mute'>
            100 β in total. Price is 1.2 τ per β. Alice’s claim is 48 τ.
          </p>
        </div>
      </div>
    </ExplainerPanel>
  );
}
