'use client';

import React, {Suspense} from 'react';
import styles from './page.module.css';
import {Link} from '@raofoundation/ui';
import Image from 'next/image';
import {motion} from 'framer-motion';
import {Equations} from '../../../components/MathEquations/MathEquations';
//@ts-ignore
import {InlineMath} from 'react-katex';

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <motion.div
        className={styles.page_container}
        initial={{opacity: 0}}
        animate={{opacity: 1}}
        transition={{duration: 1}}
      >
        <section className={styles.title_section}>
          <p className={styles.paper_title}>
            Dynamic TAO: Bittensor Improvement Template 1 <br />
            (BIT001)
          </p>
          <p className={styles.subtitle}>Opentensor / Datura.ai / Synapse Labs</p>
          <Image
            src='/images/icons/double-tao-logo.svg'
            width={40}
            height={40}
            alt='double tao logo'
          />
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>00/ Abstract</p>
          <p className={styles.abstract_text}>
            In this paper, we introduce &apos;Dynamic TAO&apos; as a pivotal enhancement to the
            Bittensor token system framework. Dynamic TAO represents a strategic refinement in
            Bittensor&apos;s token allocation process. Our primary objective with this proposal is
            to more accurately reward value&dash;add subnets through an innovative, open market,
            decentralized approach. This approach is essential to address and counteract current
            potential issues such as cronyism, apathy, and monopolistic tendencies of root network
            TAO validators who exert significant influence over network emissions. Ultimately, this
            development aims at a more profound decentralization within the blockchain, empowering
            all TAO holders to engage in informed speculation on subnets and actively participate in
            the judicious allocation of Bittensor resources. This methodology promises a more
            dynamic, fair, and efficient progression of the network, aligning with our vision of a
            decentralized future.
          </p>
          <p>
            Upward economic mobility in society, whereby new participants can come to own relatively
            large shares of the overall pie through hard work, is essential to the creation of
            wealth.1 . At the same time, the need to maintain rule&dash;bound property rights is
            essential to the functioning of society and the efficiency of market behaviour. Token
            economic systems like Bittensor share this quality since they tokenize a shared value
            system (&apos;the pie&apos;) that must be able to incentivize new players who can
            continuously grow said pie, while maintaining the assurance to earlier participants that
            value is earned, remains earned and stays undiluted. To strike a balance between these
            two aims, we construct a distinction that arises between the TAO on the coldkey balance,
            and the Dynamic TAO that token holders fluidly stake throughout the ecosystem as they
            speculate on subnets. We call this separation TAO and Dynamic TAO because the latter
            holds the characteristic that it is represented exclusively as TAO, but temporally
            dynamic in terms of it&apos;s instantaneous price relative to TAO. This proposal
            protects the immutable property rights and respective bandwidth allocation to token
            holders while facilitating the advantage that Dynamic TAO rewards active builders and
            contributors to the ecosystem at a higher rate than those that passively hold. We
            propose this change to maximally incentivize valuable subnet development through a more
            liquid and dynamic flow of funds through any TAO holder. This flow will represent, at
            equilibrium, the perception of value for each subnet by all TAO owners. Currently, this
            flow of emission distribution is solely controlled by the root validators, a
            slow&dash;moving group that is not as motivated to allocate funds efficiently. Dynamic
            TAO is a perfect extension to the thesis of the original Bittensor whitepaper, which
            spoke of the need for efficient markets to solve issues of resource allocation in AI.
            Specifically, the solution translates staked TAO (used within subnet incentive
            mechanisms) into a unique, non&dash;fungible token whose exchange rate in TAO fluctuates
            based on demand. Subnets that attract demand by adding value to the ecosystem
            appreciate, whereas those that do not, depreciate. This allows for a dynamic
            market&dash;driven evaluation in which TAO inflation is not exclusively allocated from
            the root network. The design protects the immutability of TAO&apos;s emission schedule
            while extending it with greater powers to extract digital commodities. Subnet creators
            and miners can be more accurately rewarded for their efforts, active TAO holders are
            better rewarded for participating in the liquid flow of emissions, potential for
            economic centralization is drastically reduced, and governmental cronyism is eliminated
            from the original design.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>01/ Background</p>
          <p>
            Bittensor uses incentives to extract digital commodities through a process of emitting a
            distribution of newly minted TAO every block into a set of incentive structures called
            subnets. This distribution we call the emission vector
            <InlineMath>{'\\ E \\rightarrow [S_{1},S_{2},...,S_{n}] \\:'}</InlineMath>[ 1 ]. Within
            each subnet, miners (those that produce value in the form of computational elements like
            intelligence) are rewarded by this emission in such a way that validators, who hold TAO
            and validate the work done by miners can attain that created value [ 1 ].
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/dtao_whitepaper/figure_1.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 1 / </span>Emission of
              newly minted token vector E through subnet incentive mechanisms.
            </p>
          </div>
          <p>
            The Bittensor network currently uses a weighted consensus voting system derived from the
            original Bittensor white paper called Yuma Consensus V1 (YC1) as the means to determine
            the proportion of each
            <InlineMath>{'\\ e_{i} \\ \\epsilon \\ E \\:'}</InlineMath>. YC1 takes as input weights{' '}
            <span className={styles.italic}>W</span> set by peers within Bittensor&apos;s &apos;Root
            Network&apos;, and measures trust <span className={styles.italic}>T</span> and consensus{' '}
            <span className={styles.italic}>C</span> as a protection against adversarial actors who
            simply vote for themselves. However, the system breaks down when honest participants
            cannot form a strong consensus &dash; a quality of the root network, where varying
            viewpoints on what Bittensor requires and who should build it, diverge.
          </p>
          <Equations equNo={1} equ={'\\ E = W^{T}S \\cdot \\sigma (C^{T}S) = R \\cdot T'} />
          <p>Yuma Consensus V1</p>
          <p>
            Furthermore, because of the extreme power centralization of the Root Network we cannot
            be sure this system elicits the truth nor the best outcome for Bittensor as a whole. For
            instance, YC1 does not penalize the misallocation of emission to low&dash;value subnets,
            thus opening up the potential for self&dash;interest, cronyism, or apathy to leak into
            the determination of the emission vector. Even with properly aligned incentives, no
            matter how capable, the small number of root participants cannot compete with
            equilibrium market dynamics, especially as the number of active subnets on Bittensor
            increases. Dynamic TAO seeks to fix this.
          </p>
          <p>
            A solution requires that the emission vector be computed by eliciting market
            participation from the widest number of participants. We achieve this by computing it
            via a set of token exchange pools:
            <InlineMath>
              {'\\ P= [P_{1}(t,\\alpha),P_{2}(t,\\beta)...P_{25}(t,\\omega)] \\:'}
            </InlineMath>
            which TAO holders stake in and out of to receive the dynamic token equivalent for that
            subnet. The pools measure the market equilibrium prices for each dynamic token, based on
            the rate of staking and unstaking, allowing us to calculate an emission vector based on
            each subnet&apos;s steady state price:
          </p>
          <Equations equNo={2} equ={'\\ E = softmax(P_{price})'} />
          <p>
            Emission vector <span className={styles.italic}>E </span> is computed by applying an
            activation function over the current price from each pool.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>02/ Pools</p>
          <p>
            We define Dynamic TAO
            <InlineMath>{'\\ \\alpha_{i} \\:'}</InlineMath>
            as the subnet token used for consensus and mining within subnet
            <InlineMath>{'\\ \\imath \\:'}</InlineMath>
            For each dynamic token the chain implements a liquidity pool
            <InlineMath>{'\\ P = [P_{0},...,P_{n}] \\:'}</InlineMath>, one for each subnet such that
            each pool
            <InlineMath>{'\\ P_{\\imath}(\\tau | \\alpha_{\\imath}) \\:'}</InlineMath>, facilitates
            liquidity between
            <InlineMath>{'\\ \\tau \\:'}</InlineMath>, and the subnet specific Dynamic TAO
            <InlineMath>{'\\ \\alpha_{\\imath} \\:'}</InlineMath>, TAO can only be exchange to and
            from each
            <InlineMath>{'\\ \\alpha'}</InlineMath>, by staking in and out of the subnet which
            enforces that the dynamic tokens are entirely gated by TAO and merely represent a
            non&dash;fungible share of TAO held in the reserve of each pool.
          </p>
          <div className={styles.image_container}>
            <Equations
              equNo={3}
              minify={true}
              equ={`\\ \\underbrace{\\tau}_{\\ TAO \\ held \\ on \\ balance} \\leftrightarrow
              \\underbrace{\\ P_{\\imath}(\\tau | \\alpha_{\\imath})}_{subnet \\ i \\ specific \\ swaping \\ pool} \\leftrightarrow 
              \\underbrace{\\alpha_{\\imath}}_{Dynamic \\ TAO \\ represented \\ as \\ stake \\ in \\ subnet \\ i}
               `}
            />
            <p>
              Staking TAO into a subnet initializes a purchase of the subnet&apos;s dynamic token
            </p>
          </div>
          <p>
            Users interact with Dynamic TAO through Bittensor&apos;s staking operation which
            triggers the pools, either introducing new TAO and withdrawing the dynamic token or vice
            versa. This can be executed by the chain itself requiring no intermediaries. Since all
            pools maintain a constant factor
            <InlineMath>{'\\ \\tau \\times \\alpha = \\kappa'}</InlineMath>,every exchange through
            the pool affects the relative exchange rate of the dynamic token with slippage quadratic
            in reserve supply and given by
            <InlineMath>{'\\ \\frac {\\kappa}{\\tau_{reserve}+\\tau_{added}} \\:'}</InlineMath>
            for <InlineMath>{'\\ \\tau \\:'}</InlineMath> to{' '}
            <InlineMath>{'\\ \\alpha \\:'}</InlineMath> or vice versa.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/dtao_whitepaper/figure_2.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 2 / </span> Dynamic TAO and
              TAO convert through the Uniswap pools via stake and unstake operations injecting the
              TAO and returning <InlineMath>{'\\ \\alpha \\:'}</InlineMath> or vice versa based on
              Uniswaps
              <InlineMath>{'\\ \\tau \\times \\alpha = \\kappa'}</InlineMath> constant factor
            </p>
          </div>
          <p>
            As a consequence of the manner in which the each pool&apos;s exchange rate changes
            through staking operations, the amount of TAO returnable for each unit of TAO staked is
            dynamic, hence the name.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>03/ Coinbase Injection</p>
          <p>
            The problem with typical liquidity pools is that they require substantial reserves to
            facilitate low friction transfers. The known solution to this problem is to allow
            outside individuals to introduce liquidity into the pools at specified price intervals.
            However, this would requires exterior actors to make the system run.
          </p>
          <p>
            To mitigate this, our proposal uses the chain Coinbase to inject token liquidity
            directly, each block, into each pool from both sides. By design, 50 percent of each
            Dynamic Token&apos;s <InlineMath>{'\\ \\alpha_{\\imath} \\:'}</InlineMath>
            added to the pool with the remainder of{' '}
            <InlineMath>{'\\ \\alpha_{\\imath} \\:'}</InlineMath>
            inflation distributed through the subnet&apos;s validation mechanism. Dynamic
            tokens&apos; tokenomics mirrors that of TAO with the exception of a double capped
            supply. We propose this to increase tokens in reserves for increased liquidity and
            decreased slippage when swapping through the pools. Thus, it will be 42 million capped
            supply (with 21 million going into the pool as reserve) and 21 million distributed as
            defined by the consensus mechanism. Therefore, the price of each subnet token at
            equilibrium will be <InlineMath>{'\\ \\frac{1}{n_{subnets}} \\:'}</InlineMath>
            assuming all <InlineMath>{'\\ n \\:'}</InlineMath> subnets have equal valuation.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/dtao_whitepaper/figure_3.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 3 / </span> Liquidity in
              each pool is determined by 4 inflow and outflow rates, the first two of which are
              determined by market participants: 1){' '}
              <InlineMath>{'\\ \\triangle \\tau \\:'}</InlineMath>
              the rate of staking, and and 2) <InlineMath>{'\\ \\Delta \\alpha \\:'}</InlineMath>
              the unstaking rate. The second two are determined by the chain: 3){' '}
              <InlineMath>{'\\ \\Delta \\dot{\\tau} \\:'}</InlineMath>
              the Coinbase injection, and 4) <InlineMath>{'\\ \\dot{\\alpha} \\:'}</InlineMath> the
              dynamic token Coinbase injection. The emission vector determines{' '}
              <InlineMath>{'\\ E = \\dot{\\tau} \\:'}</InlineMath> the rate of
              <InlineMath>{'\\ \\dot{\\tau} \\:'}</InlineMath> injected per block into each pool
              while dynamic token inflation
              <InlineMath>{'\\ \\dot{\\alpha} \\:'}</InlineMath> remains constant at 50 percent of
              that tokens inflation.
            </p>
          </div>
          <p>
            As time progresses (without exterior exchange) the Coinbase injections determine the
            pool&apos;s steady state price since the reserve ratio converges to{' '}
            <InlineMath>{'\\ \\frac{\\dot{\\tau}}{\\dot{\\alpha}} \\:'}</InlineMath>. This can
            easily be seen from the fact that the instantaneous price is given by the ratio of
            reserves, namely:
          </p>
          <Equations equNo={4} equ={'\\ Price_{\\imath} = \\frac{\\dot{\\tau}}{\\dot{\\alpha}}'} />
          <p>
            In this paper we suggest the calculation of the emission vector{' '}
            <InlineMath>{'\\ E \\:'}</InlineMath> via these price calculations. Whereby every block
            the chain calculates
            <InlineMath>{'\\ E \\:'}</InlineMath> and distributes the emission into each pool.
          </p>
          <Equations
            equNo={5}
            equ={
              '\\ {E}_{\\imath} = \\frac{\\exp( Price_{\\imath} )}{\\sum_{\\imath}{\\exp(Price_{\\imath})}}'
            }
          />

          <p>
            One subtle consequence of this is that without exterior interaction (staking /
            unstaking) all subnets converge to exactly
            <InlineMath>{'\\ \\frac{1}{n} \\:'}</InlineMath>TAO per block and{' '}
            <InlineMath>{'\\ \\frac{1}{n} \\:'}</InlineMath> price in TAO. The use of the{' '}
            <InlineMath>{'\\ exp  \\:'}</InlineMath>function dampens prices but could be substituted
            with various others like a shifted <InlineMath>{'\\ relu \\:'}</InlineMath>. In practice
            we will use a moving average of the price calculation to determine the emission vector
            as to reduce the possibility of manipulated price swings effecting the short term
            running of Bittensor. The remainder of this paper explores the effects and additional
            design elements of this token economic adjustment.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>04/ Subnet Initialization and Destruction</p>
          <p>
            Bittensor uses an adaptive Dutch Auction mechanism whereby entrants bid to replace the
            lowest&dash;ranked subnet with their own. When the registration / deregistration occurs,
            the subnet state is cleared and the owner key is replaced with the buyer&apos;s, whose
            purchase price is locked for the duration of the slot.
          </p>
          <p>
            Our design modifies the registration process to use the lowest price over a 30&dash;day
            moving average to select the subnet to deregister. [fig. 4]. When the subnet is
            deregistered, the dynamic tokens associated with that slot are liquidated through the
            pool, effectively initiating a sell operation across the entire supply. Since each
            pool&apos;s price is given by the ratio
            <InlineMath>{'\\ \\frac{\\tau}{\\alpha_{reserve}} \\:'}</InlineMath>
            the conversion rate between TAO and its Dynamic counterpart creates an immediate loss of
            <InlineMath>{'\\ \\frac{\\alpha_{outstanding}}{ \\alpha_{reserve} } \\:'}</InlineMath>.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/dtao_whitepaper/figure_4.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 4 / </span>
              The figure simulates the cost of registering a new subnet. Each doubling corresponds
              to a subnet registration, with the price tailing off with a fixed half&dash;life such
              that the mechanism finds an equilibrium price based on demand
            </p>
          </div>
          <Equations
            equNo={6}
            equ={'\\ S_{deregistered} = \\argmin_{\\imath} \\bar{P}_{\\imath}^{price}'}
          />
          <p>The subnet with the lowest 30 day moving average price is deregistered next.</p>
          <p>
            For the incoming subnet, rather than lock the TAO for the duration of the subnet, the
            creator &quot;buys&quot; the initial supply of the tokens at the same rate as the
            previously deregistered subnet based on the adaptive lock cost as defined above, with 50
            percent of those tokens being used to instantiate the pool and the remainder used to
            bootstrap the consensus mechanism.
          </p>
          <Equations equNo={6} equ={'\\ P( \\tau_{locked} | \\tau_{locked} * \\frac{1}{price} )'} />
          <p>
            The subnet creator attains{' '}
            <InlineMath>{'\\ \\tau_{locked} * \\frac{1}{price} \\:'}</InlineMath> of
            <InlineMath>{'\\ \\alpha_{\\imath} \\:'}</InlineMath> which have an immediate exchange
            rate through pool of
            <InlineMath>{'\\ \\tau_{locked} \\:'}</InlineMath> minus slippage.
          </p>
          <p>
            As block progression begins, 50 percent of dynamic token inflation is added to the pool,
            and the remainder is passed through the incentive mechanism to the network participants
            as usual [ 1 ]. As a result of the token appreciation through validation, the initial
            supply of the subnet token is exponentially more valuable than tokens attained later on.
            By giving subnet creators a large share of the initial supply, this allows us to
            potentially remove the usual 18 percent owner fee assigned in Bittensor&apos;s previous
            version [fig ??].
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>05/ Consensus Weight</p>
          <p>
            Each of Bittensor&apos;s subnets uses a proof&dash;of&dash;stake mechanism called Yuma
            Consensus V2 (YC2) to ensure that subnets can not be manipulated by a small group of
            participants. The security of this mechanism is proportional to the economic value of
            the tokens used.
          </p>
          <p>
            Two issues arise from the proposal in this paper: First, the use of unique stake
            accounts per token rather than a global term, bifurcates Bittensor&apos;s total market
            capitalization across 32 subnets, effectively reducing their economic security by the
            same amount. Second, Bittensor requires a shared sense of consensus weight to facilitate
            inter&dash;subnet exchange, i.e. where the digital commodities produced in one subnet
            are accessible to one another.
          </p>
          <p>
            To alleviate these issues the proposal suggests the use of Global Dynamic TAO, which is
            a key&apos;s total TAO&dash;denominated value across all dynamic tokens. The chain
            enforces that this term attains 50 percent of consensus weight on all subnets.
          </p>
          <div className={styles.image_container}>
            <Equations
              equNo={8}
              minify={true}
              equ={`\\ \\underbrace{S_{jv_{\\imath}}}_{\\text Validator \\ is \\ stake \\ weight \\ on \\ subnet \\ j} =
              \\underbrace{\\frac{\\sum_{k} P_{k}(\\alpha_{\\imath})}{\\sum_{l}\\sum_{k} P_{k}(\\alpha_{l})}}_{Normalized \\ Global \\ Dynamic \\ TAO} + 
              \\underbrace{\\frac{\\alpha_{\\imath}}{\\sum_{l}{\\alpha_{l}}}}_{Normalized \\ Subnet \\ Specific \\ Dynamic TAO}
               `}
            />
            <p>
              The validator&apos;s stake weight on a given subnet is the sum of their total
              TAO&dash;denominated value across all dynamic tokens and their share of that
              subnets&apos; tokens.
            </p>
          </div>
          <p>
            This solves the above concerns since 1) at least 50 percent of subnet consensus power is
            reflected by the entire market cap of TAO making it difficult to dominate any single
            subnet and 2) large holders of Global Dynamic TAO can use their stake weight to bridge
            protocols and interleave Bittensor&apos;s multiple digital commodities
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/dtao_whitepaper/figure_5.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 5 / </span>
              Validators <InlineMath>{'\\ S_{5} \\:'}</InlineMath> has non zero stake weight{' '}
              <InlineMath>{'\\ S_{\\alpha 5} \\:'}</InlineMath> on subnet{' '}
              <InlineMath>{'\\ \\alpha \\:'}</InlineMath> and{' '}
              <InlineMath>{'\\ S_{\\beta 5} \\:'}</InlineMath> on subnet
              <InlineMath>{'\\ \\beta  \\:'}</InlineMath> allowing it to make queries cross
              boundary.
            </p>
          </div>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>06/ Delegation and Governance</p>
          <p>
            Delegation of TAO holds primary importance for Bittensor&apos;s governmental system
            because it facilitates 1) the selection of Senate members who hold veto power against
            Bittensor&apos;s Triumvirate, 2) it acts as a funding mechanism for teams for
            contributing to Bittensor and 3) it incentivizes validators to remain active across
            multiple of Bittensor&apos;s incentive mechanisms. Delegation remains conceptually
            unchanged with Dynamic TAO. Individuals still stake and unstake with validators and
            dividends are accrued to the nominators based on their choice of validator.
          </p>
          <p>
            However, since delegates now attain Dynamic TAO delegations rather than TAO, it is now
            TAO holders that decide the emission allocation. Specifically, delegates cannot swap
            their delegators&apos; Dynamic TAO across subnets. The power remains in the hands of the
            individuals.
          </p>
          <p>
            The effect of this is paramount to the way in which Bittensor is governed since it
            breaks the relationship between the selection of subnets and the delegates, putting this
            power back in the hands of a much more liquid and horizontally scaled market dynamic.
            Further more, this change facilitates the creation of a completely separate political
            entity within the Bittensor ecosystem, which we propose to call &apos;owners&apos;.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/dtao_whitepaper/figure_6.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 6 / </span>
              Three independent entities govern Bittensor via a 2/3 multi&dash;signature style vota
              and veto structure
            </p>
          </div>
          <p>
            Distinct from, but related to, the design changes discussed in this paper, we suggest
            the creation of a 2/3 political governance system involving three distinct parties: 1)
            The Senate, composed of TAO whales selected through the delegation process; 2) The
            Subnet Owners, selected by the open market; and 3) The Triumvirate (known sudo key
            holders).
          </p>
        </section>
        <section className={styles.section_sec5}>
          <p className={styles.subtitle_sec5}>07/ Example</p>
          <p>The following outlines a sequence of events to showcase the proposal.</p>
          <ol className={styles.list}>
            <li>The chain initiates the Dynamic TAO change via a chain upgrade.</li>
            <li>
              All currently active subnets are initialized with an amount of Dynamic TAO equivalent
              to their previous TAO lock, with 50 percent allocated to the pool and the remainder to
              their owner key.
            </li>
            <li>
              Subnet pool reserves are bootstrapped by the initial owner balance and also by the
              gradual movement of staked TAO into subnet tokens through exchange.
            </li>
            <li>
              The chain emission of TAO is distributed based on initially determined prices, and
              injects dynamic tokens at a rate of 7200 per day.
            </li>
            <li>
              Time progresses until a subnet is deregistered. At this point, the tokens in the pool
              are swapped from the respective dynamic token into TAO at a price determined by the
              instantaneous <InlineMath>{'\\ P_{k}  \\:'}</InlineMath> at the time of
              deregistration.
            </li>
            <li>
              The new owner is allocated their fair portion of the new dynamic token based on the
              competitive lock TAO quantity and 30&dash;day moving average price of the previous
              subnet.
            </li>
          </ol>
        </section>
        <section className={styles.section_sec5}>
          <p className={styles.subtitle_sec5}>08/ Analysis</p>
          <p className={styles.subsection_title}>08.1/ Value Analysis</p>
          <p>
            The free floating nature of the each dynamic token&apos;s price measured in TAO opens up
            edge cases where the total market capitalization of dynamic tokens exceeds that of TAO
            outstanding. The reasonable question to ask is whether this change will remove demand
            for TAO in exchange for the subnet tokens themselves.
          </p>
          <p>
            We investigate this question theoretically based the demand theory of value which
            posits:
          </p>
          <p className={styles.image_container} style={{fontStyle: 'italic'}}>
            The price of a good is determined by the interaction of supply and demand in a market
          </p>
          <p>
            We start with the base case where there is only a single subnet token gated by the
            staking operation of TAO. We simplify the terms into three items 1) Dollars{' '}
            <InlineMath>{'\\ A \\:'}</InlineMath> 2) TAO gating token{' '}
            <InlineMath>{'\\ B  \\:'}</InlineMath> and 3) Dynamic tokens{' '}
            <InlineMath>{'\\ C_{\\imath}  \\:'}</InlineMath>.
          </p>
          <div className={styles.image_container}>
            <Equations
              equNo={9}
              minify={true}
              equ={`\\ \\underbrace{A}_{\\ USD \\ Dollar \\ Demand} \\leftrightarrow
                    \\underbrace{B}_{\\ TAO \\ Gating \\ token} \\leftrightarrow
                    \\underbrace{P_{0}(B|C_{0})}_{\\ Swapping \\ Pool} \\leftrightarrow
                    \\underbrace{C_{0}}_{\\ Dynamic \\ token \\ only \\ accessible \\ via \\ B}
               `}
            />
            <p>
              The validator&apos;s stake weight on a given subnet is the sum of their total
              TAO&dash;denominated value across all dynamic tokens and their share of that
              subnets&apos; tokens.
            </p>
          </div>
          <p className={styles.subsection_title}>08.1.1/ Base Case</p>
          <p>
            Hypothesis: The value of B is greater than or equal to the value of{' '}
            <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> when both are priced in A. Given: B can only
            be purchased by units of A and <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> can only be
            purchased by units of B.
          </p>
          <p>
            Proof: By the demand theory of value, the value of B in terms of A is determined by its
            supply and demand from A. The value of <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> is
            dependent on its demand from B. Since
            <InlineMath>{'\\ C_{0}  \\:'}</InlineMath>can only be acquired through B, all demand for{' '}
            <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> must first pass through
            <InlineMath>{'\\ B  \\:'}</InlineMath>, thus its demand is inherently capped by the
            demand for B. Furthermore, if the value of <InlineMath>{'\\ C  \\:'}</InlineMath> were
            to exceed the value of <InlineMath>{'\\ B  \\:'}</InlineMath>, it would imply that{' '}
            <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> is more valuable than the means (B) required
            to obtain it, which is an economic inconsistency.
          </p>
          <p className={styles.subsection_title}>08.1.2/ Inductive Case</p>
          <p>
            Hypothesis: The value of B is greater than the value of
            <InlineMath>{`\\ \\sum_{i}(C_{i}) = [C_{0}, C_{1}, ... C_{n}] \\:`}</InlineMath>
            when all are gated by <InlineMath>{`\\ B`}</InlineMath>. Given that{' '}
            <InlineMath>{`\\ C_{\\imath}`}</InlineMath> can only be purchased by units of B.
          </p>
          <p>
            Proof: For <InlineMath>{`\\ n = 1 `}</InlineMath>, we only have the item{' '}
            <InlineMath>{`\\ C_{0}`}</InlineMath> which can be purchased with B. As we have already
            established, the value of B must be greater than <InlineMath>{`\\ C_{0}`}</InlineMath>
            when priced in A, because B&apos;s value includes its own intrinsic value plus its
            utility in acquiring <InlineMath>{`\\ C_{0}`}</InlineMath>. Therefore, the base case
            holds.
          </p>
          <p>
            Inductive: Assume that the statement is true for<InlineMath>{`\\ n`}</InlineMath> items,
            i.e., the value of B is greater than each of
            <InlineMath>{`\\ C_{1}, C_{2}, \\ldots, C_{n} `}</InlineMath> when priced in A. Now,
            introduce a new item <InlineMath>{`\\ C_{n + 1} `}</InlineMath> which can also only be
            purchased with B.
          </p>
          <p>
            With the introduction of <InlineMath>{`\\ C_{n + 1} `}</InlineMath>, the demand for B
            increases because it is now required to purchase <InlineMath>{`\\ n + 1 `}</InlineMath>{' '}
            items instead of just <InlineMath>{`\\ n `}</InlineMath>. This increased demand for B,
            as the sole means of obtaining <InlineMath>{`\\ C_{n + 1} `}</InlineMath> (and the other
            <InlineMath>{`\\ C `}</InlineMath> items), should increase its value.
          </p>
          <p>
            The value of <InlineMath>{`\\ C_{n + 1} `}</InlineMath>, like the other{' '}
            <InlineMath>{`\\ C`}</InlineMath> items, is capped by the value of B. Since B&apos;s
            value has increased due to the added demand from
            <InlineMath>{`\\ C_{n + 1} `}</InlineMath>, and since
            <InlineMath>{`\\ C_{n + 1} `}</InlineMath> cannot have a value exceeding the means to
            acquire it (B), the value of B remains greater than
            <InlineMath>{`\\ C_{n + 1} `}</InlineMath>.
          </p>
          <p>
            The trend observed from <InlineMath>{`\\ C_{1} `}</InlineMath> to{' '}
            <InlineMath>{`\\ C_{n} `}</InlineMath> continues with{' '}
            <InlineMath>{`\\ C_{n + 1} `}</InlineMath>. The value of B must exceed that of{' '}
            <InlineMath>{`\\ C_{n + 1} `}</InlineMath> to prevent economic anomalies, like
            disproportionate arbitrage opportunities or the illogical situation where the means (B)
            is less valuable than the end (any <InlineMath>{`\\ C_{\\imath} `}</InlineMath>).
          </p>
          <p>
            Since the statement is true for the base case and the inductive step holds, it follows
            that for any number of items{' '}
            <InlineMath>{`\\ C_{1}, C_{2}, \\ldots, C_{n} `}</InlineMath> purchasable only with B,
            the value of B will always be greater than each of these items when priced in A.
          </p>
          <p className={styles.subsection_title}>08.1.3/ Discussion</p>
          <p>
            The proof above does not rule out the possibility that the market capitalizations of all
            dynamic tokens exceeds that of TAO. This is a natural possibility based on Uniswap
            pools, which allow prices to reach infinity. Indeed, this situation is likely, and will
            arise based on the speculated future value of each token. It does not, however mean that
            demand has been removed from TAO, on the contrary it will likely fuel ecosystem demand
            which is gated by TAO.
          </p>
          <p className={styles.subsection_title}>08.2/ Cabal Attack</p>
          <div className={styles.image_container}>
            <img
              src='/images/dtao_whitepaper/figure_7.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 7 / </span>
              Shows the ownership loss (as percentage of original amount) of the dishonest subset as
              it performs the greedy purchase strategy suggested above
            </p>
          </div>
          <p>
            We consider a situation where a subset of the network stakeholders decides to exploit
            the system by manipulating the price of dynamic tokens to gain a larger percentage of
            token emissions. The conflict between the honest subset of participants and the
            dishonest participants can be determined by the network ownership held by each group.
            The honest group must attain a higher proportion of ownership to maintain its dominance
            and protect the network.
          </p>
          <p>
            We assume that the proportion of the network owned by the honest subset is greater than
            that of the dishonest subset. Initially, the chain creates two subnets, dividing the
            network equally between them. We follow the dynamic pool structure as defined earlier.
            The honest group buys tokens in the &apos;good&apos; networks honestly and sells tokens
            in the &apos;bad&apos; networks. Conversely, the dishonest subset buys &apos;bad&apos;
            subnet tokens and sells &apos;good&apos; subnet tokens. We track Global Dynamic TAO over
            time.
          </p>
          <p>
            As described earlier, the chain progresses daily, emitting
            <InlineMath>{`\\ \\tau `}</InlineMath> into the left&dash;hand side of the pools and
            <InlineMath>{`\\ \\alpha `}</InlineMath>
            into the right&dash;hand side. The honest subset buys TAO with alpha (from the dishonest
            subnet) and sells TAO for beta (in the honest subnet). In contrast, the dishonest subset
            buys TAO with beta (from the honest subnet) and sells TAO for alpha (from the dishonest
            subnet). The imbalance of initial funds means the dishonest subnet is diminished over
            time. Figure 8.2 shows this loss
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>09/ Conclusion</p>
          <p>
            The proposal is for an extension to the Bittensor incentive structure which we are
            calling Dynamic TAO. The design makes a demarcation between TAO that is held on balance
            and TAO that is staked for the purpose of attaining consensus power or to extracting
            dividends through validation. The value of Dynamic TAO is captured by the
            value&dash;holding token TAO while still allowing for dynamism in global share. The
            primary result is the removal of Bittensor&apos;s root network as the primary
            determining group for subnet emissions. Further more, it introduces the potential for
            greater economic mobility within Bittensor&apos;s token system without dilution and
            governmental decentralization.
          </p>
          <p>
            To achieve this aim, the paper showcases the singular importance of computing the
            emission vector. We showed how a Uniswap pool structure could be used to facilitate its
            computation through a competitive and speculative mechanism that organically distributed
            tokens through the pools both from Bittensor&apos;s TAO Coinbase, and on the other side
            from each Dynamic Token Coinbase. Prices for each pool are negotiated by staking
            (purchase) and unstaking (sale).
          </p>
          <p>
            Following this, we showed (`1`) how consensus weight is still liquid across the
            ecosystem both for economic security and for inter&dash;subnet communication and (`2`)
            how the proposal introduces true disjoint governance between 3 parties. At the end we
            prove how demand for TAO is not diluted, and investigate how this system is resistant to
            collusion from a less than majority stake weight cabal.
          </p>
        </section>
        <section className={styles.section_sec5}>
          <p className={styles.subtitle_sec5}>References</p>
          <p>
            [ 1 ] Y. Rao, “Bittensor: A peer to peer intelligence benchmark,”
            <span style={{fontStyle: 'italic'}}>arXiv preprint arXiv:1804.07461, 2020</span>
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Appendix</p>
          <p className={styles.subsection_title}>10.1/ Owner Share</p>
          <div className={styles.image_container}>
            <img
              src='/images/dtao_whitepaper/figure_8.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 8 / </span>
              Shows owner&apos;s token balance over the first year without mining, simply through
              holding their own Dynamic Token
            </p>
          </div>
          <p className={styles.subsection_title}>10.2/ Value Analysis</p>
          <p>
            Can this design leak value away from TAO? We investigate this concept from a demand
            theory of value:
          </p>
          <p className={styles.image_container} style={{fontStyle: 'italic'}}>
            The demand theory of value posits:the price of a good is determined by the interaction
            of supply and demand in a market.
          </p>
          <p>
            We investigate this system theoretically, starting with the case that there is only a
            single subnet token gated by the staking operation of TAO. We simplify the terms into
            three items 1) Dollars <InlineMath>{`\\ A`}</InlineMath> 2) TAO gating token{' '}
            <InlineMath>{`B`}</InlineMath> and 3) Dynamic tokens{' '}
            <InlineMath>{`\\ C_{i}`}</InlineMath>
          </p>
          <p>
            Hypothesis: The value of B is greater than or equal to the value of{' '}
            <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> when both are priced in A. Given: B can only
            be purchased by units of A and <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> can only be
            purchased by units of B.
          </p>
          <p>
            Proof: By the demand theory of value, the value of B in terms of A is determined by its
            supply and demand from A. The value of <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> is
            dependent on its demand from B. Since
            <InlineMath>{'\\ C_{0}  \\:'}</InlineMath>can only be acquired through B, all demand for{' '}
            <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> must first pass through
            <InlineMath>{'\\ B  \\:'}</InlineMath>, thus its demand is inherently capped by the
            demand for B. Furthermore, if the value of <InlineMath>{'\\ C  \\:'}</InlineMath> were
            to exceed the value of <InlineMath>{'\\ B  \\:'}</InlineMath>, it would imply that{' '}
            <InlineMath>{'\\ C_{0}  \\:'}</InlineMath> is more valuable than the means (B) required
            to obtain it, which is an economic inconsistency.
          </p>
          <div className={styles.image_container}>
            <Equations
              equNo={10}
              minify={true}
              equ={`\\ \\underbrace{A}_{\\ USD \\ Dollar \\ Demand} \\leftrightarrow
                    \\underbrace{B}_{\\ TAO \\ Gating \\ token} \\leftrightarrow
                    \\underbrace{P_{0}(B|C_{0})}_{\\ Swapping \\ Pool} \\leftrightarrow
                    \\underbrace{C_{0}}_{\\ Dynamic \\ token \\ only \\ accessible \\ via \\ B}
               `}
            />
          </div>
          <p className={styles.subsection_title}>10.2.1/ Base Case</p>
          <p>
            Hypothesis: The value of B is greater than the value of{' '}
            <InlineMath>{`C_{0}`}</InlineMath> when both are priced in A. Given: B can only be
            purchased by units of A. <InlineMath>{`C_{0}`}</InlineMath> can only be purchased by
            units of B.
          </p>
          <p>
            Proof: For By the demand theory of value, the value of B in terms of A is determined by
            its supply and demand from A. The value of <InlineMath>{`C_{0}`}</InlineMath> is
            dependent on its demand from B. Since <InlineMath>{`C_{0}`}</InlineMath> can only be
            acquired through B, all demand for <InlineMath>{`C_{0}`}</InlineMath> must first pass
            through <InlineMath>{`B`}</InlineMath>, thus its demand is inherently capped by the
            demand for B. Furthermore, if the value of <InlineMath>{`C`}</InlineMath> were to exceed
            the value of <InlineMath>{`B`}</InlineMath>, it would imply that{' '}
            <InlineMath>{`C_{0}`}</InlineMath> is more valuable than the means (B) required to
            obtain it, which is an economic inconsistency.
          </p>
          <p>
            As such, under perfect market efficiency if <InlineMath>{`C_{0}`}</InlineMath> were
            valued more than B, it would lead to an unsustainable situation where everyone would
            prefer to trade B for <InlineMath>{`C_{0}`}</InlineMath>
            {` `}
            directly, ignoring the intrinsic value of B leading to an equilibrium price drop to
            match this discrepancy.
          </p>
          <p className={styles.subsection_title}>10.2.2/ Inductive Case</p>
          <p>
            Hypothesis: The value of B is greater than the value of{' '}
            <InlineMath>{` \\sum_{i}(C_{\\imath}) = [C_{0}, C_{1}, ...C_{n}]`}</InlineMath>
            when all are gated by <InlineMath>{`B`}</InlineMath>. Given that B can only be purchased
            by units of A. <InlineMath>{`C_{\\imath}`}</InlineMath>
            can only be purchased by units of B and the value of B in terms of A is initially set as
            the supply of B times the price of B in A.
          </p>
          <p>
            Proof: For <InlineMath>{`n=1`}</InlineMath>, we only have the item{' '}
            <InlineMath>{`C_{0}`}</InlineMath> which can be purchased with B. As we have already
            established, the value of B must be greater than <InlineMath>{`C_{0}`}</InlineMath> when
            priced in A, because B&apos;s value includes its own intrinsic value plus its utility in
            acquiring <InlineMath>{`C_{0}`}</InlineMath>. Therefore, the base case holds.
          </p>
          <p>
            Inductive Step: Assume that the statement is true for <InlineMath>{`\\ n `}</InlineMath>{' '}
            items, i.e., the value of B is greater than each of{' '}
            <InlineMath>{`C_{1}, C_{2}, \\ldots, C_{n} `}</InlineMath> when priced in A. Now,
            introduce a new item <InlineMath>{`\\ C_{n + 1}`}</InlineMath> which can also only be
            purchased with B.
          </p>
          <p>
            With the introduction of <InlineMath>{`\\ C_{n + 1}`}</InlineMath>, the demand for B
            increases because it is now required to purchase <InlineMath>{`\\ n + 1`}</InlineMath>{' '}
            items instead of just <InlineMath>{`\\ n `}</InlineMath>. This increased demand for B,
            as the sole means of obtaining <InlineMath>{`\\ C_{n + 1}`}</InlineMath> (and the other
            <InlineMath>{`\\ C`}</InlineMath> items), should increase its value.
          </p>
          <p>
            The trend observed from <InlineMath>{`\\ C_{1} `}</InlineMath> to{' '}
            <InlineMath>{`\\ C_{n} `}</InlineMath> continues with{' '}
            <InlineMath>{`\\ C_{n + 1} `}</InlineMath>. The value of B must exceed that of{' '}
            <InlineMath>{`\\ C_{n + 1} `}</InlineMath> to prevent economic anomalies, like
            disproportionate arbitrage opportunities or the illogical situation where the means (B)
            is less valuable than the end (any <InlineMath>{`\\ C_{\\imath} `}</InlineMath>).
          </p>
          <p>
            Since the statement is true for the base case and the inductive step holds, it follows
            that for any number of items
            <InlineMath>{` \\ C_{1}, C_{2}, \\ldots, C_{n} `}</InlineMath> purchasable only with B,
            the value of B will always be greater than each of these items when priced in A. This
            conclusion hinges demand theory of value.
          </p>
        </section>
        <span className={styles.paper_link}>
          <Link
            href='/pdfs/dtao_whitepaper/Dynamic_TAO_Bittensor_Improvement_Template_1.pdf'
            isExternal={true}
          >
            Follow this link for the original version
          </Link>
        </span>
      </motion.div>
    </Suspense>
  );
};

export default page;
