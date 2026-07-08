import React from 'react';
import {InlineMath} from 'react-katex';
import {Equations} from './components/Equations';
import styles from './page.module.css';

const Section1 = () => {
  return (
    <section className={styles.section}>
      <h2 className={styles.subtitle}>Section 1: Motivation</h2>
      <p>
        Bittensor incentivizes the production and distribution of digital commodities by emitting
        newly minted TAO in each block to a set of subnets. This TAO is distributed to the subnets
        according to what is collectively referred to as the emission vector{' '}
        <InlineMath>{'E \\rightarrow [E_1, E_2,..., E_n]'}</InlineMath>. Within each subnet, this
        emission is divided across three participant groups in the corresponding proportion:
      </p>
      <ul className={styles.unorder_list}>
        <li>Validators (41%)</li>
        <li>Miners (41%)</li>
        <li>Subnet Owners (18%)</li>
      </ul>
      <p>
        The details of the calculation are not important here, but roughly speaking, the emission
        vector is determined by the following calculation (where <InlineMath>{'i'}</InlineMath>{' '}
        stands for the <InlineMath>{'i^{th}'}</InlineMath> subnet):
      </p>
      <Equations
        minify={true}
        equ={`
  E_i = \\sum_j
  \\left( \\begin{array}{c} \\text{j}^{\\text{th}} \\text{ validator's} \\\\ \\text{staked TAO} \\end{array} \\right)
  \\left( \\begin{array}{c} \\text{j}^{\\text{th}} \\text{ validator's favorability} \\\\ \\text{weight of } i^{\\text{th}} \\text{ subnet} \\end{array} \\right)
  `}
      />
      <p>
        The driving term in the above equation is the validator&apos;s favorability towards each
        subnet. Thus, these favorabilities essentially constitute a form of voting. Perhaps ideally,
        the weight vector resulting from this voting process would reward the subnets that are the
        most deserving, or those with the highest capital requirements. Problematically, the current
        system relies entirely on validators to manually assess and determine subnet value through
        their voting power. This creates a fundamental scaling problem: as the number of subnets
        grows, validators become increasingly unable to thoroughly evaluate each subnet&apos;s
        contribution to the network. The sheer volume of subnets requiring assessment often leads to
        validator apathy, where thorough evaluation becomes practically impossible.
      </p>
      <p>
        This apathy can be further exacerbated by the lack of any meaningful consequences for
        validators who fail to maintain active and accurate weighting patterns. Indeed, the system
        provides no direct incentive for validators to regularly update their weight assignments or
        carefully consider their voting decisions. Even worse, the system incentivizes behavior that
        need not align with any ideal hypothetical weight distribution across subnets. As an example
        of a trivial way of manipulating the mechanism, validators may put more weight on subnets
        they actively validate upon in order boost their rewards (even if these are suboptimal
        subnets). Indeed, it would be merely altruistic for validators to provide weight to subnets
        on which they do not actively validate.
      </p>
      <p>
        Another form of manipulation involves subnets owners offering revenue sharing agreements or
        other inducements to validators in exchange for larger weights on their subnet. Insidiously,
        this is a win&#45;win scenario for the validators and subnet owners.
      </p>
      <p>
        Fundamentally, the current incentive mechanism, with all of its flaws, can lead to less
        deserving subnets being rewarded in place of the more deserving subnets that would in fact
        accrue greater value to the Bittensor ecosystem. This is the motivation for introducing
        DTAO.
      </p>
    </section>
  );
};

export default Section1;
