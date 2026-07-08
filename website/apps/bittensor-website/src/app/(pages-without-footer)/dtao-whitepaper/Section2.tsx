import React from 'react';
import styles from './page.module.css';

const Section2 = () => {
  return (
    <section className={styles.section}>
      <h2 className={styles.subtitle}>Section 2: What is DTAO</h2>
      <p>
        At it&apos;s core, Dynamic TAO (DTAO) is an upgrade to the Bittensor chain that replaces the
        previous emission logic with an intelligent, market&#45;driven mechanism that can be used to
        determine token emissions. Operationally, we introduce subnet tokens (which we will
        informally refer to as Alpha1, Alpha2, Alpha3, etc.) through which miners, validators, and
        subnet owners will earn rewards. In so far as Bittensor enables the decentralized production
        and distribution of digital commodities, subnet tokens serve as the medium through which the
        network intelligently values these commodities. To facilitate this valuation, we also
        introduce subnet pools, which are Constant Product AMMs (described in details in section
        3.1) through which users can stake TAO to receive subnet tokens and vice versa. The price
        discovery that occurs in these subnet pools functions as a view into how the Bittensor
        network values each subnet. Critically, this is exactly what we want, i.e. a scalable
        mechanism for valuing subnets that does not rely on a privileged, corruptible set of
        validators. Specifically, we will use the subnet token prices to guide the emission process.
      </p>
      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_1.jpg'
          alt='The Bittensor Subnets'
          className={styles.image_container_image}
        />
        <p className={styles.image_container_caption}>
          <span className={styles.image_container_caption_no}>Figure 1: </span>
          The Bittensor Subnets.
        </p>
      </div>
      <p>
        DTAO represents a fundamental shift towards true market democracy in the valuation of
        subnets. Where the previous system concentrated power in the hands of a few validators, DTAO
        opens participation to anyone willing to stake TAO in subnet pools. This broader
        participation not only makes the system more resistant to manipulation (try bribing a vast
        number of market participants for an extended period of time) but also ensures that subnet
        valuations reflect the collective wisdom of the market rather than the potentially biased
        views of a small group. Moreover, this market&#45;driven approach scales naturally with
        network growth &#45; as more subnets are added, the market simply expands to accommodate
        them, without degrading the quality of price discovery. Of course, this upgrade leads to a
        number of downstream changes, the core of which are discussed in detail in section (3.1)
        &#45; (3.6), and the rest discussed in the appendix.
      </p>
    </section>
  );
};

export default Section2;
