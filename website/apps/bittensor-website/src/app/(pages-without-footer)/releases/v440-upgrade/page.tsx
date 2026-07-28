import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Code} from '@/app/components/Code/Code';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';
import {EmissionGateExplorer} from './EmissionGateExplorer';
import {GateCurveDiagram, QMassBarDiagram, SlotCostDiagram} from './diagrams';

export const metadata: Metadata = {
  title: 'The V440 Upgrade — The Emission Gate',
  description:
    'Subnet emission remains price-based, but prices now pass through a threshold gate ' +
    'centered around rank 32. Idle slots earn little, and the cost of building on Bittensor ' +
    'falls toward the registration transaction.',
  alternates: {canonical: '/releases/v440-upgrade'},
};

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The V440 Upgrade</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            The Emission Gate · July 2026
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Today, every registered subnet earns emission in direct proportion to its moving price.
            That rule is simple but too flat: for instance, a subnet nobody uses still collects a
            slice of every block, which is to say, the tail of the network dilutes the subnets
            doing real work. Simultaneously, the passive income of a parked slot props up the cost
            of registering a new one, since the expected return of nothing is large. The result:
            the network cannot grow without taxing its champions.
          </p>
          <p>
            Spec <strong>440</strong> changes Bittensor&apos;s emission curve. Emission remains
            price-based, but prices now pass through a threshold &apos;gate&apos; — an inflection
            point centered around the 32nd rank. Above the gate, emission is essentially unchanged.
            Below it, emission collapses toward zero. Technically no subnet is hard-cut — every
            price still gets some emission — but a parked slot pays substantially less than an
            active one.
          </p>
          <p>
            Because a slot below the gate earns little, slots no longer need to be scarce — this
            release clears the path to raise the subnet cap later, without taxing the head today.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Thesis: a slot should cost nothing</p>
          <p>
            The cost of registering a subnet has been one of the network&apos;s quietest failures.
            Under flat price-proportional emission, even the worst slot on the network carried a
            passive yield, so slots traded at inflated values. This taxed teams who wanted to
            build, since they needed to borrow against the dead weight of the slot itself
            (currently ~1300 TAO). Most took on debt to make this happen. The people the network
            most wants were the ones the pricing punished.
          </p>
          <p>
            Release 440 changes this by lowering the base return of an idle slot to approximately
            nothing, so an idle slot is worth approximately nothing, and the price of entry should
            fall toward the cost of the registration transaction. In other words, a team no longer
            buys a guaranteed slice of emission — just a starting position below a strict
            competition bar. Over time, new and creative teams can earn emission by attracting
            substantial demand from TAO holders.
          </p>
          <SlotCostDiagram />
          <p className={styles.graph_caption}>
            Idle yield props the market price of a slot under flat emission. With the gate, idle
            carry collapses and entry should fall toward the registration transaction.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>The bar and the gate</p>
          <p>
            We create the gate as follows: sort subnets by price and walk down until the subnets
            above you carry a fraction <code>q</code> of all prices — the price at that point is
            the &apos;bar&apos;. Each subnet&apos;s emission weight is then thresholded around this
            point.
          </p>
          <QMassBarDiagram />
          <p className={styles.graph_caption}>
            The q-mass bar: accumulate sorted demand until the running total crosses q; the share
            at the crossing is θ. Defaults put that crossing near rank 32 on today&apos;s Finney
            distribution.
          </p>
          <Code
            language='rust'
            code={`// pallets/subtensor/src/coinbase/subnet_emissions.rs
let theta = Self::q_mass_bar(&shares, EmissionBarQuantile::<T>::get()); // q
let h = EmissionGateExponent::<T>::get();                               // h

// gate(s) = s^h / (s^h + theta^h); emission weight = s * gate(s)
let weights: Vec<U64F64> = shares
    .iter()
    .map(|s| {
        let sh = s.saturating_pow(h);
        s.saturating_mul(sh.safe_div(sh.saturating_add(theta.saturating_pow(h))))
    })
    .collect();`}
          />
          <p>
            We use a smooth function for the threshold (a sigmoid). At the threshold it cuts
            exactly half of a subnet&apos;s emission; two ranks above, nothing is cut; deep below
            that rank, a subnet gets almost nothing.
          </p>
          <GateCurveDiagram />
          <p className={styles.graph_caption}>
            Hill gate at h = 3. At s = θ the gate passes exactly ½; well above the bar it is ~1;
            deep in the tail it is ~0.
          </p>
          <p>
            Notably, if the tail grows real demand, θ rises and the standard gets harder for
            everyone; if demand concentrates, θ falls. At the defaults — <code>q = 0.61</code>,{' '}
            <code>h = 3</code> — the bar lands on today&apos;s distribution at the average demand
            share of an active subnet, around rank 32. A subnet earns its full linear emission only
            if it attracts more than an average slice of demand. Because the bar is a property of
            the demand distribution, registering new subnets does not move it.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>See the cliff</p>
          <p>
            The figure below is computed from a live mainnet snapshot of all 126 subnets, with
            every subnet treated as emission-enabled. The muted line is emission under the old
            price-proportional rule; the red line is emission through the gate. Drag <code>q</code>{' '}
            to move the bar and <code>h</code> to sharpen or soften the cliff.
          </p>
          <EmissionGateExplorer />
          <p className={styles.graph_caption}>
            Finney SubnetMovingPrice snapshot, July 2026. Defaults q = 0.61, h = 3 — the shipped
            hyperparameter values.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What the defaults do</p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Measure</th>
                <th>Before</th>
                <th>After (q = 0.61, h = 3)</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Emission below the bar (94 subnets)</td>
                <td>38.4%</td>
                <td>12.5%</td>
              </tr>
              <tr>
                <td>Emission to the top 8</td>
                <td>32.8%</td>
                <td>52.7%</td>
              </tr>
              <tr>
                <td>Rank where cumulative emission reaches 80%</td>
                <td>64</td>
                <td>23</td>
              </tr>
              <tr>
                <td>Effective subnet count (1/Σs²)</td>
                <td>~50</td>
                <td>~22</td>
              </tr>
              <tr>
                <td>Subnets hard-zeroed</td>
                <td>0</td>
                <td>0</td>
              </tr>
            </tbody>
          </table>
          <p>
            At h = 3 the cliff is decisive. A subnet just above the bar keeps its emission almost
            untouched; the mid-tail keeps 15–30% of its linear share; the deep tail keeps 1–5% — a
            token drip that marks the climb path without being worth holding. And the leverage near
            the bar is strong in the right direction: a subnet at rank 36 that grows its demand 10%
            gains roughly 26% more emission. Growth near the cliff is the best-paid move on the
            network.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Parameters</p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Hyperparameter</th>
                <th>Default</th>
                <th>Meaning</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>
                  <code>EmissionBarQuantile</code>
                </td>
                <td>0.61</td>
                <td>Fraction of demand carried by subnets above the bar (sets θ)</td>
              </tr>
              <tr>
                <td>
                  <code>EmissionGateExponent</code>
                </td>
                <td>3</td>
                <td>Hill exponent h — cliff sharpness at the bar</td>
              </tr>
            </tbody>
          </table>
          <p>
            Both are root-sudo settable and rate-limited. The bar itself is recomputed once per
            tempo from the same de-manipulated moving prices that already drive emission, so a
            single block of wash trading cannot move it. Because a sharper h = 3 gate makes the
            boundary more sensitive, that per-tempo cadence is what keeps emission near the bar
            steady rather than flapping.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>What to do</p>
          <ul className={styles.list}>
            <li>
              <strong>Node operators:</strong> wait for the on-chain <code>spec_version</code> to
              move to 440, then update to the matching release.
            </li>
            <li>
              <strong>Subnet owners below the bar:</strong> your emission does not go to zero, but
              it no longer pays to idle. The bar is public — query it with{' '}
              <code>btcli subnets bar</code> and track your share against θ.
            </li>
            <li>
              <strong>Subnet owners above the bar:</strong> nothing to change. Your emission rises
              as the tail&apos;s subsidy is redistributed.
            </li>
            <li>
              <strong>Stakers:</strong> alpha positions in deep-tail subnets earn materially less
              emission after this upgrade; the head earns more. Reprice accordingly.
            </li>
            <li>
              <strong>Teams waiting to build:</strong> existing slots lose the passive carry that
              made them expensive. Expect registration cost to fall toward the transaction fee — a
              slot buys a starting position, not an income stream. Raising the subnet slot cap is
              deferred to a later release.
            </li>
          </ul>
          <p>
            Signers: after the release train proposes, use{' '}
            <code>btcli upgrade sign --url &lt;v440 release URL&gt; -w &lt;wallet&gt;</code>.
          </p>
        </section>

        <span className={styles.paper_link}>
          <Link href='/code/pallets/subtensor/src/coinbase/subnet_emissions.rs'>
            Read the emission gate implementation
          </Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
