import {WHITEPAPER_URL} from '@/app/config';
import {Link} from '@raofoundation/ui';
import Image from 'next/image';
import {Suspense} from 'react';
import {Equations} from './components/Equations';
import {B_matrix, S_matrix, T_matrix, W_matrix} from './components/utils';
import styles from './page.module.css';

import 'katex/dist/katex.min.css';

// @ts-ignore
import FadeInWrapper from '@/app/components/FadeInWrapper';
import {InlineMath} from 'react-katex';

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>Bittensor: A Peer-to-Peer Intelligence Market</p>
          <p className={styles.subtitle}>Yuma Rao</p>
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
            As with other commodities, markets could help us efficiently produce machine
            intelligence. We propose a market where intelligence is priced by other intelligence
            systems peer-to-peer across the internet. Peers rank each other by training neural
            networks which learn the value of their neighbors. Scores accumulate on a digital ledger
            where high ranking peers are monetarily rewarded with additional weight in the network.
            However, this form of peer-ranking is not resistant to collusion, which could disrupt
            the accuracy of the mechanism. The solution is an incentive mechanism that maximally
            rewards honestly selected weights, making the system resistant to collusion of up to 50
            percent of the network weight. The result is a collectively run intelligence market that
            continually produces newly trained models and pays contributors who create information
            theoretic value.
          </p>

          {/* test equations */}
          {/* <BlockMath>
          {` \\frac{44}{77}`}
        </BlockMath> */}
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>0.1/ Introduction</p>
          <p>
            The production of machine intelligence has come to rely almost entirely on a system of
            benchmarking, where machine learning models are trained to perform well on narrowly
            defined supervised problems. While this system works well for pushing the performance on
            these specific problems, the mechanism is weak in situations where the introduction of
            markets would enable it to excel. For example, intelligence is increasingly becoming
            untethered from specific objectives and becoming a commodity that is (`{1}`) expensively
            mined from data (Schwartz et al. [2019]), (`{2}`) monetarily valuable (OpenAI [2020]),
            (`
            {3}`) transferable (Devlin et al. [2019]), and (`{4}`) generally useful (Radford et al.
            [2019]). Measuring its production with supervised objectives does not directly reward
            the commodity itself and causes the field to converge toward narrow specialists (Chollet
            [2019]). Moreover, these objectives (often measured in uni-dimensional metrics like
            accuracy) do not have the resolution to reward niche or legacy systems, thus what is not
            currently state of the art is lost. Ultimately, the proliferation of diverse
            intelligence systems is limited by the need to train large monolithic models to succeed
            in a winner-take-all competition. Standalone engineers cannot directly monetize their
            work and what results is centralization where a small set of large corporations control
            access to the best artificial intelligence (OpenAI [2020]).
          </p>
          <p>
            A new commodity needs a new type of market (`{1}`). This paper suggests a framework in
            which machine intelligence is measured by other intelligence systems. Models are ranked
            for informational production regardless of the subjective task or dataset used to train
            them. By changing the basis against which machine intelligence is measured, (`{1}`) the
            market can reward intelligence that is applicable to a much larger set of objectives, (`
            {2}`) legacy systems can be monetized for their unique value, and (`{3}`) smaller
            diverse systems can find niches within a much higher resolution reward landscape. The
            solution is a network of computers that share representations continuously and
            asynchronously, peer-to-peer (P2P) across the internet. The constructed market uses a
            digital ledger to record ranks and to provide incentives to peers in a decentralized
            manner. The chain measures trust, making it difficult for peers to attain rewards
            without providing value to the majority. Researchers can directly monetize machine
            intelligence work and consumers can directly purchase it.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>01 Model</p>
          <p>
            We begin with an abstract definition of intelligence Hinton et al. [2015] in the form of
            a parameterized function
            {/* y = f(x) */}
            <InlineMath>{'\\ y = f(x) \\:'}</InlineMath>
            trained over a dataset
            {/* D = [X, Y ] */}
            <InlineMath>{'\\ D = [X, Y] \\:'}</InlineMath>
            to minimize a loss
            {/* L = ED[Q( y, f(x)) )]. */}
            <InlineMath>{'\\ {\\mathcal{L}} = E_{D}[Q(y,f(x))] \\:'}</InlineMath>. Our network is
            composed of n functions
            {/* F = f0, ..., fj , ...fn, */}
            <InlineMath>{'\\ F = f_{0}, ..., f_{j}, ...f_{n} \\:'}</InlineMath>
            ’peers’ where each is holding zero or more network weight
            {/* S = [si ] */}
            <InlineMath>{'\\ S = [s_{i}] \\:'}</InlineMath>
            ’stake’ represented on a digital ledger. These functions, together with losses and their
            proportion of stake, represent a stake-weighted machine learning objective
            {/* Pn i Li ∗ si */}
            <InlineMath>{'\\sum_{i}^{n} {\\mathcal{L}}_{i} * s_{i} \\:'}</InlineMath>
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/whitepaper/figure1.png'
              alt='Peer functions with losses Li and unique datasets Di'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 1 / </span>Peer functions
              with losses
              <InlineMath>{'\\ {\\mathcal{L}}_{i} \\:'}</InlineMath>
              and unique datasets
              <InlineMath>{'\\ D_{i} \\:'}</InlineMath>
            </p>
          </div>
          <p>
            Our goal is the distribution of stake <i>I</i>, as an incentive, to peers who have
            helped minimize the loss-objective (Figure-1), and importantly, in such a way that, it
            is difficult for a small proportion of stake to collude as a means to maximize their
            distribution in the network without minimizing the loss (Figure-3).
          </p>
          <Equations equNo={1} equ={'\\ S_{t+1} = S_{t} + \\tau I'} />
          <p>
            In this paper, we suggest this can be achieved through peer-ranking, where peers use the
            outputs of others
            {/* F(x) = [f0(x)...fn(x)] */}
            <InlineMath>{'\\ F(x) = [f_{0}(x)...f_{n}(x)] \\:'}</InlineMath>
            as inputs to themselves
            {/* f(F(x)) */}
            <InlineMath>{'\\ f(F(x)) \\:'}</InlineMath>
            and learn a set of weights
            {/* W = [wi,j ] */}
            <InlineMath>{'\\ W = [w_{i,j}] \\:'}</InlineMath>
            where peer i is responsible for setting the i th row through transactions on a digital
            ledger.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/whitepaper/untitled_01.png'
              alt='Peer functions with losses Li and unique datasets Di'
              className={styles.image_container_image}
            />
          </div>
          <p>
            Setting weights using an fishers information pruning score LeCun et al. [1989]; Yu et
            al. [2017] in the ranking calculation,
            {/* R = WT · S, */}
            <InlineMath>{'\\ R = W^{T} \\cdot S \\:'}</InlineMath>
            achieves an idealized scoring where each peer’s incentive is equivalent to its pruning
            score: the cost in entropy towards
            {/* Pn i Li ∗ si */}
            <InlineMath>{'\\sum_{i}^{n} {\\mathcal{L}}_{i} * s_{i} \\:'}</InlineMath>
            induced by removing it from the network.
          </p>
          <Equations
            equNo={2}
            minify={true}
            equ={
              '\\ r_{i} \\approx \\frac{1}{n} \\sum_{j}^{n} \\sum_{x \\in D_{j}} \\Delta F^{T}(x)_{i} \\cdot H(Q_{j}(x)) \\cdot \\Delta F(x)_{i}'
            }
          />
          <p>
            However, this approach is not resistant to collusion, where peers vote for themselves,
            notably instead of using (`{2}`), and set weights to enhance their own inflation at the
            expense of the network(Figure-3). This attack is trivial since the digital ledger cannot
            audit the parameters of each model, only the inter-model weights W.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/whitepaper/figure3.png'
              alt='Peer functions with losses Li and unique datasets Di'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 3 / </span>Disjoint cabal:
              peers in the right sub-network only vote for themselves.
            </p>
          </div>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>02 Incentive</p>
          <p>
            We extended the naive ranking method to evade collusion with an ’incentive’ function
            {/* I(W, S) */}
            <InlineMath>{'\\ I(W, S) \\:'}</InlineMath>
            which limits reward to peers that have not reached consensus in the network. Assuming no
            group of peers holds more than the majority of stake in the system, then peers can only
            attain inflation by working to attract votes from the majority: a core assumption in
            many decentralized systems like Bitcoin. Reintroducing our terms, our incentive
            mechanism requires a stake vector S and a set of weights W where rows are inter-peer
            rankings. We also infer a trust matrix T from the weights, where
            {/* ti,j = 1 */}
            <InlineMath>{'\\ t_{i,j} = 1 \\:'}</InlineMath>
            if and only if there is a non-zero edge between peer i and j.
          </p>
          <Equations equNo={3} equ={`\\ ${W_matrix} ${S_matrix} ${T_matrix} `} minify={true} />
          <p>
            We define peers who have reached ’consensus’ as those with non-zero edges from more than
            50 percent of stake in the network. (This is simply the normalized values of
            {/* (T T · S) &gt; 0.5) */}
            <InlineMath>{'\\ (T^{T} \\cdot S) > 0.5'}</InlineMath>
            ). To ensure the mechanism is differentiable we define this computation using the
            continuous sigmoid function. The sigmoid produces a threshold-like scaling that rewards
            connected peers and punishes the non-trusted. The steepness and threshold point can be
            modulated through a temperature ρ and shift term κ.
          </p>
          <Equations equNo={4} equ={`\\ C = \\sigma (\\rho (T^{T}S - \\kappa)) `} />

          <div className={styles.image_container}>
            <img
              src='/images/whitepaper/figure4.png'
              alt='Peer functions with losses Li and unique datasets Di'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 4 / </span>Consensus
              function
              {/* ci = σ(ρ P j tj,isj − κ) */}
              <InlineMath>
                {'\\ c_{i} = \\sigma(\\rho \\sum_{j}^{n} t_{j,i}s_{j} - \\kappa) \\:'}
              </InlineMath>
              with temperature ρ = 10 and shift κ = 0.5. The activation takes the trust scores and
              produces an exponential scaling up to our inflection point where a peer is connected
              to the majority.
            </p>
          </div>
          <p>
            We use the consensus term to scale the original rankings. As peers attain more weight in
            the network they increase their inflation exponentially up to 0.5. In section 10 we show
            how this ensures that the larger of two competing sub-graphs comes to own an
            exponentially larger proportion of the network through inflation.
          </p>
          <Equations equNo={5} equ={`\\ I = R \\cdot C `} />
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>03 Bonds</p>
          <p>
            This consensus described above protects against naive collusion by making it difficult
            for small groups to achieve inflation. However, it does not provide a incentive for
            correctly selecting weights. We introduce these incentives by adapting the inflation
            mechanism with a speculation based reward in the form of ’bonds’ B. Here,
            {/* bi,j ∈ B */}
            <InlineMath>{'\\ b_{i,j} \\in B \\:'}</InlineMath>
            is the proportion of bonds owned by peer i in peer j.
          </p>
          <Equations equNo={6} equ={`${B_matrix} `} />
          <p>
            Bonds accumulate at each step similarly to token inflation where
            {/* ∆B = W · S. */}
            <InlineMath>{'\\ \\Delta B = W \\cdot S \\:'}</InlineMath>
            In this way, peers accumulate bonds in the peers they rank, thus ’bonding’ themselves to
            those that they are connected to.
          </p>
          <Equations equNo={7} equ={`\\ B_{t+1} = B_{t} + W \\cdot S `} />
          <p>
            Using the B bond matrix, the chain redistributes the normal incentive scores
            {/* ∆S = BT · I. */}
            <InlineMath>{'\\ \\Delta S = B^{T} \\cdot I \\:'}</InlineMath>
            Like market based speculation on traditional equities, the peers that have accumulated
            bonds in peers that others will later value attain increased inflation themselves. Thus
            it makes sense for peers to accumulate bonds in peers which it expects to do well
            according to other peers with stake in the system - thus speculating on their future
            value. Finally, we adapt this mechanism slightly to ensure peers attain a fixed
            proportion of their personal inflation. For instance, 50 percent,
            {/* ∆S = 0.5BT I + 0.5I. ∆S */}
            <InlineMath>{'\\ \\Delta S = 0.5B^{T}I + 0.5I. \\: \\Delta S \\:'}</InlineMath>
            becomes the mechanism step update which determines network incentives across the n
            peers.
          </p>
          <Equations equNo={8} equ={`\\ S_{t+1} = S_{t} + \\tau \\Delta S `} />
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>04 Reaching Consensus</p>
          <p>
            The incentive function in Section 2 rewards highly trusted peers, however, it may not
            solve the collusion problem if the honest nodes do not reach consensus. Notably loose,
            unused stake or incorrectly set weights will detract from the inflation proportion of
            honest peers in comparison to a colluding sub-network. The honest network, although
            holding more stake, may not gain enough inflation to overshadow its adversary. The
            dishonest sub-graph need only attain enough inflation to compete with its largest
            competitor, not to entirely dominate the network.
          </p>
          <p>
            This attack is possible when the majority of token inflation is being distributed
            towards peers which are non-majority-trusted in the graph. The chain can measure this
            through a ’loss term’
            {/* L = −R · (C − 0.5) */}
            <InlineMath>{'\\ {\\mathcal{L}} = -R \\cdot (C - 0.5) \\:'}</InlineMath>
            (Figure 7). The term is negative if the majority of inflation is being distributed
            towards peers with more than 0.5 consensus. The chain uses the loss calculation as a
            peg. By increasing the number of weights the average miner sets across the network the
            chain can ensure consensus.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/whitepaper/figure5.png'
              alt='Peer functions with losses Li and unique datasets Di'
              className={styles.image_container_image}
            />
          </div>
          <p>
            <span className={styles.image_container_caption_no}>Figure 5 / </span>The left network
            has low consensus
            {/* L &gt; 0. */}
            <InlineMath>{'\\ {\\mathcal{L}} > 0 \\:'}</InlineMath>
            The system is not resistant to a cabal with less than 50 percent of the stake. The chain
            increases the number of edges set by peers until
            <InlineMath>{'\\ {\\mathcal{L}} < 0\\:'}</InlineMath>. At this point the majority of
            inflation flows to peers with majority consensus.
          </p>
        </section>
        <section className={styles.section_sec5}>
          <p className={styles.subtitle_sec5}>05 Running the Network</p>
          <p>The steps to run a peer in the network are:</p>
          <ol className={styles.list}>
            <li>
              The peer defines its dataset
              {/* Di */}
              <InlineMath>{'\\ D_{i} \\:'}</InlineMath>
            </li>
            <li>
              At each training iteration, the peer conditionally broadcasts batches of examples from
              {/* Di */}
              <InlineMath>{'\\ D_{i} \\:'}</InlineMath>
              to its peers x = [batch_size,sequence_length, input_size].
            </li>
            <li>
              The responses
              {/* F(x) = [...fj (x)...] */}
              <InlineMath>{'\\ F(x) = [...f_{j}(x)...] \\:'}</InlineMath>– each of the common shape
              {/* fj (x) = [batch_size,sequence_length, output_size] */}
              <InlineMath>{'\\ f_{j}(x)\\:'}</InlineMath>= [batch_size, sequence_length,
              output_size] – are joined using the gating function and used as input to the local
              model
              {/* fi */}
              <InlineMath>{'\\ f_{i} \\:'}</InlineMath>
            </li>
            <li>
              Comparison against the target labels produces a loss-gradient
              {/* ∂L ∂F */}
              <InlineMath>
                {'\\ \\frac{\\partial \\mathcal{L}}{\\partial \\mathcal{F}} \\:'}
              </InlineMath>
              which back-propagates through fi and out to the network
            </li>
            <li>
              During 2 and 3 the peers learn the weights for their row
              {/* wi,j ∈ W */}
              <InlineMath>{'\\ w_{i,j} \\in W \\:'}</InlineMath>
              by measuring the value of the signals produced by their peers.
            </li>
            <li>
              At distinct time-step t participants submit changes to the weights
              {/* ∆Wi */}
              <InlineMath>{'\\ \\Delta W_{i} \\:'}</InlineMath>
              to update the ranking R, inflation I, consensus term C, and bond distributions
              {/* δB. */} <InlineMath>{'\\ \\delta B \\:'}</InlineMath>
            </li>
            <li>
              The chain measures ’loss’ and optionally distributes newly minted stake into the
              network
              {/* ∆S */}
              <InlineMath>{'\\ \\Delta S \\:'}</InlineMath>
              according to the bond ownership.
            </li>
          </ol>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>06 Tensor Standardization</p>
          <p>
            A common encoding of inputs and outputs is required for the various model types and
            input types to interact. The use of tensor modalities can be used to partition the
            network into disjoint graphs. At the beginning, the network can be seeded with a single
            modality TEXT, then expanded to include IMAGE, SPEECH, and TENSOR. Eventually,
            combinations of these modalities can be added; for instance TEXT-IMAGE, to bridge the
            network into the multi-modality landscape. Incentives to connect modalities can be
            integrated with the same trust scaling suggested in section (`{2}`). Eventually,
            successful models should accept inputs from any modality and process them into a useful
            representation. For consistency, we can use a standard output shape across the network
            [batch_size, sequence_dim, output_dim] similar to the common tensor-shapes produced by
            language and image models – and extend this size as the network increases in complexity.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/whitepaper/figure6.png'
              alt='Peer functions with losses Li and unique datasets Di'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 6 / </span>Standardization
              of input dimensions within the network
            </p>
          </div>
          <p>
            By working on abstract input classes we can ensure participants work towards a general
            multi-task understanding Kaiser et al. [2017]. Participants may use: (`{2}`) completely
            distinct computing substrates Nugent and Molter [2014], (`{2}`) datasets Lample and
            Conneau [2019], (`{3}`) models, and (`{4}`) strategies for maximizing their incentives
            in the market. It makes sense for peers to work on unsupervised datasets where data is
            cheap and privacy not required.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>07 Conditional Computation</p>
          <p>
            As the network grows, outward bandwidth is likely to become a major bottleneck. The need
            to reduce network transfer and a method of selecting peers is required. Conditional
            computation can be used where peers learn through gradient descent how to select and
            prune neighbors in the network. For example, a product key layer or a sparsely gated
            layer Shazeer et al. [2017].
          </p>
          <Equations equNo={9} equ={'f_{i} = f_{i}(G(x))'} />
          <Equations equNo={10} equ={'G(x) = \\sum_{j} g_{j}(x) * f_{j}(x)'} />
          <p>
            The conditional layer determines a sparse combination of peers to query for each example
            and multiplicatively re-joins them, cutting outward bandwidth by querying only a small
            subset of peers for each example. The method can drastically increase outward bandwidth
            Shazeer et al. [2017] Ryabinin and Gusev [2020], allowing peers to communicate with many
            more neighbors in the graph. In essence, the layer acts as a trainable DNS lookup for
            peers based on inputs. Furthermore, being trainable with respect to the loss, it
            provides a useful proxy for the weights
            {/* wi,j ∈ W. */}
            <InlineMath>{'\\ w_{i,j} \\in W \\:'}</InlineMath>
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>08 Knowledge Extraction</p>
          <p>
            Dependence between functions ensures that models must stay online and cannot be run in
            production. Breaking this dependence can be achieved using distillation Hinton et al.
            [2015]: A compression and knowledge extraction technique in which a smaller model – the
            student - mimics the behavior of the remaining network. The distillation layer is
            employed in conjunction with a conditional computation (10) layer where the student
            model learns to mimic the network using the cross-entropy (shown below as KL) between
            the logits produced by the gating network and the student’s predicted distribution Sanh
            et al. [2020].
          </p>
          <Equations equNo={11} equ={'\\ distillation \\ loss = KL_{D} (dist(x), G(x)'} />
          <p>
            Because the distilled model acts as a proxy for the network, models can be fully taken
            off-line and evaluated. Recursion through the network is also cut between components
            allowing for arbitrary network graphs. If models go offline, their peers can use the
            distilled versions in-place. Private data 6 can be validated over the distilled models
            instead of querying the network. Eventually, components can fully disconnect from the
            network using the distilled models to do validation and inference offline.
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/whitepaper/figure7.png'
              alt='Peer functions with losses Li and unique datasets Di'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 7 / </span>Queries
              propagate to depth=1 before the distilled model is used.
            </p>
          </div>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>09 Learning Weights</p>
          <p>
            Our goal in this work is the production of a ranking
            {/* r = [ri ] */} <InlineMath>{'\\ r = [r_{i}] \\:'}</InlineMath>
            over peers where the score
            {/* ri ∈ R*/} <InlineMath>{'\\ r_{i} \\in R'}</InlineMath>
            represents a participant’s information-theoretic significance to the benchmark.
            Following LeCun and others LeCun et al. [1989]; Yu et al. [2017], it is reasonable to
            define this significance by equating it with the cost of removing each peer from the
            network. We can derive this score analytically where
            {/* ∆F(x)i */}
            <InlineMath>{'\\ \\Delta F(x)_{i} \\:'}</InlineMath>
            is a perturbation of the
            {/* j th */}
            <InlineMath>{'\\ j^{th} \\:'}</InlineMath>
            peers’s inputs when the
            {/* i th */},<InlineMath>{'\\ i^{th} \\:'}</InlineMath>
            peer is removed from the network (Appendix 12.2):
          </p>
          <Equations
            equNo={12}
            minify={true}
            equ={
              '\\ r_{i} \\approx \\frac{1}{n} \\sum_{j}^{n} \\sum_{x \\in D_{j}} \\Delta F^{T}(x)_{i} \\cdot H(Q_{j}(x)) \\cdot \\Delta F(x)_{i} \\\\ \\: \\\\ \\Delta F(x)_{i} = [0, ...0,-f_{i}(x), 0, ...0]'
            }
          />

          <p>
            Note, when the error function
            {/* Qj */}
            <InlineMath>{'\\ Q_{j} \\:'}</InlineMath>
            is the twice-differentiable cross-entropy, then
            {/* H(Qj ) */}
            <InlineMath>{'\\ H(Q_{j}) \\:'}</InlineMath>
            is its Fisher- information matrix, and
            {/* ri ∈ R */}
            <InlineMath>{'\\ r_{i} \\in R \\:'}</InlineMath>
            is suitably measured as each peer’s informational significance to the network as a
            whole. However, information theoretic weights require the full Hessian of the error. In
            practice it is more reasonable to use a heuristic to propagate a contribution score from
            the error function through to the inputs Yu et al. [2017]. For instance, weights from
            the gating layer (Section 6) provide a useful differentiable proxy.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>10 Collusion</p>
          <p>
            We consider the scenario where a subset of the peers in the network have formed a
            ’cabal’: A set of colluding peers attempting to maximize their inflation without
            accurately scoring their neighbors. The fight between the honest graph A with stake{' '}
            <InlineMath>{'\\ S_{A}'}</InlineMath> and the disjoint cabal B with stake{' '}
            <InlineMath>{'S_{B}'}</InlineMath> can be determined by the proportion of network stake
            held by each. The honest graph must attain more inflation to maintain its dominance and
            protect the network
            <InlineMath>{'\\ I_{A} \\gg I_{B}'}</InlineMath>
          </p>
          <p>
            We assume that the proportion of stake in the honest graph is more than that found in
            the dishonest graph
            <InlineMath>{'\\ S_{A} \\gg S_{B} \\:'}</InlineMath>
            and that the chain has reached consensus
            <InlineMath>{'\\ \\mathcal{L} < 0 \\:'}</InlineMath>
            Since all peers in B are disjoint from A, our loss term
            {/* −RB · (CB − 0.5) {`>`} 0 */}
            <InlineMath>{'\\ -R_{B} \\cdot (C_{B} - 0.5) > 0 \\:'}</InlineMath>
            is positive. Because
            <InlineMath>{'\\ \\mathcal{L} < 0 \\:'}</InlineMath>
            it must be the case that
            {/* RA · (CA − 0.5) {`<`} 0 */}
            <InlineMath>{'\\ R_{A} \\cdot (C_{A} - 0.5) < 0 \\:'}</InlineMath>
            is negative and there are peers in the honest sub-graph A who are connected to the
            majority.
          </p>
          <p>
            As the chain progresses, newly minted stake is being emitted at our inflation rate τ in
            proportion to I = R · T. Importantly, the gradient of the incentive function with
            respect to the stake is positive and super-linear at our inflection point between the
            honest and dishonest graph. Notably,
            {/* δI δS = 5 2 */}
            <InlineMath>{'\\ \\frac{\\delta I}{\\delta S} = \\frac{5}{2}'}</InlineMath>, this
            ensures that the amount of stake held by each sub-graph reflects a non-linear change in
            their inflation at the next iteration.
          </p>
          <p>
            Initially, since
            {/* SA {`>`} 0.5 and SB {`<`} 0.5 */}
            <InlineMath>{'\\ S_{A} > 0.5 \\: and \\: S_{B} < 0.5'}</InlineMath>
            the proportion of stake emitted in sub-graph A exceeds that in sub-graph B, and
            sub-graph A’s incentive grows super-linearly compared to B. The result is that the ratio
            of stake
            {/* SB SA+SB */}
            <InlineMath>{'\\ \\frac{S_{B}}{S_{A} + S_{B}} \\:'}</InlineMath>
            decreases – the cabal must continually add stake to its sub-graph to maintain itself
            through time.
          </p>
          <p>
            We consider this proportion between the competing graphs under continuous inflation.
            Converting to python code ...
          </p>
          <div className={styles.image_container}>
            <img
              src='/images/whitepaper/codeblock_1.png'
              alt='Peer functions with losses Li and unique datasets Di'
              className={styles.image_container_image}
            />
            <img
              src='/images/whitepaper/codeblock_2.png'
              alt='Peer functions with losses Li and unique datasets Di'
              className={styles.image_container_image}
            />
          </div>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>11 Conclusion</p>
          <p>
            We have proposed an intelligence market that runs on a P2P network outside of a trusted
            environment. Crucially, the benchmark measures performance as representational knowledge
            production using other intelligence systems to determine its value. The fact that this
            can be done in a collaborative and high-resolution manner suggests that the benchmark
            could provide a better reward mechanism for the field in general.
          </p>
          <p>
            To achieve this aim, the paper began with the definition of a P2P network composed of
            abstractly defined intelligence models. We showed how this framework allowed us to
            produce a ranking for each peer based on the cost to prune it from the network. Peers
            negotiated this score using a set of weights on a digital ledger. However, the system
            was incomplete without mechanisms that prevented participants from forming dishonest
            sub-graphs.
          </p>
          <p>
            To resolve this, we proposed an incentive scheme based on peer connectivity which
            exponentially rewarded peers for being trusted by a large portion of the network. This
            ensured that over time dishonest sub-graphs decay to irrelevance.
          </p>
          <p>
            Following this, we showed (`{1}`) how peers reduced the network bandwidth by learning
            connectivity using a differential layer and (`{2}`) how they could extract fully
            network-disconnected machine learning models to run in production. The result is an
            intelligence market that rewards participants for producing knowledge and making it
            available to new learners in the system.
          </p>
        </section>
        <span className={styles.paper_link}>
          <Link href={WHITEPAPER_URL} isExternal={true}>
            Follow this link for the original version
          </Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
