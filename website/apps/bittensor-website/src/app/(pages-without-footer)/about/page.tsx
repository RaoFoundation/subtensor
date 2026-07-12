import FadeInWrapper from '@/app/components/FadeInWrapper';
import {MenuSchema} from '@/app/components/Header/MenuSchema';
import {Link} from '@raofoundation/ui';
import {Suspense} from 'react';
import styles from './page.module.css';

const CONNECT_LINK_ORDER = ['DISCORD', 'X', 'GITHUB'] as const;
const connectLinks = CONNECT_LINK_ORDER.map((label) =>
  MenuSchema.connect.find((item) => item.label.toUpperCase() === label),
).filter((link): link is (typeof MenuSchema)['connect'][number] => Boolean(link));

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>Bittensor Paradigm</p>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Written by Const, core dev @ Rao Foundation
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Introduction</p>
          <p>
            Bittensor&apos;s vision for a decentralized AI company has captivated a large number of
            people in the arenas of Artificial Intelligence and computer science, who have yearned
            for an alternative to the top-down world being created by our current technology giants.
            However the promise of what Bittensor is and can be has been masked by large technical
            barriers to understanding, and thus has not been well digested by non-technical
            audiences.
          </p>
          <p>
            The purpose of this article is to answer
            <strong> why Bittensor matters to the world, why you should care </strong> and
            <strong> why we need to build it together</strong>. These are core questions, and each
            of them will require that we delve into the history of cryptocurrency, and how Bittensor
            is uniquely positioned to play a significant role in the future that is being written
            today.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Summary</p>
          <p>
            The invention of Bitcoin was a revolutionary moment for humanity because it introduced
            two simultaneously united yet distinct technological innovations, reshaping the way that
            humans organize themselves. Primarily, it was the first ever decentralized currency,
            which gave humanity a shared value system - one that could not be mutated by vested or
            centralized interests. With this, Bitcoin promised a future of fair, anti-fiat, finance.
          </p>
          <p>
            Secondly and to achieve this aim, it birthed the original, disseminated, and
            permissionless digital commodity market. This second point has not been overlooked by
            anyone who has been watching closely - the Bitcoin network&apos;s computational power,
            measured in raw hashing power, has blast past the potential of any company or
            government.
          </p>
          <p>
            Bittensor is essentially a language for writing numerous decentralized commodity
            markets, or &apos;subnets&apos;, situated under a unified token system. These distinct
            markets function through Bittensor&apos;s blockchain, allowing each to interact and join
            into a singular computing infrastructure. By analogy, Bittensor brings the same type of
            abstraction which Ethereum added to Bitcoin for running decentralized contracts, but
            onto Bitcoin&apos;s inverse innovation — <i>digital markets</i>.
          </p>
          <p>
            Compared to Bitcoin and other cryptocurrencies attempting to leverage the digital
            marketplace, Bittensor has built a framework that provides ease for creating these
            viable and enormously powerful systems. However, its genius lies in the fact that every
            one of these inter-networked markets is connectable, and available, to the whole.
            Building a hierarchical web of resources, ultimately culminating in the production of
            intelligence; intelligence leverages computation, which leverages data, which leverages
            storage, then finally leveraging oracles and data procurement and into infinity, all
            within the same ecosystem.
          </p>
          <p>
            This is Bittensor&apos;s overarching vision; directing the power of digital markets
            towards society&apos;s most important digital commodity - Artificial Intelligence. Not
            only to build the most powerful intelligence network, but also to ensure that the
            benefits and <i>the ownership</i> of machine intelligence are in the hands of mere
            mortals. Bottom up, rather than top down.
          </p>
          <p>
            Bittensor&apos;s goal and function of this technological foundation has several
            implications. For developers, a language to write markets for bespoke commodities such
            as compute, allowing them to take advantage of the enormous size, cost-efficiency and
            natural functionality of decentralized market systems. For front-end customers,
            Bittensor offers access to resources at a cheaper price and without intermediary;
            <i>unclosable</i>, not falsely self-proclaimed as “open”. For the Bittensor network as a
            whole, to create Machine Intelligence, in an open and equitable manner on top of ordered
            and dependent sub-markets. And finally - for the world, to ensure that the supremely
            important commodity of intelligence is owned by everyone.
          </p>
          <p>
            Bittensor is, in a similar way, also a computer - albeit one with differing problems
            such as market misalignments - but one that is <i>programmable</i>, with the potential
            to become the most powerful ever created. And, it is a <i>shared</i> computer - not
            directly owned by any single entity, and available for anyone&apos;s use. To run,
            govern, contribute to, and importantly <i>profit</i> from the technological products it
            produces.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Understanding Bittensor</p>
          <p>
            Understanding Bittensor is difficult for the exact same reason that it is powerful. This
            is because the majority of Bittensor&apos;s computational power is not concretely
            defined, but instead abstracted <i>underneath markets</i>. For instance, Bittensor
            leverages machine learning models to produce intelligence products, but Bittensor also
            leverages physical storage to create storage products. Meanwhile, it does
            <i> not define</i> the specifics of how these resources are produced. Instead, Bittensor
            simply defines <i>the market</i> which will reward those commodities for becoming
            available to the network.
          </p>
          <p>
            Abstractions like this are the crucial element to how Bittensor is able to leverage
            enormous swaths of resources: without defining the way in which a computer interacts
            with Bittensor&apos;s markets, the market leaves space for innovation around{' '}
            <i> how </i>
            that problem will be solved. In other words, without directly purchasing or inventing
            the computers that plug themselves into the network, that problem is left to thousands
            of incredibly talented individuals all across the Earth, who achieve that aim in various
            and creative ways.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Bittensor vs Bitcoin</p>
          <p>
            As a good starting point for understanding how the rubber hits the road, we can start by
            comparing Bittensor to Bitcoin. We use Bitcoin as it is the ultimate example of digital
            markets in action, and has the benefit of being well understood by many already.
          </p>
          <p>
            Bitcoin is secured by a game theoretic equilibrium, whereby it is much more profitable
            for miners to accurately and truthfully replicate the state of Bitcoin&apos;s account
            balances, rather than diverge or create false histories. Bitcoin achieves this
            equilibrium by distributing rewards to its network in rough proportion to the quantity
            of compute a network participant brings to the network, thus creating a market for
            compute power.
          </p>
          <p>
            Bitcoin itself, i.e. the code of the protocol which stitches this equilibrium together,
            does not itself run any computers, nor in recent years does it even define the way in
            which computing power proves its existence to the network. Rather, it defines the way in
            which computing power is <i>validated</i> - that is, the manner in which the computers
            in the system <i>agree together</i> who has the most computing power and thus the most
            reward in the system. This is a <strong>digital commodity incentive mechanism</strong>.
          </p>
          <p>
            The effects of this abstraction have dazzled many. Over the last 12 years,
            Bitcoin&apos;s provable computing power has exceeded 1000x times the equivalent
            potential of the trillion dollar company Google, and is set to consume more electricity
            than the entire nation of Australia. But how did Bitcoin achieve this?
          </p>
          <p>
            In summary, digital commodity incentive mechanisms are the <i>perfect markets</i>, and
            perfect markets have the amazing quality that, when aligned, they are unstoppable and
            unequivocally powerful. For instance, Bitcoin miners worldwide continuously and
            aggressively find ways to reduce the cost to mine Bitcoin by purchasing cheap hardware,
            or finding unused lakes of available energy. The system is antifragile, constantly
            killing off bad competition. It is unclosable. Anyone can join, no questions asked, and
            contributions get paid <i>directly</i> and
            <i> continuously</i>, no contracts. No HR department &minus; this is <i>capitalism</i>{' '}
            in its purest form.
          </p>
          <p>
            Bittensor is like Bitcoin in many ways. It has a transferrable and censorship resistant
            token, TAO, which runs on a 24/7 decentralized blockchain substrate which is auditable
            and transparent. Bittensor is also run by miners, like Bitcoin, who can exist globally
            and anonymously.
          </p>
          <p>
            Importantly and conversely, Bittensor utilizes these same advantages of Bitcoin, which
            has incentivized its compute network to grow enormously. For instance, when a market is
            defined on Bittensor, miners worldwide continuously and aggressively find ways at
            reducing the cost to mine TAO on that market; they purchase hardware, stay up all night,
            and find every way to reduce costs. Procuring all available resources whether from their
            own home, or from datacenters worldwide, both known and unknown - all to create the
            digital product of intelligence, or anything and everything the network defines. In
            essence, we leverage the same killer app.
          </p>
          <p>
            Where Bittensor differs primarily is the product. Whilst Bitcoin focuses exclusively on
            the importance of ensuring Bitcoin&apos;s <i>total resistance</i> to exterior actors and
            the immutability of its token economic system, Bittensor focuses primarily on building
            value-creating markets.
          </p>
          <p>
            Focusing here; the Bittensor paradigm is about being a network, computer and language
            for <i>writing systems</i>, alongside the market which created Bitcoin&apos;s gargantuan
            size. However, they fundamentally differ in <i>how</i> they use these resources. Where
            Bitcoin simply secures its network by creating one trivial product (namely SHA-256
            Hashes - which are unusable and not readily transferable into any real problem),
            Bittensor doubles down, allowing the building of multiple intertwined and complementary
            sub incentive systems, which build out commodities, like data or bandwidth, with
            real-world, applicable value, culminating ultimately, in
            <i> intelligence</i>.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Bittensor&apos;s Core Tech</p>
          <p>
            Arguably, Bittensor&apos;s core technological innovation is the <i>separation</i> of the
            chain&apos;s core functioning (transferring funds etc.) from the running of the
            validation systems which define the markets for its digital commodity creation. This is
            distinct from Bitcoin, Ethereum, Filecoin and most every digital commodity system on the
            market today
          </p>
          <p>
            Because of this separation, Bittensor does not need to have a special purpose language
            for writing these validation systems. They can be written in Rust, Python, C++, or any
            other programming language. Furthermore,
            <i> all of the tools required for validating the subnet mechanisms remain off-chain</i>,
            allowing them to be potentially extremely data heavy and compute intensive.
          </p>
          <p>
            For instance, in Bittensor&apos;s flagship commodity market, Subnet 1, intelligence
            providers are validated for answering queries using a machine learning model. Those
            outputs are <i>themselves</i> validated using machine learning models, expensive and
            large ones, which can subsequently learn and be adapted
            <i> without any information moving between the validator set and the chain</i>. This
            would be impossible on nearly any available blockchain.
          </p>
          <p>
            What makes this possible is Yuma Consensus (YC). <strong>Yuma Consensus</strong> is the
            mechanism Bittensor runs on its chain, to enforce that there is agreement between the
            validators of the individual sub mechanisms in Bittensor&apos;s network. By analogy,
            Yuma Consensus is the central processing unit (CPU) of Bittensor. YC takes the varying
            incentive mechanisms written by developers and transforms them into an
            <i> incentive landscape</i>, whereby agreement is reached and miners are forced to act
            in
            <i>
              the way defined by the people who write the consensus mechanisms for the individual
              subnets
            </i>
            .
          </p>
          <p>
            The unique innovation of YC is that it is
            <strong> agnostic to what is being measured</strong>, and allows for fuzzy consensus
            around <i>probabilistic truths</i> - truths like intelligence. Indeed, YC was essential
            for the validation of intelligence, given the nature of that amorphous information
            theoretic commodity.
          </p>
          <p>
            YC is what makes Bittensor fundamentally distinct from various other blockchain projects
            - most of which write their validation systems <i>directly into their chain</i> and thus
            make the problem being solved, or the way it is solved, <strong>immutable</strong> down
            the road. Yuma Consensus has no such limitations.
          </p>
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>Why should business people care?</p>
          <p>
            The ability to <strong>write a multiplicity of Bittensor incentive systems</strong> and
            have them <strong>concurrently run within a single token ecosystem</strong> is not
            <i> just cool</i>, it&apos;s absolutely essential for building decentralized
            organizations which <i>can compete</i> with large corporations. Google has an AI team,
            backed by a storage team, plus a compute team all under one roof - whilst you (without
            Bittensor) are forced to use 10 different token economic markets on 10 different chains.
          </p>
          <p>
            Bittensor gives you all of it: an AI team, a storage team, a compute team, and anything
            else that can be dreamed of - all under a <i>single token framework</i>, turning TAO
            holders into effective Decentralized Cloud Providers,
            <strong> backed by an incentivized, adapting and ever-improving infrastructure</strong>.
            TAO <i>is the bridge</i> between clients and this mega-computer, plus the resources that
            are being created, through various subnetwork mechanisms, in Bittensor. This is the main
            functionality of TAO. Functionality, which hitherto was only within the reach of immense
            supercorps like OpenAI, is now accessible - in one <i>non-permissioned </i> place.
          </p>
          <p>
            Bittensor&apos;s technology was <strong>built for developers primarily</strong>. But,
            just like any Information Technology, Bittensor enables business opportunities. Whilst
            its initial support is vastly driven by a community of technologists, Bittensor&apos;s
            impact will multiply once business professionals also understand the potential of
            decentralizing applications powered by intelligence
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>What&apos;s in it for developers?</p>
          <p>
            The exercise is now to find old applications that are slow, expensive, archaic,
            highly-permissioned or not meeting the required needs, and figure out if they could be
            re-thought via incentive mechanisms, for the same things. Developers must learn about
            digital commodity markets, and ask if old processes can be replaced by open ones.
          </p>
          <p style={{textAlign: 'left', width: '100%'}}>
            If you fall in one of these categories, you are in an excellent position to start diving
            into Bittensor:
          </p>
          <ol className={styles.list}>
            <li> Machine learning engineers</li>
            <li>Intelligence companies</li>
            <li>Trading firms</li>
            <li>Network infrastructure companies</li>
            <li>Data acquisition companies</li>
            <li>Oracles</li>
            <li>Infrastructure providers</li>
            <li>Mining teams for all of the above</li>
            <li>Anyone who wants to have a say in the future of technology.</li>
          </ol>
          <p>
            Bittensor is open to innovators and, for the first time, developers have the opportunity
            to monetize their ideas for the grand resource allocation system <i>that they want</i>.
            Drawing together huge quantities of resources, under incentive driven compute systems -
            <i>without the need to engage in the creation of an entirely new chain</i>. Bittensor
            provides a platform to build these systems in one place. It thus provides a
            one-stop-shop for those seeking{' '}
            <i>
              {' '}
              all the compute requirements for building unstoppable applications on top of an
              incentivized infrastructure.
            </i>
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Open Markets, Open Ownership</p>
          <p>
            At its core, Bittensor is a horizontally scalable, cryptography-based,
            resource-allocation protocol that is also open to contribution. However, what makes it
            so powerful from a technical standpoint is what makes it equally attractive from a
            humanistic perspective. That&apos;s because the inverse of Bittensor&apos;s ability to
            attract resources from the wider world, <strong>is</strong> its ability to distribute
            real <i>visceral ownership</i> back out to that same world.
          </p>
          <p>
            In essence, Bittensor&apos;s permeability to contribution is its permeability to
            control. You can own the network in the same manner I, as a long-term developer of
            Bittensor, can own it. This is a quality of open ownership systems which, like its
            predecessor <i>open source software</i>, reevaluates what <i>makes things good</i>.
          </p>
          <p>
            In open source we give the code away; in open ownership we give the control away, to the
            same effect and from the same principle -
            <strong>
              {' '}
              that which is the most open to contribution and control is the most open to innovation
            </strong>
            , and therefore most likely to align with the largest group of people.
          </p>
          <p>
            This is as simultaneously <strong>vital</strong> - the only thing <i>capable</i> of
            competing with the largest corporations is the <i>unity of everyone </i>- as it is
            <strong> ethical</strong>. The excessive centralization of control, especially around
            AI, poses the greatest risk to the human race. The concentration of power
            <strong> inevitably</strong> creates biased decision-making, controlled access to
            benefits, and significant abuse.
          </p>
          <p>
            Satoshi understood that the centralization of power over money <i>inevitably</i> creates
            corruption. This is the reason that Bitcoin sought to decentralize finance. This truth
            maps self-evidently onto humankind&apos;s most valuable digital resources. AI must not
            be owned by the few for the same, and exponentially more dangerous, threat.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>Conclusion</p>
          <p>
            It is important to revisit that there is often a parallel between what should be for
            technological reasons, and what should be for bigger issues facing the human race.
          </p>
          <p>
            Technologically, Bittensor makes sense;
            <strong>
              {' '}
              abstracted computing markets are the most powerful tool humanity has ever had for
              aligning digital resources.
            </strong>
          </p>
          <p>
            And humanistically,
            <strong>
              {' '}
              open-ownership systems are tools for the bottom-up placing of these resources,
              directly into people&apos;s hands
            </strong>
            . Bittensor is the ecosystem and TAO is the mycelial network connecting humanity with
            the future of Artificial Intelligence, such that its control can be ethically united
            with a life-affirming and truly democratic future for humanity, alongside machines.
          </p>
          <p>
            We are approaching a forking point for mankind; down one road is the centralization of
            power and resources, in large regulated industries, who have entrenched and overbearing
            access to the best-in-class intelligence and computers. Down the other road is the
            <strong>
              {' '}
              potential for sharing these resources through open protocols, via technological
              foundations, which enable global participation and ownership
            </strong>
            .
          </p>
          <p>
            Bittensor finds itself at the precipice of a completely new-age of computing:
            supercomputers via abstracted markets. This precedence shines forth by taking the
            greatest financial innovation our planet has ever seen, then wedding it to the real
            world: Bitcoin meets Artificial Intelligence. We can do it. We have the people, we have
            the desire, and we have the technology.
          </p>
        </section>
        <section className={styles.section}>
          <div className={styles.connect_links}>
            {connectLinks.map((link) => (
              <Link
                key={link.href}
                href={link.href}
                isExternal={link.isExternal}
                className={styles.connect_link}
              >
                <span className={styles.connect_label}>{link.label}</span>
              </Link>
            ))}
          </div>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
