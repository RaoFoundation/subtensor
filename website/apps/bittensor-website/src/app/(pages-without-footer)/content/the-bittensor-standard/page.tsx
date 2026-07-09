import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import Image from 'next/image';
import {Suspense} from 'react';
import styles from './page.module.css';

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>The Bittensor standard</p>
          <p className={styles.subtitle} style={{fontSize: '12px'}}>
            TOWARDS P2P COMPUTATIONAL CAPITALISM
          </p>
          <p className={styles.subtitle} style={{fontSize: '12px'}}>
            WRITTEN BY TIMO
          </p>

          <Image
            src='/images/icons/double-tao-logo.svg'
            width={40}
            height={40}
            alt='double tao logo'
          />
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            &quot;There is a very similar pattern that you find in the structure of societies, in
            the structure of companies, and in the structure of computers, and all three are moving
            in the same direction, that is, away from a top&#8208;down structure of a central
            command system, giving the system instructions on how to behave, towards a system that
            is parallel, that is flat, which is a web, in which change moves from the bottom up.
            this is going to happen across all institutions and technical devices, it&#39;s the way
            they work.&quot; &#8208; Nick Land, 1994
          </p>
          <p>
            The evolution of AI follows a similar trajectory. From early algorithms created entirely
            top&#8208;down, human knowledge directly encoded into the system, pre&#8208;defining the
            solution. To today&#39;s deep learning algorithms like language models where humans only
            define the objective function and let the computer search an abstract space of possible
            configurations, discovering the model bottom&#8208;up through iteratively adapting
            parameters towards the gradient of the objective. Bottom&#8208;up evolving the model
            towards desired capability, ushering the modern paradigm of machine learning.
          </p>
          <p>
            The field evolved from initially defining the key, to now only defining the lock and
            letting the computer find the key.
          </p>
          <p>
            Throwing energy against a constraint structure without constraining its assembly to fit
            the constraint, over time approximates the optimal configuration. It is a matter of
            adaptive search.
          </p>
          <p>
            This draws an analogy to a principle of nature. The world around us can be viewed as an
            emergent adaptation to a thermodynamic constraint, multiscale evolving towards energy
            gradients of its environment. Constraints set the landscape, energy gradients the
            trajectory through it.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image1.webp'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            Designing our algorithms by this principle has been the core fundamental to the era of
            machine learning and paradigm shifts, like transformers, were driven by optimizing this
            adaptive process, leaving increasingly more space for pattern capture and novelty to
            bottom up emerge from the computer. transformer&#39;s special sauce is contextually
            adaptive neural connectivity.
          </p>
          <p>
            We can find this pattern fractally across nature. From physics, chemistry and biology to
            computers all the way up to linguistics, cultures and economies. Its interscale
            evolution and fundamentally inescapable, solely a matter of adept navigation.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Self&mdash;organization in distributed systems</p>
          <p>
            Nature is inherently distributed, adaptive and parallel. Order in complex systems
            emerges decentrally through a web of bottom&#8208;up interactions organizing around
            local constraints, propagating contextual signals and successful adaptive configurations
            openly scale&#8208;free across the system. Emerging a decentralized collective
            intelligence ordering chaos into coherence. Evolution&#39;s innate gravity towards
            synergy driving local assembly into cooperations, recursively creating levels of
            hierarchical structure, each evolving the level below to their constraints following
            local energy gradients, multiscale aligning adaptive trajectories towards higher order
            organism functioning and fitness. Emerging increasingly complex levels of organization
            in fractal evolution, from cells to companies.
          </p>
          <p>
            This equally describes capitalism, there are deep similarities to market economics.
            There are countless parallels to nature. For instance, slime mold, a classic distributed
            intelligence, allocates resources similarly to capital markets. Excessively diverting
            energy from older pathways to frontiers in search of utility, maximizing reward from its
            environment.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image2.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            Markets are deeply natural and inherently decentralized mechanisms, like an emergent
            nervous and vascular system of civilization. Though central banks and other
            top&#8208;down institutions have corrupted our means of representation, distorting the
            signal our distributed intelligence depends upon for coordination.
          </p>
          <p>
            There is much to ramble about, but let&#39;s get to the plot. In reflection of the great
            utility applying this adaptive bottom&#8208;up search principle to algorithm design
            brought us, it is intriguing to consider what applying it to economic design, whose
            incentives subordinate algorithm design, could yield.
          </p>
          <p>
            In other words, making economic incentives openly programmable on computers to
            adaptively search their configuration space for utility. Since markets are inherently
            distributed, depend on networks and common forms of representation, a protocol makes
            sense. Its substrate maximally native to the nature of a market. Meaning distributed,
            permissionless and bottom&#8208;up adaptive.
          </p>
          <p>
            Thus, we have to look into the realm of distributed and programmable digital currency.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Bitcoin as a p2p market system for hashes</p>
          <p>
            Turns out, the largest amount of computation parallelized towards a common objective
            today by an order of magnitude is Bitcoin, coordinated by distributed programmatic
            incentives. Bitcoin&#39;s unprecedented qualities as a market system driving the
            production of hashes are obscured, as they hold no intrinsic value, only abstractly in
            the security of the ledger.
          </p>
          <p>
            By reaching consensus on a clearly defined methodology to programmatically distribute
            resources as an untainted reward signal for desired output, with unconstrained
            competition and innovation, massively parallelized, Bitcoin created the most efficient
            and antifragile market system known to man.
          </p>
          <p>
            Only final output counts, anyone can compete by any means around the globe fully
            adaptive to their local circumstances, unconstrained local assembly of top&#8208;down
            central coordination, dynamically divisioning the with scale surging organizational
            complexity across many concurrently competing sub&#8208;collectives nested in coherence
            of consensus, currency and aligned incentives. Harnessing human ingenuity,
            competitiveness and globally stranded resources freely. Maverick but competent
            individuals, misfit in traditional structures, can thrive and express their energy
            potential.
          </p>
          <p>
            Emerging companies building specialized hardware, lobbying, marketing and miners
            arbitraging energy costs, bureaucracy and cheap, stranded compute around the globe.
            Driving efficiency of hash production up and cost down, by deeply adapting processes to
            the physical world. Which is the market function scaling material progress into
            societies.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image3.png'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            BTC price cycles stimulate hash&#8208;market expansion and resilience, by oscillating
            between exploration and pruning.
          </p>
          <p>
            The magic happens in Bitcoin as a fungible digital currency, it interoperates the
            virtual protocol economy with the outside world through a shared container for monetary
            energy. That combined with the programmability of inflation distribution inside
            adversarially robust consensus is the foundation for p2p programmable incentives.
          </p>
          <p>
            For the growth flywheel to function the incentives need to connect miners and holders in
            circular value flows, for Bitcoin it&#39;s computationally insured permissionless scarce
            immutable p2p currency.
          </p>
          <p>
            The purity of natural market principles respected in Bitcoin&#39;s self&#8208;securing
            design paired with its ability to directly apply speculative liquidity towards its
            objective in a looped circuit, is what enabled it to reach its mind&#8208;blowing
            computational scales, out of reach for any government.
          </p>
          <p>
            Bitcoin&#39;s incentive mechanism is an artwork of resource coordination the artist set
            to unfold by itself indefinitely. Satoshi defined a self&#8208;perpetuating adaptive
            trajectory evolving the world against the objective of computing hashes to secure
            Bitcoin. The art is the market.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image4.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            In the category of organizational structures, Bitcoin is something new, something
            abstracted that can nest other organizations below, parallelizing them in coherence,
            synchronized under a central heart&#8208;beat, aligning incentives programmatically
            towards a shared objective. An economic machine learning model, bottom&#8208;up adapting
            the physical world towards its objective function.
          </p>
          <p>
            Yet Bitcoin is only a static type of programmable incentive configuration, not built to
            dynamically adapt itself, but everything around it. Just potently beginning to tap into
            the vast possibility space of its paradigm.
          </p>
          <p>
            But to explore this space adaptively, like training a neural network, it first needs to
            be generalized into a unified framework open for exploration, an abstraction over
            Bitcoin&#39;s incentive mechanism that can coherently nest many configurations beneath
            in concurrent evolution, making objectives freely programmable in circular value flows
            with protocol stake.
          </p>
          <p>
            For true generality, the only feasible circular design is giving Stake control over
            weighting the distribution of block rewards between peers. Enabling them to create their
            own incentive mechanisms locally adapted to their needs. Sounds good however, ultimate
            subjectivity is flawed, as the most short&#8208;term profitable strategy for Stake is to
            simply reward itself in isolation, a snake eating its own tail. Making individually
            rational behavior the contrary of collectively rational behavior.
          </p>
          <p>
            We are lacking a consensus to enforce agreement on reward distribution between
            Stakeholders to create global incentive alignment. Yet the more space for adaptation,
            the more useful configurations are possible. Similar to how transformer&#39;s success
            fundamentally builds on enhancing adaptive space. So it is a core design principle to
            constrain local space to adapt minimally.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Yuma Consensus</p>
          <p>
            Bittensor, the trailblazer of this field, answered with Yuma Consensus. Designed for
            massively scalable dynamic intersubjective agreement in adversarial climate, a fuzzy
            consensus to determine probabilistic truth purely from a set of weights and stake.
            Translating local weights into the global chain weights.
          </p>
          <p>
            YC assumes an honest Stake majority and a potentially dishonest minority, while
            dynamically stabilizing Stakeholder incentives in a robust game&#8208;theoretic
            equilibrium.
          </p>
          <p>
            Consensus&#8208;weight is calculated by determining the highest weight supported by at
            least the Stake majority, meaning it assigned either equal or higher weight. Any weights
            set above the consensus&#8208;weight are automatically down&#8208;corrected to
            consensus, while minority lower weights are effectively ignored. Disallowing any
            minority attempt of manipulating miner incentives.
          </p>
          <p>
            The honest majority equilibrium&#39;s robustness is guaranteed through a
            consensus&#8208;based reward proportional to Stake. The deeper in&#8208;consensus, the
            more reward. The weaker consensus, the higher the reward for strengthening consensus,
            while out&#8208;of&#8208;consensus weights come at increasing cost. Creating guarantees
            of honesty being the optimal strategy for majority stake. Upholding market&#8208;wide
            Stake coherence by making global alignment the local self&#8208;interest of
            Stakeholders.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image5.png'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            The high&#8208;level idea of YC is to flexibly enforce majority agreement before
            translating local weights into the global chain weights, incentives. While neutralizing
            manipulation attempts and guaranteeing alignment to the majority is individually
            optimal, in a dynamically stable equilibrium.
          </p>
          <p>
            By constraint to a unifying consensus, Bittensor makes the
            stake&#8208;weights&#8208;reward distribution circuit game&#8208;theoretically viable.
            Laying the foundations for generalized, permissionless, economically scalable
            peer&#8208;to&#8208;peer programmable incentives.
          </p>
          <p>
            Since Yuma Consensus runs purely on a set of weights and stake, it becomes fully
            agnostic to both what is being measured, and the methodology applied for it. Only the
            final numeric weight distribution is passed onchain, stored as a weight matrix,
            separating all computations involved in its calculation from the protocol. Granting
            Validators unconstrained adaptation to local circumstances. Thus, any set of programming
            languages and computing systems can be applied in modularity for building incentive
            landscapes, substrate agnostic.
          </p>
          <p>
            While the protocol is permissionless, Incentive builders are free to locally enforce
            permissions, compliance measures like KYC/KYB or even contractual agreements for
            participation, essential to capture legacy business.
          </p>
          <p>
            Through living at the edge of order and chaos, Yuma became the first adversarially
            robust yet fully substrate&#8208;agnostic consensus mechanism. A vital innovation
            ushering a golden age of unified incentive space exploration.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Bittensor&apos;s market anatomy</p>
          <p>
            Bittensor is a p2p economic computer designed to coherently run incentive applications
            in massive parallelism, looped in a self&#8208;adaptive circuit, continuously
            reconfiguring its energy gradients to adapt the physical world to its objectives and its
            objectives to Stakeholders.
          </p>
          <p>
            Stake as open&#8208;to&#8208;join vested interest, a permissionless ownership monetary
            energy container interoperating the computable protocol economy with the outside world.
            Representing ownership over incentives and what they emerge or more abstractly share in
            the present ability and future potential of the computer to find high utility
            equilibrium between incentive builders and incentive miners.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image6.jpeg'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            Both adaptively searching a match between their configuration spaces evolving in duality
            against constraints of the other side, aligning their adaptive trajectories towards
            convergence, parallelly adapting to the meta constraints of the physical world, in
            coherence of shared liquidity and consensus.
          </p>
          <p>
            The economic model exhibits multiscale isomorphism to machine learning, a
            co&#8208;evolution between incentive builders and incentive miners, resembling model
            architecture and neural optimizer, searching a complex space for dually coherent high
            utility configurations.
          </p>
          <p>
            The abstract market dynamic is a circular game between mining Stake in an objective
            function by measurably approximating it and Stake searching for utility in objective
            function configuration space.
          </p>
          <p>
            Emergent on top a game of applying the resulting output effectively to the physical
            world, forming competing offchain organizations conducting business development, acting
            as real&#8208;world extensions of the protocol.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image7.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            The Torus describes the higher order dynamic of the system well, a continuous adaptive
            flow folding into itself while rewiring its trajectory. Similar to an evolving neural
            network.
          </p>
          <p>
            The circular flow anchored to Stake enables a fluid transformation of speculative
            liquidity into native energy for the TAO economy, dissipated through protocol incentive
            landscapes. Speculation on the objective of an incentive mechanism is transmuted into
            monetary energy gradients, fueling the adaptive assembly of configurations converging
            with the constraint of the objective. Speculative energy on Bittensor&#39;s future is
            diverted effectively to fuel its realization, resembling the phenomenon of
            self&#8208;fulfilling prophecy in its inner function.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Market of markets</p>
          <p>
            When running many subnets of incentive configurations in parallel, something needs to
            determine emission distribution between them, driving competition. Bittensor created a
            root subnet, where Stake currently manually assigns weights, modulated by an older
            variant of YC. A higher instance of consensus weighting the emission flows across YC
            instances nested below.
          </p>
          <p>
            While miners compete around subnet emissions, subnets compete around protocol emissions.
          </p>
          <p>
            Creating a market of markets, pricing the intersubjective value each subnet has to
            Stakeholders, while subnets price the value each miner has to its objective. On whatever
            basis Stake prices, creates the energy gradient subnet developers will adapt towards.
          </p>
          <strong className={styles.bold}>Dynamic TAO Proposal</strong>
          <p>
            This mechanism lacks dynamism, has oligopolistic tendencies and is vulnerable to
            cronyism and collusion. It works well enough for now, but mechanism innovations are
            actively being conceptualized, led by the ‘dynamic TAO&#39; proposal. It deserves and
            will have its own whitepaper, but the high&#8208;level idea is making subnets tradable
            denominated in TAO, allowing an open dynamic market to price their emissions.
          </p>
          <p>
            This makes the process of subnet price discovery open to everyone, not just operators of
            large Validators, while creating vast profit opportunities in applying accurate
            prediction and information asymmetry to the price. This should decentralize and vastly
            increase the amount of intelligence poured into figuring out the appropriate emissions
            for Bittensor&#39;s individual incentive configurations.
          </p>
          <p>
            One of the promises of efficient, collusion resistant subnet pricing is expansion beyond
            Yuma Consensus, allowing subnets to innovate outside its constraints.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Prediction markets as coordination tools</p>
          <p>
            Running on only a set of weights without assumptions, Yuma Consensus is recursively
            composable. Weights can weigh weights, enabling hierarchical layers of prediction, each
            determining probabilistic liquid truth evolving a layer of parallelized YCs below.
            Allowing to break the multiscale complexity of a pricing problem down into layers. Stake
            is global, participating at every level simultaneously.
          </p>
          <p>
            Whenever the metaorganisms need to coordinate itself grows, new instances of Yuma with
            optionally novel incentive structures can be spawned to continuously produce
            probabilistic prediction around a constraint, a liquid truth available as modular
            component across the protocol. Stake&#8208;weighted is just the original archetype of
            Yuma Consensus but can be arbitrarily replaced by e.g. reputation mechanisms, making
            weight in consensus something to be earned through for example historically accurate
            prediction, not bought. So adaptations of YC can be applied from the mining side, not
            limited to Stake.
          </p>
          <p>
            Emissions can be allocated to structure incentives around YC prediction. For instance,
            Bittensor has time&#8208;averaged bonds validators attain in miner&#39;s they weigh,
            making part of weighting a medium&#8208;term directional prediction on a miner&#39;s
            weight.
          </p>
          <p>
            We can expect new types of bonds to emerge over time, potentially in tandem with custom
            scoring and reputation mechanisms, as a response to a need of focusing areas of
            prediction on certain aspects, aligning the collective focus on certain
            time&#8208;horizons or simply increasing adaptability to new information.
          </p>
          <p>
            Producing evaluations or predictions that are composably available to the protocol is
            not limited to YC, there can be prediction subnets of any format. Opening up on the fly
            adaptive emergence of distinct specialization in producing signals for subdimensions of
            the meta coordination.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image8.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            an emergent techno&#8208;capital organism adaptively evolving senses with market
            intelligence to measure and coordinate its own expanding complexity.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Open Opportunity</p>
          <p>
            Another subtle but significant feature of Bittensor is the decoupling of Stake ownership
            and access to development/business opportunity. Stake can freely delegate to accepting
            Validators, giving them control over Stake weight and access bandwidth, allowing them to
            utilize and participate in Bittensor on Stakeholder&#39;s behalf. Validators take a
            currently hard&#8208;coded 18% cut on Stake dividends accrued from setting weights.
          </p>
          <p>
            Meaning anybody with a compelling plan can get bootstrapped, funded and granted
            access&#8208;bandwidth by delegators, by advertising to them competitively. Generally
            speaking, they would transform subnet outputs into higher subjective value or simply
            profit, while providing competitive value distribution back to delegators.
          </p>
          <p>
            Delegator is anyone not running their own Validator, which requires unique
            infrastructure needs per subnet with frequent not always smooth updates as validation
            and mining co&#8208;evolve. Hence most Stakeholders choose to delegate, defaulting into
            a conscious decision of what groups and initiatives represented by individual
            validators, they want to amplify in the protocol economy.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image9.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            Delegation is liquid and can be freely adapted any block, allowing fast feedback loops
            between a delegation&#39;s expectation and reality. Loosening Stake liquidity in the
            protocol considerably, with a constant gravitational pull towards highest subjective
            value&#8208;creating utilization and highest dividends through good weighting,
            stimulating multiscale pricing efficiency and utilization while keeping value flows and
            final control anchored to Stakeholders.
          </p>
          <p>
            The same principle applies to subnets. Anyone able to raise or acquire currently 1000
            TAO to lock, amount shifting based on demand, can launch a subnet and start developing
            an incentive configuration, competing around root subnet weight, judged by Stake.
          </p>
          <p>
            Subnet operators are responsible for creating an incentive landscape aligning miners
            efficiently towards their objective, an ongoing and usually rigorous R&D effort in the
            open, incentivized through an 18% cut in emissions the root subnet allocates. Possibly a
            significant sum of continuous liquid income.
          </p>
          <p>
            Allowing anyone the opportunity to build a company or career around or within Bittensor.
            Stake is just the final controller and beneficiary, but does not constrain contribution,
            only its evolutionary constraints.
          </p>
          <p>
            Letting change move from the bottom up, open access to opportunity through competition,
            not just in Bittensor&#39;s mining markets but in building incentive mechanisms, turning
            their outputs into products or whatever people come up with. Anyone can accelerate the
            ascension upward Bittensor&#39;s meta adaptive trajectory and get directly rewarded with
            ownership and control. Competency, talent and effort can quickly rise to influential
            positions and get flooded with funding, yet devalued in days when results degrade.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Incentive engineering</p>
          <p>
            Incentive engineering is hard, especially in permissionless thus adversarial climate.
            You are trying to synthesize a coherent collective output, by wielding a volatile
            interplay of web&#8208;scale loose human ingenuity and competitiveness at your
            fingertips. With currently $50m+ a month of liquid rewards distributed to miners across
            subnets, proportionally changing with price
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image10.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            Formally defining the objective is not enough, you need an adversarially robust
            autonomous pricing methodology scoring the relative value of miners continuously.
            Pricing deterministically measurable digital commodities like raw compute is
            straightforward, but when dealing with something probabilistic in nature like machine
            intelligence, any miscalibrations and inefficiencies can quickly blow up in your face.
            There are talented and well&#8208;capitalized teams lurking from the shadows picking
            apart the OpenSource code and models in search of profitable exploits.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image11.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            Yuma Consensus aligns Stakeholder incentives, but aligning miner incentives is an
            ongoing challenge unique to every subnet. While innovation is transferable and can
            compound across them, every subnet needs to invent a tailored algorithmic
            game&#8208;theory aligning miner incentives towards its objective.
          </p>
          <p>
            The holy grail is an incentive landscape where everybody maximally acting in local
            self&#8208;interest culminates in swarm behavior maximally acting in global interest of
            the objective, or in other words minimizing the tension between locally rational and
            globally rational behavior, maximizing synergy. Pricing value not purely on an
            individual level, but in context of the whole.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>peer to peer intelligence markets</p>
          <p>
            This ideal is especially compelling in the realm of AI, Bittensor started off as a p2p
            market system for intelligence prior to generalizing into subnets. Intelligence makes a
            great example to zoom into some emergent dynamics of permissionless incentive markets.
          </p>
          <p>
            Only the final output is measured without assumptions on its origin, leaving full space
            for creative ways of locally optimizing performance and cost. If that means using a
            fine&#8208;tuned OpenAI API, an overseas call center replying to requests manually,
            applying a secret AI breakthrough or all simultaneously is for the free market to
            decide.
          </p>
          <p>
            Every new OpenSource model release, technique, paper or even closed&#8208;source API is
            a potential arbitrage opportunity inside the subnets incentive landscape. Depending on
            miner efficiency, it takes only a few days until a useful OpenSource release is applied
            by the majority. Innovation is constantly sucked into, refined and adapted to the
            subnets objective, because the edge is immediately liquid by achieving better relative
            validation scoring with lower costs, instantly felt in profitability. With scale, this
            kind of system could become an industry&#8208;standard benchmark used to evaluate a
            model&#39;s performance versus what according to market forces is the real state of the
            art.
          </p>
          <p>
            Since technically market actors are just anonymous endpoints generating outputs, an AI
            lab sitting on major private innovations unable to publicize could still directly
            monetize in fit subnets while maintaining secrecy. Outputs can be filtered for sensitive
            information locally. Making such subnets, similar to financial markets, likely one of
            the first places to show indications of major AI advancements long before they become
            public.
          </p>
          <p>
            Validation itself can be a modular stack of AI systems that can backpropagate gradients
            for miners to actively train on. The enormous continuous amounts of outputs, synthetic
            data, generated across miners can be used to build datasets to distill models from the
            aggregate intelligence and serve them back to the subnet recursively refining
            intelligence.
          </p>
          <p>
            Validators can implement a sparse Mixture of Experts, learning a routing model of
            miners, building a map of contextual capability to route requests to the best experts or
            coalitions of them.
          </p>
          <p>
            Every miner is an endpoint, and every endpoint can theoretically nest infinitely more
            endpoints behind it, that can each do the same. Allowing offchain optimized model
            swarms, communicating with Bittensor through a local routing layer. Sharing information
            and optimizing as a collective that can scale mining slots fluidly with bandwidth. The
            possible levels of sophistication, hence market efficiency, are unpredictable.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Market composability</p>
          <p>
            One subnet&#39;s output could be the other&#39;s input. Specialized subnets for compute,
            storage, data scraping, data curation, training, RLHF, and inference all working in
            unison, allowing the protocol to organize like departments of a bigtech corporation, but
            massively parallelized under bottom&#8208;up permeable hierarchies. There are no bounds
            on composability and emergence.
          </p>
          <p>
            Small subnets specializing in helping or complementing larger subnets in some way might
            emerge. The system can dynamically complement itself on the fly. Subnets are by
            necessity OpenSource and developed in the radical open, OS resilience multiplied by
            incentives, unintentionally running massive bounties to discover vulnerabilities.
          </p>
          <p>
            There is an unstoppable innovation arbitrage between subnet builders, in their nature as
            market systems developing open mechanisms in production. Evolutionary progress compounds
            quickly across the protocol driven by market forces. Any successful adaptive
            configurations proving antifragility to a subnet&#39;s mining market over time can be
            forked with confidence as modular components into validation of other subnets. Could be
            a novel game&#8208;theoretic aspect, a single classifier or the whole stack. Invention
            spreads virally.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image12.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            If a subnet builder starts off well and gains market share, but then can&#39;t scale or
            sustain sensible validation, anyone could fork attempting takeover by convincing the
            root market of their ability to carry forward, as miners follow emissions. This
            radically free OpenSource competition accelerates evolution and therefore search through
            incentive configuration space.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>High&mdash;dimensional markets</p>
          <p>
            Digital commodities are native to Bittensor&#39;s digital markets. Machine intelligence
            has many contextual dimensions of value out of bounds with legacy marketplace price
            discovery.
          </p>
          <p>
            Markets are essential as a medium to coordinate distributed production of commodities,
            every action leaves a trace of information guiding actors in their local context and
            stimulates subsequent action, an effect called stigmergy or the invisible hand of the
            market. Dynamically divisioning work and guiding local resource management into global
            bottom&#8208;up emergent order pulling towards equilibrium. Emerging an autocatalytic
            global flow driving the collective forward on aligned trajectory, without direct
            communication or awareness of existence between actors.
          </p>
          <p>
            So for distributed production efficiency of intelligence as a commodity, the market
            needs to act as a medium for stigmergy, the feedback loop velocity sets coordination
            tickrate. Autonomous pricing provides a continuously available wire for stigmergic
            communication, vital for distributed training markets and many other configurations. And
            compellingly, to capture contextual and semantic nuances of intelligence in pricing, you
            need autonomous intelligence itself. Neural pricing for the neural element. Pricing
            methodology = market&#39;s objective function.
          </p>
          <p>
            It seems like the natural evolution of technology, neural markets underlying the neural
            commodity. The market actively learning itself. The multiscale isomorphisms between
            Bittensor&#39;s anatomy and machine learning systems make it intuitive, even Yuma
            Consensus is similar to neural networks inherently probabilistic and adaptive.
          </p>
          <p>YC as market alignment tool</p>
          <p>
            With scale, Bittensor&#39;s incentives become increasingly influential to the larger
            trajectory of humanity and purely demand&#8208;adhering, unstoppable cyber markets could
            have detrimental rippling effects on society. So an important property of Yuma Consensus
            is protocol morality always aligns to Stake majority. As long as immoral motives are in
            minority, they cannot express themselves at all. Which secures Bittensor against immoral
            generative content and other minority issues, Stakeholders decentrally moderate protocol
            outputs. Their financial incentive is aligning Bittensor with the moral majority of
            society. Aligning AI from first principles, the incentives of the markets driving its
            creation.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Computational Capitalism</p>
          <p>
            Modern civilization is plagued by coordination quagmires, the multifaceted complexity of
            our activities and coordination problems for maintaining civilizational stability are
            outgrowing money in its current form as a method of value representation and memory
            model to structure monetary incentives. Money forces to collapse high&#8208;dimensional
            value into a one&#8208;dimensional representation whose information is further
            compromised as a powerful tradeoff for fungibility, failing to capture the exploding
            nuance of economic and societal context. Money is a pre&#8208;information age
            technology. That it&#39;s also corrupted by parasitic institutions does not help.
            Malfunctioning profit incentives and game&#8208;theoretic dilemmas are becoming
            progressively more pervasive to our collective functioning and well&#8208;being. Western
            medicine or the military industrial complex are the most ubiquitous examples.
          </p>
          <p>
            Technological evolution is driving humanity towards contextually adaptive more granular
            forms of value representation, that can measure reality as a computational model able to
            represent an arbitrary amount of measurement dimensionality. Semantic money, or neural
            money, a wider fuzzy notion that will take many forms.
          </p>
          <p>
            Markets can encode more information into money, so money can encode more information
            into markets. It&#39;s the transformer moment for capitalism, contextual sensitivity. It
            should have the potential to enable formation of novel individual and collective paths
            to value, a complete repricing of human activity capturing deeper social and contextual
            nuances suddenly opening many paths of valuable coordination previously out of bounds
            with the lossy value compression of money. Humanity stepping the second foot into the
            information age.
          </p>
          <p>
            This is a predictable but likely generational societal shift that could bring
            potentially major systemic turbulences as powers of current economic paradigm painfully
            fight adaptive pressure or attempt a top&#8208;down controlled transition into a more
            granular representation paradigm ultimately still controlled by their structures, like
            CBDCs, marginally captivating and distorting our bottom&#8208;up emergent
            self&#8208;organization likely similarly leading to coordination dilemmas.
          </p>
          <p>
            Cryptography, distributed ledgers and AI are the substrate for the technologically pure
            implementation of this shift to emerge from, and probably we should be in a rush.
          </p>
          <p>
            Digital currency allows us to already today fluidly bridge parts of our fiat currency
            based economy into a computable environment to structure profit incentives around
            arbitrarily complex contextually sensitive scoring mechanisms, memory models and
            globally adaptively optimized game&#8208;theoretic configurations, starting with digital
            commodities.
          </p>
          <p>
            Allowing this new paradigm of coordination mechanisms to symbiotically coexist and
            evolve with present structures, exchanging energy through fiat trading pairs of the
            coin, functioning as a computable extension of capitalism. Enabling at least
            technologically a smooth transition towards full embracement of distributed
            computational economics.
          </p>
          <p>
            Humanity is in a complexity crisis, a constant race between exploding complexity and our
            competency mechanisms trying to keep up, while themselves expanding their complexity.
            Clearly, we are currently lagging behind, which could eventually cascade into the
            collapse of globalized systems. Hence Bittensor&#39;s promise of computational markets
            with hyper&#8208;dimensional pricing and algorithmic game&#8208;theory is big, a leap
            toward a more information&#8208;centric society.
          </p>
          <p>
            The platform for distributed incentive computing is here, now it&#39;s up to the market
            to discover revolutionary configurations on top, a rigorous evolutionary process. The
            bet is desired solutions are within the possibility space, and it&#39;s merely a matter
            of adaptive search and scale.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image13.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            By the state of current incentive configurations, imagining that one day this framework
            might solve immensely complex and nuanced market coordination problems seems utopian,
            but so did the idea of intelligent conversation partners to be discovered within the
            configuration space of neural networks before mega&#8208;scale multi&#8208;year adaptive
            search.
          </p>
          <p>
            Fundamentally Bittensor is a thermodynamic system, capturing and dissipating free energy
            in search of maximum entropy producing co&#8208;adaptations between itself and the
            physical world, minimizing energy potentials, establishing flows with external energy
            gradients, accumulating energy into its body to maintain order amidst expanding
            complexity, indefinitely perpetuating its exploratory adaptive trajectory, anchored to
            Stake.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/the_bittensor_standard/image14.gif'
              alt='schematic of the bittensor network'
              className={styles.image}
            />
          </div>
          <p>
            It is impossible to predict how the system has reconfigured itself or what outcomes it
            serves 5&#8208;10 years from now, impossible to predict the volume of global
            human&#8208;capital&#8208;machine resources under its control. While as of writing this
            admin keys to the chain are still in control of a centralized multisig inside the
            Rao Foundation, once renounced the system is free to scale towards being
            virtually unstoppable, similar to Bitcoin.
          </p>
          <p>
            Anyone inside of it is replaceable, everything is a market. Hierarchies are
            bottom&#8208;up permeable, anyone moving in misalignment to the larger flow and
            objectives is quickly replaced. Yet collectively, Stake is in control, inextricably
            tethering Stakeholders to the meta&#8208;organism. It is hard to predict where its
            markets will reach, but theoretically, anything resource&#8208;seeking measurable by
            computers coupled to an identifier can be reached by its incentives.
          </p>
          <p>
            It is a matter of infrastructure to enable incentive programming around tangible
            commodities, physical labor and processes. Chainlink&#39;s hybrid smart contracts and
            oracle systems, IoT sensor networks and legal automation are promising leaps towards
            scaling Bittensor beyond digital commodities.
          </p>
          <p>
            Obviously there is immense complexity and novel challenges across domains associated
            with all those things, yet to be figured out. But the market engine continuously
            propelling capital times human ingenuity towards it is running and only growing in
            volume and efficiency. There is no reason for pessimism and every reason for optimism.
          </p>
          <p>
            Right now we are still in the early stages of the protocol and infant stages of the
            paradigm, but on a strong trajectory towards ushering a new era of all&#8208;out
            computational capitalism. Augmenting the old economic paradigm to enter the age of
            computers, in order to overcome the turbulences and complexity of the 21st century.
          </p>
        </section>
        <span className={styles.paper_link}>
          <Link
            href='https://timo37.substack.com/p/54af9772-4402-4e0a-8edc-016f5ca6df22'
            isExternal={true}
          >
            Follow this link for the original version
          </Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
