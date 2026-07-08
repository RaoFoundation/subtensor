import React from 'react';
import {InlineMath} from 'react-katex';
import {Equations} from './components/Equations';
import styles from './page.module.css';

const Section3 = () => {
  return (
    <section className={styles.section}>
      <h2 className={styles.subtitle}>Section 3: Technical Overview</h2>
      <h3 className={styles.subtitle}>Section 3.0: A Look at the Components</h3>
      <p>
        In this section, we will look at some of the technical aspects of the DTAO system in a
        little more detail. There are many components to go over, so we will divide this section
        into five subsections, according to the following:
      </p>
      <ul>
        <li>(3.1) the AMM governing the subnet pools</li>
        <li>(3.2) the way in which new liquidity is injected</li>
        <li>(3.3) how TAO may be staked/unstaked from a subnet</li>
        <li>(3.4) the way that emissions are issued to users</li>
        <li>(3.5) the halving schedule</li>
      </ul>
      <p>
        Additionally, there are several (less immediately relevant) technical sections reserved for
        the appendix.
      </p>
      <h3 className={styles.subtitle}>Section 3.1: Subnet AMMs</h3>
      <p>
        Each subnet has its own liquidity pool allowing for swaps (and hence for price discovery)
        between the subnet&#45;specific tokens and TAO. These pools will conduct swaps using the
        standard <em>constant product</em> AMM, and for this reason, we include a quick reminder of
        the relevant mechanics.
      </p>
      <p>
        In a constant product AMM, we consider a liquidity pool containing two types of tokens, and
        we denote the total reserves of each by <InlineMath>{'x'}</InlineMath> and{' '}
        <InlineMath>{'y'}</InlineMath>. Swaps are conducted in a way to maintain the product of the
        token reserves. So, for a swap that changes the reserves by amounts{' '}
        <InlineMath>{'(\\Delta x, \\Delta y)'}</InlineMath>, we must satisfy{' '}
        <InlineMath>{'(x + \\Delta x)(y + \\Delta y) = xy'}</InlineMath>. This results in the viable
        states forming a hyperbola given by the equation <InlineMath>{'xy = L^2'}</InlineMath>, for
        some liquidity constant <InlineMath>{'L'}</InlineMath>. Moreover, the spot price{' '}
        <InlineMath>{'p'}</InlineMath> (i.e. the exchange rate{' '}
        <InlineMath>{'\\Delta y/\\Delta x'}</InlineMath>) is given by the ratio of the reserve
        amounts
        <InlineMath>{'p = y/x'}</InlineMath>, These relationships, in turn allows us to write the
        reserves as <InlineMath>{'x = L/\\sqrt{p}'}</InlineMath> and{' '}
        <InlineMath>{'y = L\\sqrt{p}'}</InlineMath>.
      </p>
      <p>
        New liquidity can be added to a constant product pool in any number of ways. However, if we
        wish to maintain the current spot price, then the added amounts
        <InlineMath>{'(\\Delta x, \\Delta y)'}</InlineMath>
        must, themselves, be at the ratio of the current spot price. In other words, if we have
        <InlineMath>{'y/x = p'}</InlineMath> and{' '}
        <InlineMath>{'\\Delta y / \\Delta x = p'}</InlineMath> , then we will also have
        <InlineMath>{`\\frac{y + \\Delta y}{x + \\Delta x} = p
    `}</InlineMath>
      </p>
      <p>
        {' '}
        For the remainder of this document, the subscript <InlineMath>{'i'}</InlineMath> will be
        reserved for the
        <InlineMath>{'\\ i^{th} '}</InlineMath> subnet, and we will denote the subnet pool reserves
        by the following:
      </p>
      <Equations
        equNo={1}
        minify={true}
        equ={`\\alpha_i = \\text{amount of subnet token in the pool} `}
      />
      <Equations
        equNo={2}
        minify={true}
        equ={`\\tau_i = \\text{amount of TAO token in the pool}`}
      />
      <p>
        Indeed, we will sometimes informally refer to the subnet token simply as "Alpha." Moreover,
        we let <InlineMath>p_i</InlineMath> denote the subnet token price denominated in TAO (i.e.,
        the conversion from Alpha to TAO value). Then, from our discussion of the constant product
        AMM, we may write:
      </p>
      <Equations
        equNo={3}
        equ={`
          p_i = \\frac{\\tau_i}{\\alpha_i}
        `}
      />
      <h3 className={styles.subtitle}>Section 3.2: Injections</h3>
      <p>
        At each block, an amount of Alpha and TAO tokens are to be added to the pool reserves. We
        call this an <em>injection</em>, and we denote these injection quantities by{' '}
        <InlineMath>{'(\\Delta \\alpha_i, \\Delta \\tau_i)'}</InlineMath>. These injection amounts
        are meant to reflect the relative performance of the subnets.There is no correct way to do
        this, but it is natural to imagine the subnet tokens as being speculative instruments, such
        that their prices correlate with subnet performances. We make the choice to define the
        subnet emissions along this line of reasoning.
      </p>
      <p>
        To be precise, we fix a certain amount of total TAO to be emitted at each block, denoted{' '}
        <InlineMath>{'\\Delta \\overline{\\tau}'}</InlineMath>, and we begin with the idea that this
        quantity should be divided among the subnets in proportion to their Alpha prices. In other
        words, the TAO injection for the <InlineMath>{'i^{th}'}</InlineMath> subnet (
        <InlineMath>{'\\Delta \\tau_i'}</InlineMath>) would be given by the following:
      </p>
      <Equations
        equNo={4}
        equ={`
          \\Delta \\tau_i = \\frac{p_i}{\\sum_j p_j} \\times \\Delta \\overline{\\tau}
        `}
      />
      <p>
        When doing the injection, we don&apos;t want to artificially alter the subnet pool price. In
        other words, we should choose the corresponding Alpha injection (
        <InlineMath>{`\\Delta \\alpha_i`}</InlineMath>) so that it maintains the ratio given in
        expression (`3). Specifically, this means that we must have the relation
        <InlineMath>{`\\Delta \\alpha_{i} = \\frac{\\Delta \\tau_i}{p_i}`}</InlineMath>
        which then results in the following:
      </p>

      <Equations
        equNo={5}
        equ={'\\Delta \\alpha_i = \\frac{1}{\\sum_j p_j} \\times \\Delta \\overline{\\tau}'}
      />

      <p>
        However, we will want to modify this formula based on the following observation:
        <em>
          if the sum of prices
          <InlineMath>{`\\sum p_j`}</InlineMath>
          becomes small, the Alpha injection
          <InlineMath>{`\\Delta \\alpha_i`}</InlineMath>
          can grow without bound.
        </em>
        In order to prevent runaway inflation of the subnet tokens, we modify formula (`5) by
        putting a <em>cap</em> on the amount of Alpha emitted, which we will denote by
        <InlineMath>{`\\Delta \\overline{\\alpha}_i`}</InlineMath> (Note that this cap depends on
        the particular subnet <InlineMath>{`i`}</InlineMath>depending on where the subnet is in its
        halving schedule—see Section 3.5.) Our injection formulas can then be summarized as follows:
      </p>
      <Equations
        equNo={6}
        equ={`\\Delta \\tau_i = \\frac{p_i}{\\sum_j p_j} \\Delta \\overline{\\tau}`}
      />
      <Equations
        equNo={7}
        equ={`\\Delta \\alpha_i = \\min\\left\\{ \\frac{\\Delta \\overline{\\tau}}{\\sum_j p_j}, \\;\\Delta \\overline{\\alpha}_i \\right\\}`}
      />
      <p>We note two immediate consequences from these formulas:</p>
      <Equations equNo={8} equ={`\\sum_i \\Delta \\tau_i = \\Delta \\overline{\\tau}`} />
      <Equations equNo={9} equ={`\\Delta \\alpha_i \\leq \\Delta \\overline{\\alpha}_i`} />
      <p>Thus, we have firm upper bounds on the emission amounts.</p>
      <p>
        The value of <InlineMath>{`\\Delta \\overline{\\tau}`}</InlineMath> is initialized to be 1,
        to maintain the current emission schedule of 1 TAO per block. Similarly, for subnet tokens,
        we will take<InlineMath>{`\\Delta \\overline{\\alpha}_i = 1`}</InlineMath>when each subnet
        is initialized, though all of these values are subject to halve according to the halving
        schedule (again, see section 3.5).
      </p>
      <p>
        It should be noted that the <em>min</em> statement in (`7) has the following effect: when
        the sum of all prices drops below a subnet&#45;specific threshold (i.e., if
        <InlineMath>{`\\sum_j p_j < \\frac{\\Delta \\overline{\\tau}}{\\Delta \\overline{\\alpha}_i}`}</InlineMath>
        ), then less Alpha is emitted than would be needed to maintain pool price, and this drives
        the price of the pool <em>up</em>. This will be a critical feature in the early days of low
        liquidity when pool prices are more sensitive to movement.
      </p>
      <p>
        Our injection formulas from (`6)&#45;(`7) are presented in a way that is meant to be both
        readable and mathematically motivated. However, the way in which these quantities are
        computed in code is actually slightly different (while of course producing the same values).
        For the interested reader, we present the pseudo&#45;code below. The variables{' '}
        <code>tao_in</code> and <code>alpha_in</code> represent the amount of tokens being injected
        into the subnet pool reserves, while the variable <code>alpha_out</code> represents the
        alpha to be emitted to the subnets, specifically as miner incentives and validator
        dividends. (One may note, the <code>inject</code> function not only injects liquidity into
        subnet pools, but it also delivers the <code>alpha_out</code> to the relevant parties.)
      </p>
      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_1_code.png'
          alt='Injection pseudo-code'
          className={styles.image_container_image}
        />
        <p>
          <span className={styles.image_container_caption_no}>Figure 1: </span>
          Injection pseudo&#45;code
        </p>
      </div>
      <p>
        Lastly, it should be noted that when using the injection formulas in practice, we will
        actually use the exponentially weighted moving average (EMA) in place of the actual subnet
        pool price <InlineMath>{`p_i`}</InlineMath>. This is done for several reasons:
      </p>
      <ul className={styles.unorder_list}>
        <li>
          to discourage potential malicious attacks that can operate through price manipulation
        </li>
        <li>
          to smoothen out otherwise volatile price movements that may occur for low liquidity pools.
        </li>
      </ul>

      <h3 className={styles.subtitle}>Section 3.3: Staking/Unstaking</h3>
      <p>
        If a user swaps some TAO on a subnet pool to obtain some subnet token (Alpha), then we say
        that the user stakes the TAO to the subnet. Similarly, if a user redeems their subnet token
        by swapping it for TAO, we say that the user unstakes TAO. Thus, the mechanics of staking
        and unstaking are relatively straightforward, as the rules for a constant product AMM are
        well established.
      </p>
      <p>
        It should be noted, however, that different subnet tokens cannot be exchanged directly with
        each other. For example, if a user wishes to exchange some subnet token
        <InlineMath>{`\\Delta \\alpha_i`}</InlineMath> for other subnet token
        <InlineMath>{`\\Delta \\alpha_j`}</InlineMath>, they must first <em>unstake</em> some TAO
        from subnet <InlineMath>{`i`}</InlineMath> (thereby relinquishing their
        <InlineMath>{`\\Delta \\alpha_i`}</InlineMath>)and then they must <em>stake</em> that TAO on
        subnet
        <InlineMath>{`j`}</InlineMath> to obtain the desired amount
        <InlineMath>{`\\Delta \\alpha_j`}</InlineMath>
      </p>
      <p>
        Lastly, it is worth noting that, unlike in typical AMMs, there will be no fees taken on the
        swaps, as there will be no liquidity providers to claim such fees. In other words, all
        liquidity of Alpha and TAO tokens is provided by the emission process.
      </p>
      <h3 className={styles.subtitle}>Section 3.4: Subnet Emissions</h3>
      <p>
        Whereas validators, miners, and subnet owners were previously rewarded for their
        participation in the network through TAO, they will now be rewarded in Alpha. To accomplish
        this, we will have an additional quantity of Alpha, denoted
        <InlineMath>{`\\Delta \\alpha_i'`}</InlineMath>
        that is emitted at each block, and this quantity is to be divided up and distributed to
        users. For simplicity, we take
        <InlineMath>{`\\Delta \\alpha_i'`}</InlineMath>
        to be equal to the value
        <InlineMath>{`\\Delta \\overline{\\alpha}_i`}</InlineMath>
        (i.e. the maximum amount of Alpha injected into the pool per block, as previously defined).
      </p>
      <Equations equNo={10} equ={`\\Delta \\alpha_i' = \\Delta \\overline{\\alpha}_i`} />

      <p>Then the precise way in which we divide this quantity up can be described as follows:</p>
      <ul className={styles.unorder_list_unique}>
        <div>
          <p>
            The Alpha emitted <InlineMath>{"\\Delta \\alpha_i'"}</InlineMath> is divided up into
            three pieces, according to the ratio 41:41:18.
          </p>
          <li>18% go to subnet owners</li>
          <li>41% go to miners</li>
          <li>41% go to validators</li>
        </div>
        <div>
          <p>Next, we consider the the following quantities:</p>
          <li>
            <InlineMath>{'\\alpha_i^o'}</InlineMath> = the <em>alpha outstanding</em> is the total
            supply of Alpha held by users (not pool reserves).
          </li>
          <li>
            <InlineMath>{'\\tau_0'}</InlineMath> = the total TAO staked in the root subnet.
          </li>
          <li>
            <InlineMath>{'\\gamma'}</InlineMath> = a freely chosen parameter that we call the{' '}
            <i>tao weight</i>, to be discussed momentarily.
          </li>
        </div>
        <div>
          With these quantities, we define something called the <i>root proportion</i>, denoted by
          <InlineMath>{'r'}</InlineMath>, and defined as:
        </div>
        <Equations
          equNo={11}
          equ={`r_i = \\frac{\\gamma \\hspace{0.1pc} \\tau_0}{\\gamma \\hspace{0.1pc} \\tau_0 + \\alpha_i^o}`}
        />
        <div>
          <p>
            We use this ratio <InlineMath>{`\\ r_i`}</InlineMath> to split the validator dividends
            into two separate portions:
          </p>
          <li>
            the fraction (<InlineMath>{'\\ 1 \\text{-}r_i'}</InlineMath>) is given to subnet
            validators
          </li>
        </div>
      </ul>
      <p>This process can be summarized in the following figure:</p>
      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_2.jpg'
          alt='Flow chart of emission quantities'
          className={styles.image_container_image}
        />
        <p>
          <span className={styles.image_container_caption_no}>Figure 2: </span>
          Flow chart of emission quantities
        </p>
      </div>
      <p>
        Now, one may wonder how we actually choose the value of this tao weight parameter. Generally
        speaking, the tao weight is meant to help smoothen the transition to the current DTAO
        design. In particular, there are a few guiding principles and motivations behind this
        parameter:
      </p>
      <ul className={styles.unorder_list}>
        <li>
          We want to eventually transition from the current economic consensus to one fully governed
          by Alpha, but there are dangers in transitioning too quickly. In particular, the shifting
          supply and relatively low liquidity of the subnet tokens will drastically change in the
          first few months, and this can severely upset the existing consensus. However, with the
          tao weight parameter, we can slow this transition by diverting emissions to the root
          network initially, and we can keep the economic weight from shifting too fast. In the
          following figure, we plot the percent ownership of Alpha held by the root validators over
          time. This quantity naturally decreases over time, but the steepness of the curve varies
          depending on the specific chosen value of the tao weight:
        </li>
        <div className={styles.image_container}>
          <img
            src='/images/new_dtao_paper/figure_3.jpg'
            alt='Flow chart of emission quantities'
            className={styles.image_container_image}
          />
          <p>
            <span className={styles.image_container_caption_no}>Figure 3: </span>
            Root percentage ownership of Alpha over time
          </p>
        </div>
        <li>
          Though we do not discuss the consensus mechanism here, we note that the root proportion{' '}
          <InlineMath>{'\\ r'}</InlineMath> is also used to blend the stake weight (used in Yuma
          consensus) between TAO and Alpha. By tuning the value of the tao weight, we effectively
          have some control over the period where consensus transitions from being TAO dominated to
          being Alpha dominated. For example, if we instantly transition to a state of relying
          entirely on the Alpha stake for consensus, then as pools launch with low initial
          liquidity, it would be trivial for validators to try to quickly acquire Alpha and
          manipulate consensus on a given subnet. Thus, the tao weight allows us to prevent
          situations like this.
        </li>
        <li>
          The tao weight can prevent early Alpha holders from receiving absurdly high APYs. For
          example, note that <em>if</em> the tao weight is equal to zero (
          <InlineMath>{`\\gamma = 0`}</InlineMath>
          ), then expression (12) tells us that the root proportion will also be zero (
          <InlineMath>{`r = 0`}</InlineMath>
          ). Thus, from the flow chart above, it is clear that root stakers who do not own Alpha
          will receive <em>no dividends</em> at all in this case. One can easily work out that this
          results in a situation where the initial Alpha holders receive runaway dividends with
          shockingly high APYs (see figure 4).
        </li>
      </ul>
      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_4.jpg'
          alt='Flow chart of emission quantities'
          className={styles.image_container_image}
        />
        <p>
          <span className={styles.image_container_caption_no}>Figure 4: </span>
          APY for early Alpha holders v.s tao weight
        </p>
      </div>

      <h3 className={styles.subtitle}>Section 3.5: Halving Schedule</h3>
      <p>
        The TAO halving schedule will remain unchanged from the previously established halving
        schedule. Specifically, the amount of TAO emitted per block is always equal to the quantity
        <InlineMath>{`\\Delta \\overline{\\tau}`}</InlineMath>, and this quantity is subject to
        halve every time the accumulated TAO supply reaches certain supply thresholds. This causes
        the emission rate to halve roughly every four years (the details are provided in section
        5.2).
      </p>
      <p>
        New Alpha tokens will also follow the same halving schedule, with the emission amount
        <InlineMath>{`\\Delta \\overline{\\alpha}_i`}</InlineMath>
        being halved when reaching the same supply thresholds as TAO does (see appendix). Of course,
        the growth rate of Alpha supply can be up to <i>double</i> the value of
        <InlineMath>{`\\Delta \\overline{\\alpha}_i`}</InlineMath>, due to the fact that we inject
        some Alpha for the pool reserves (which is less than or equal to
        <InlineMath>{`\\Delta \\overline{\\alpha}_i`}</InlineMath>) as well as the emission amount
        (equal to <InlineMath>{`\\Delta \\overline{\\alpha}_i`}</InlineMath>) for the rewards. Thus,
        while the growth rate of Alpha supply will be faster in time than that of the TAO supply,
        all halving events nevertheless occur at the same set of supply thresholds, and therefore
        every token supply approaches the same asymptotic value: <em>21,000,000</em>. The following
        figure demonstrates this:
      </p>
      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_5.jpg'
          alt='TAO and Alpha supply growth over time'
          className={styles.image_container_image}
        />
        <p>
          <span className={styles.image_container_caption_no}>Figure 5: </span>
          TAO and Alpha supply growth over time
        </p>
      </div>
      <p>
        Lastly, we note that as a consequence of the tapering rate of token supply growth, subnets
        that launch sooner in time will benefit from periods of faster liquidity growth in their
        subnet pools. Conversely, subnets that launch later in time must be content with slower
        liquidity growth.
      </p>
    </section>
  );
};

export default Section3;
