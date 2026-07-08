import {Link} from '@raofoundation/ui';
import Image from 'next/image';
import {Suspense} from 'react';
import styles from './page.module.css';
//@ts-ignore
import FadeInWrapper from '@/app/components/FadeInWrapper';
import {InlineMath} from 'react-katex';

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <p className={styles.paper_title}>Stake-Based Consensus for Utility Scoring</p>
          <p className={styles.subtitle}>
            Francois Luus / Jacob Steeves / Ala Shaabana / Yuqian Hu / Sin Tai Liu
          </p>
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
            We formulate a stake-based consensus problem in terms of a two-player game and propose a
            protagonist consensus policy to optimize a Nash equilibrium via a weight reduction
            algorithm with a guarantee of minority stake deterioration. We generalize this to a
            two-team game and propose a smooth density evolution algorithm that outperforms coarser
            estimates. We perform a full-scale Monte Carlo analysis and confirm the accuracy of our
            theoretical results, and show the possibility of a 40% stake + 25% utility attack. The
            result is a variable-expense consensus algorithm that can be fit to blockchain compute
            constraints to reach accurate consensus in adversarial settings.
          </p>
        </section>
        <section className={styles.section}>
          <p className={styles.subtitle}>01/ Stake-based weight consensus</p>
          <p className={styles.subsection_title}>1.1 Problem definition</p>
          <p>
            We consider a two-player game between (protagonist) honest stake
            <InlineMath>{'\\ 0.5< s_{H} \\le 1 \\:'}</InlineMath> and (adversarial) cabal stake (
            <InlineMath>{'\\ 1 - s_{H} \\:'}</InlineMath>) competing for total fixed reward{' '}
            <InlineMath>{'\\ e_H + e_C = 1 \\:'}</InlineMath>with honest emission{' '}
            <InlineMath>{'\\ e_{H}'}</InlineMath> and cabal emission{' '}
            <InlineMath>{'\\ e_{C}'}</InlineMath>, respectively, followed by stake updates{' '}
            <InlineMath>{'\\ s^{\\prime}_{H}= \\frac{{s_{H} + e_{H}}}{2} \\:'}</InlineMath> and{' '}
            <InlineMath>{'\\ s^{\\prime}_{C}= \\frac{{1 - s_{H}+e_{C}}}{2} \\:'}</InlineMath>. The
            honest objective <InlineMath>{'\\ s_{H} \\le e_{H} \\:'}</InlineMath> at least retains
            scoring power <InlineMath>{'\\ s_{H}'}</InlineMath> over all action transitions in the
            game, otherwise when <InlineMath>{'\\ e_{H} \\le s_{H} \\:'}</InlineMath> honest
            emission will erode to 0 over time, despite a starting condition of{' '}
            <InlineMath>{'\\ 0.5 < s_{H}'}</InlineMath>.
          </p>
          <p>
            We assume honest stake sets objectively correct weights{' '}
            <InlineMath>{'\\ w_{H} \\:'}</InlineMath> on itself, and{' '}
            <InlineMath>{'\\ 1 - w_{H} \\:'}</InlineMath> on the cabal, where honest weight
            <InlineMath>{'\\ w_{H} \\:'}</InlineMath>
            represents an ongoing expense of the honest player, sustained throughout the game.
            However, cabal stake has an action policy that freely sets weight{' '}
            <InlineMath>{'\\ w_{C} \\:'}</InlineMath> on itself, and{' '}
            <InlineMath>{'\\ 1 - w_{C} \\:'}</InlineMath> on the honest player, at no cost to the
            cabal player, with the objective to maximize the required honest self-weight expense{' '}
            <InlineMath>{'\\ w_{H} \\:'}</InlineMath> via
            <InlineMath>
              {'\\ w_{C}^{*}=\\arg \\max_{w_{C}}E[w_H| \\;s_{H}=e_{H}(s_{H},w_{H},w_{C})] \\:'}
            </InlineMath>
            .
          </p>
          <p>
            We then assume the honest majority <InlineMath>{'\\ s_{H}>0.5 \\:'}</InlineMath> can
            counter with a consensus policy <InlineMath>{'\\  \\pi \\:'}</InlineMath>allowed to
            modify all weights modulo player labels, so it is purely based on the anonymous weight
            distribution itself, optimizing the Nash equilibrium{' '}
            <InlineMath>
              {'\\ min_{\\pi}\\max_{w_{C}}E[w_{H}\\;|\\;s_{H}=e_{H}(s_{H},\\pi(\\mathbf{w}))]\\:'}
            </InlineMath>
            .
          </p>
          <p>
            The majority stake enforces an independent and anonymous consensus policy{' '}
            <InlineMath>{'\\ \\pi \\:'}</InlineMath>(e.g. through a blockchain solution) that
            modifies the weights to minimize the expense <InlineMath>{'\\ w_{H} \\:'}</InlineMath>,
            which has been maximized by the cabal applying an objectively incorrect gratis
            self-weight <InlineMath>{'\\ w_{C} \\:'}</InlineMath>. Consensus aims to produce{' '}
            <InlineMath>
              {'\\ \\pi( \\mathbf{w}) \\rightarrow (w^{ \\prime}_{H}, w^{\\prime}_{C}) \\:'}
            </InlineMath>{' '}
            so that <InlineMath>{'\\ w^{\\prime}_{C}=1-w^{\\prime}_{H} \\:'}</InlineMath>, by
            correcting the error{' '}
            <InlineMath>{'\\ \\epsilon=w^{\\prime}_C+w^{\\prime}_H-1>0'}</InlineMath>. Note that the
            input cost <InlineMath>{' \\ w_{H} \\:'}</InlineMath> remains fully expensed, and that{' '}
            <InlineMath>{' \\ w^{\\prime}_{H} \\:'}</InlineMath> merely modifies the reward
            distribution that follows, but not knowing which players are honest or cabal (anonymous
            property).
          </p>
          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_1.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 1 / </span>
              Selfish weighting problem: Minority cabal sets{' '}
              <InlineMath>{`\\ w_{C}=1 \\:`}</InlineMath> self-weight to maximally grow its relative
              stake, e.g. at (`1) honest majority stake of{' '}
              <InlineMath>{`\\ s_{H}=0.6 \\:`}</InlineMath> and honest utility of{' '}
              <InlineMath>{`\\ w_{H}=0.75 \\:`}</InlineMath> would require cabal to report
              self-weight <InlineMath>{`\\ w_{C}<0.62 \\:`}</InlineMath> for honest stake to be
              retained. (b) Consensus solution: Stake-based consensus{' '}
              <InlineMath>{`\\ \\eta=3 \\:`}</InlineMath> corrects excessive self-weight of minority
              stake, e.g. at (`2) <InlineMath>{`\\ s_{H}=0.6 \\:`}</InlineMath> ,{' '}
              <InlineMath>{`\\ w_{H}=0.75 \\:`}</InlineMath> no selfish cabal weight can prevent
              honest stake retention, even <InlineMath>{`\\ w_{C}=1 \\:`}</InlineMath> results in
              honest stake ratio gain. Zero-weight problem: Minority cabal is virtually the only
              scoring incentive recipient of the cabal utility reward when its actual utility is
              near-zero, e.g. at (`3) where honest stake deteriorates. (c) Weight trust solution:
              Require the majority stake to agree that a weight is non-zero, otherwise smoothly
              nullify the associated reward to the degree of mistrust, which then removes the honest
              stake deterioration region when <InlineMath>{`\\ w_{H}>0.95 \\:`}</InlineMath>.
              Consensus guarantee: Honest majority stake is retained when{' '}
              <InlineMath>{`\\ s_{H}\\ge 0.6 \\:`}</InlineMath> and{' '}
              <InlineMath>{`\\ w_{H}\\ge 0.75 \\:`}</InlineMath>, despite strategic cabal weight
              setting.
            </p>
          </div>
          {/* image wrapper end */}
          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_2.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 2 / </span>
              Retention line interpretation: (a) Honest incentive share contour plot for{' '}
              <InlineMath>{'\\ s_{H}=0.6 \\:'}</InlineMath>
              only, highlighting where the emission is <InlineMath>{'\\ e_{H}=0.6 \\:'}</InlineMath>
              , e.g. at (`1). However, at (`2) the contour recedes again due to the zero-weight
              problem. (b) Similarly, the specific emission contour plot for{' '}
              <InlineMath>{'\\ s_{H}=0.7 \\:'}</InlineMath>, highlighting the contour where the
              emission is <InlineMath>{'\\ e_{H}=0.7 \\:'}</InlineMath>, which means with inflation
              the honest share ratio of <InlineMath>{'\\ s_{H}=0.7 \\:'}</InlineMath> can be
              retained if honest utility is at least <InlineMath>{'\\ w_{H}>0.75 \\:'}</InlineMath>{' '}
              like at (`3). (c) Retention lines: A compound plot combines all the highlighted{' '}
              <InlineMath>{'\\ s_{H}=e_{H} \\:'}</InlineMath>contours from individual contour plots
              (e.g. <InlineMath>{'\\ s_{H}=0.6 \\ and \\ s_{H}=0.7'}</InlineMath>), to show the
              overall retention profile. Generally, the higher the honest stake, the higher the
              honest utility requirement to retain stake proportion under adversarial weight
              setting.
            </p>
          </div>
          {/* image wrapper end */}
          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_3.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 3 / </span>
              Scoring incentive: A percentage share of the utility rewards equal to stake in a
              score, as incentive to encourage honest scoring. (a) No scoring incentive leads to
              extreme selfish weight setting, since the cabal does not share in the honest rewards.
              (b)-(f) Higher scoring incentive reduces selfishness evidenced by receding honest
              self-weight requirement for stake retention. However, the zero-weight problem at (`1)
              increases as well, since only the cabal can claim reward share from both honest and
              cabal subsets while honest sets zero weight on the cabal. The weight trust solution
              with smooth edge coverage <InlineMath>{'\\ w_{H}>0.95) \\:'}</InlineMath> can only be
              extended so far before legitimate low-utility is also nullified, which practically
              limits scoring incentive around 50%.
            </p>
          </div>
          {/* image wrapper end */}
          <p className={styles.subsection_title}>1.2 Reward emission</p>
          <p>
            In the two-player characterization of the game, there are two bimodal weight
            distributions of <InlineMath>{'\\ (w_{H},\\; 1-w_{C}) \\:'}</InlineMath> and{' '}
            <InlineMath>{'\\ (1-w_{H}, w_{C}) \\:'}</InlineMath> on the honest and cabal players,
            respectively. The stake proportions behind the bimodal distributions are{' '}
            <InlineMath>{'\\ (b_{HH}, \\ b_{CH})=(w_Hs_H, \\ (1-w_{C})(1-s_{H})) \\:'}</InlineMath>{' '}
            and
            <InlineMath>
              {'\\ (b_{HC}, \\ b_{CC})=((1-w_{H})s_{H}, \\ w_{C}(1-s_{H})) \\:'}
            </InlineMath>{' '}
            , respectively.
          </p>
          <span className={styles.centered}>
            <InlineMath>{`\\ b_{HH}=w_{H}s_{H} \\ \\ \\ b_{CH}=(1-w_{C})(1-s_{H}) \\:`}</InlineMath>
            <br />
            <InlineMath>{'\\ b_{HC}=(1-w_{H})s_{H} \\ \\ \\ b_{CC}=w_{C}(1-s_{H}) \\:'}</InlineMath>
          </span>

          <p>
            Primary incentive <InlineMath>{'i'}</InlineMath> is the normalized sum of stake
            proportions, where honest rank{' '}
            <InlineMath>{'\\ r_{H}=w_{H} s_{H}+(1-w_{C})(1-s_{H}) \\:'}</InlineMath> and cabal rank{' '}
            <InlineMath>{'\\ r_{C}=(1-w_{H})s_{H}+w_{C}(1-s_{H}) \\:'}</InlineMath> are normalized
            to give <InlineMath>{'\\ i_{H}=\\frac{r_{H}}{r_{H}+r_{C}} \\:'}</InlineMath> and{' '}
            <InlineMath>{'\\ i_{C}= \\frac{r_{C}}{r_{H}+r_{C}} \\:'}</InlineMath> . An additional
            reward <InlineMath>{'d'}</InlineMath> is the scoring share of incentive, i.e.{' '}
            <InlineMath>
              {'\\ d_{H}=\\frac{w_{H} s_{H}}{r_{H}}i_{H}+\\frac{(1-w_{H})s_{H}}{r_{C}}i_{C} \\:'}
            </InlineMath>{' '}
            and
            <InlineMath>
              {'\\ d_{C}=\\frac{(1-w_{C})s_{C}}{r_{H}}i_{H}+\\frac{w_{C}s_{C}}{r_{C}}i_{C} \\:'}
            </InlineMath>
            . Finally, the complete reward emissions are
            <InlineMath>{'\\ e_{H}=\\frac{i_{H}+d_{H}}{2}'}</InlineMath> and{' '}
            <InlineMath>{'\\ e_{C}=\\frac{i_{C}+d_{C}}{2} \\:'}</InlineMath>, such that{' '}
            <InlineMath>{'\\ e_{H}+e_{C}=1 \\:'}</InlineMath> .
          </p>
          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_4.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 4 / </span>
              Larger minority excess: The excess weight above consensus is larger for minority-stake
              when <InlineMath>{'\\ w_{H}<w_{C} \\:'}</InlineMath>. (a) Positive contours when{' '}
              <InlineMath>{'\\ w_{H}<w_{C} \\:'}</InlineMath> at (`1) indicate regions of cabal
              error-correction potential. (b) Cabal error-correction region grows as majority stake
              increases <InlineMath>{'\\ s_{H}=0.55\\rightarrow 0.95 \\:'}</InlineMath>. (c) However
              at (`2), larger majority excess appears on the right-side when{' '}
              <InlineMath>{'\\ w_{C}<w_{H} \\:'}</InlineMath>(to be avoided), which negatively
              impacts the majority weight more than the minority.
            </p>
          </div>
          {/* image wrapper end */}
          <p className={styles.subsection_title}>1.3 Consensus deviation</p>
          <p>
            The weight consensus is the stake-proportion weight average
            <InlineMath>
              {'\\ \\overline{w_{j}} = \\sum_i(s_{i} w_{ij})w_{ij} / \\sum_k(s_{k} w_{kj})\\:'}
            </InlineMath>
            , and accordingly the consensus weights for the honest and cabal players are <br />
            <InlineMath>
              {
                '\\ \\overline{w_{H}}=\\frac{s_{H} w_{H}^2 + (1-s_{H})(1-w_{C})^2}{s_{H} w_{H} + (1-s_{H})(1-w_{C})} \\:'
              }
            </InlineMath>{' '}
            and ,<br />
            <InlineMath>
              {
                '\\ \\overline{w_{C}}=\\frac{s_{H} (1-w_{H})^2 + (1-s_{H})w_{C}^2}{s_{H} (1-w_{H}) + (1-s_{H})w_{C}} \\:'
              }
            </InlineMath>{' '}
            respectively
          </p>
          <p>
            Under typical adversarial play with <InlineMath>{'\\ 1-w_{H}<w_{C} \\:'}</InlineMath> ,
            the upper modes <InlineMath>{'\\ w_{H}> \\overline{w_{H}} \\:'}</InlineMath> and{' '}
            <InlineMath>{'\\ w_{C}> \\overline{w_{C}} \\:'}</InlineMath> of the honest and cabal
            weight distributions, respectively, will exceed the consensus. Honest excess{' '}
            <InlineMath>{'\\ \\overline{w_{H}}<w_{H} \\:'}</InlineMath> is present when{' '}
            <InlineMath>{' \\ 1-w_{C}<w_{H}: \\:'}</InlineMath>
          </p>
          <div
            style={{display: 'flex', flexDirection: 'column', gap: '16px', alignItems: 'center'}}
          >
            <InlineMath>{'\\ \\overline{w_{H}}<w_{H} \\:'}</InlineMath>
            <InlineMath>
              {
                '\\ \\frac{s_{H} w_{H}^{2} + (1-s_{H})(1-w_{C})^2}{s_{H} w_{H} + (1-s_{H})(1-w_{C})}<w_{H} \\:'
              }
            </InlineMath>
            <InlineMath>{'\\ s_{H} w_{H}^{2} + (1-s_{H})(1-w_{C})^{2} < \\:'}</InlineMath>
            <InlineMath>
              {'\\ \\qquad\\qquad\\qquad\\qquad s_{H} w_{H}^{2} + (1-s_{H})(1-w_{C})w_{H} \\:'}
            </InlineMath>
            <InlineMath>
              {'\\ \\qquad\\qquad\\qquad\\qquad (1-s_{H})(1-w_{C})^2<(1-s_{H})(1-w_{C})w_{H} \\:'}
            </InlineMath>
            <InlineMath>{'\\ \\qquad\\qquad\\qquad\\qquad 1-w_{C}<w_{H} \\:'}</InlineMath>
          </div>
          <p style={{width: '100%', flex: 1}}>
            Similarly, <InlineMath>{'\\ 1-w_{H}<w_{C} \\:'}</InlineMath> produces cabal excess{' '}
            <InlineMath>{'\\ \\overline{w_{C}}<w_{C} \\:'}</InlineMath> :
          </p>
          <div
            style={{display: 'flex', flexDirection: 'column', gap: '16px', alignItems: 'center'}}
          >
            <InlineMath>{'\\ \\overline{w_{C}}<w_{C} \\:'}</InlineMath>
            <InlineMath>
              {
                '\\ \\frac{s_{H} (1-w_{H})^2 + (1-s_{H})w_{C}^{2}}{s_{H} (1-w_{H}) + (1-s_{H})w_{C}}<w_{C} \\:'
              }
            </InlineMath>
            <InlineMath>{'\\ s_{H} (1-w_{H})^{2} + (1-s_{H})w_{C}^{2} < \\:'}</InlineMath>
            <InlineMath>
              {'\\ \\qquad\\qquad\\qquad\\qquad s_{H} (1-w_{H})w_{C} + (1-s_{H})w_{C}^{2} \\:'}
            </InlineMath>
            <InlineMath>
              {'\\ \\qquad\\qquad\\qquad\\qquad s_{H} (1-w_{H})^{2} < s_{H} (1-w_{H})w_{C} \\:'}
            </InlineMath>
            <InlineMath>{'\\ \\qquad\\qquad\\qquad\\qquad 1-w_{H}<w_{C} \\:'}</InlineMath>
          </div>
          <p>
            <strong className={styles.bold}>Lemma 1 (Larger Minority Excess). </strong>
            <i className={styles.italized}>
              Minority-stake excess weight is larger than majority-stake excess weight, i.e.
            </i>
            <InlineMath>
              {
                '\\ w_{C} - \\overline{w_{C}} > w_{H} - \\overline{w_{H}}, \\ when \\ w_{H} < w_{C} \\:'
              }
            </InlineMath>
          </p>
          <p style={{width: '100%', flex: 1}}>
            We use a symbolic solver (Wolfram) to show that the cabal excess is larger when{' '}
            <InlineMath>{'\\ w_{H} < w_{C}, \\:'}</InlineMath> i.e. <br />
            <span className={styles.centered}>
              <InlineMath>
                {
                  '\\ \\frac{dw_{C}}{dw_{H}}=\\frac{w_{C}-\\overline{w_{C}}}{w_{H}-\\overline{w_{H}}} > 1 \\:'
                }
              </InlineMath>{' '}
            </span>
            <br />
            The cabal excess is larger with majority honest stake when
          </p>
          <InlineMath>
            {
              '\\begin{Bmatrix} 0<w_{C} \\leq 0.5 \\\\ 0.5<w_{H} \\leq 1-w_{C} \\\\ 0.5<s_{H}<1 \\end{Bmatrix} \\ or \\ \\begin{Bmatrix} 0.5<w_{C} \\leq 1 \\\\ 0.5<w_{H} \\le w_{C} \\\\ 0.5<s_{H}<1 \\end{Bmatrix}'
            }
          </InlineMath>
          <p>Otherwise, for the following conditions</p>
          <InlineMath>
            {
              '\\begin{Bmatrix} 0<w_{C} \\leq 0.5 \\\\ 1 - w_{C} < w_{H} \\end{Bmatrix} \\ or \\ \\begin{Bmatrix} 0.5<w_{C} \\leq 1 \\\\ w_{C} < w_{H} < 1 \\end{Bmatrix}'
            }
          </InlineMath>
          <p>Otherwise, for the following conditions</p>
          <InlineMath>
            {`\\ 
              \\frac{w_{C}^{2}-w_{C}}{w_{C}^{2}-w_{C}-w_{H}^{2}+w_{H}}- \\\\
              \\sqrt{\\frac{w_{C}^{2} w_{H}^{2}-w_{C}^{2} w_{H}-w_{C} w_{H}^{2}+w_{C} w_{H}}{\\left(w_{C}^{2}-w_{C}-w_{H}^{2}+w_{H}\\right)^{2}}}<s_{H}<1.
              
              \\:`}
          </InlineMath>

          <p className={styles.subsection_title}>1.4 Excess weight reduction</p>
          <p>
            Stake-proportional consensus advantages the honest player with{' '}
            <InlineMath>{'\\ s_{H}>0.5 \\:'}</InlineMath> , since it biases the consensus weight
            toward the honest vote and exposes the cabal excess self-weight
            <InlineMath>{'\\ w_{C} > \\overline{w_{C}} \\:'}</InlineMath> where{' '}
            <InlineMath>{'\\ dw_{C}>dw_{H} \\:'}</InlineMath> . Consequently, a consensus policy{' '}
            <InlineMath>
              {'\\ \\pi(\\mathbf{w})=\\min(\\overline{\\mathbf{w}}, \\mathbf{w}) \\:'}
            </InlineMath>{' '}
            can reduce excess weight above the consensus
            <InlineMath>{'\\ \\overline{\\mathbf{w}} \\:'}</InlineMath> , where cabal weight should
            decrease more than honest weight. The weight reductions normally only happen in the
            upper modes <InlineMath>{'\\ w_{H} \\:'}</InlineMath> and{' '}
            <InlineMath>{'\\ w_{C} \\:'}</InlineMath> of the honest and cabal weights, respectively.
          </p>
          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_5.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 5 / </span>
              Larger minority excess: The excess weight above consensus is larger for minority-stake
              when <InlineMath>{'\\ w_{H}<w_{C} \\:'}</InlineMath>. (a) Positive contours when{' '}
              <InlineMath>{'\\ w_{H}<w_{C} \\:'}</InlineMath> at (`1) indicate regions of cabal
              error-correction potential. (b) Cabal error-correction region grows as majority stake
              increases <InlineMath>{'\\ s_{H}=0.55\\rightarrow 0.95 \\:'}</InlineMath>. (c) However
              at (`2), larger majority excess appears on the right-side when{' '}
              <InlineMath>{'\\ w_{C}<w_{H} \\:'}</InlineMath>(to be avoided), which negatively
              impacts the majority weight more than the minority.
            </p>
          </div>
          {/* image wrapper end */}
          <p>
            The consensus policy{' '}
            <InlineMath>
              {'\\ \\pi(\\mathbf{w})\\rightarrow (w^{\\prime}_{H}, w^{\\prime}_{C}) \\:'}
            </InlineMath>{' '}
            attempts to correct the error{' '}
            <InlineMath>{'\\ \\epsilon=w_{H}+w_{C}-1 \\:'}</InlineMath> so that
          </p>
          <InlineMath>
            {`\\ w^{\\prime}_{C} = 1 - w^{\\prime}_{H} \\\\
             w_{C} - \\Delta w_{C} = 1 - (w_{H} - \\Delta w_{H}) \\\\
             \\Delta w_{H} + \\Delta w_{C} = w_{H} + w_{C} - 1 \\\\
             \\eta(d w_{H} + d w_{C}) = w_{H} + w_{C} - 1 \\\\
             \\eta = \\frac{{w_{H} + w_{C} - 1}}{d w_{H} + d w_{C}}.\\
             `}
          </InlineMath>
          <p>
            The approximate number of weight reduction steps is{' '}
            <InlineMath>{'\\ \\eta \\:'}</InlineMath>, and the consensus policy is thus converted to
            an iterated function <InlineMath>{'\\ \\pi=f^{\\eta} \\:'}</InlineMath> , where the
            function is repeated <InlineMath>{'\\ \\eta \\:'}</InlineMath> times{' '}
            <InlineMath>{'\\ f^3(\\mathbf{w})=f(f(f(\\mathbf{w}))) \\:'}</InlineMath> . Note that
            <InlineMath>
              {'\\ f(\\mathbf{w})=\\min(\\overline{\\mathbf{w}}, \\mathbf{w}) \\:'}
            </InlineMath>{' '}
            recomputes the consensus weight{' '}
            <InlineMath>{'\\ \\overline{\\mathbf{w}} \\:'}</InlineMath> each time.
          </p>

          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_6.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 6 / </span>
              Emission slashing: Iterative weight correction reduces effective scoring weight and
              incentive share. (a) Initial scoring weight is always 1, but weight correction reduces
              this whenever <InlineMath>{'\\ \\sum w\\neq 1 \\:'}</InlineMath>, with the largest
              reduction seen at (`1). (b) Emission is the average of scoring incentive and utility
              reward and cabal emission is slashed, i.e.{' '}
              <InlineMath>{'\\ e^{(\\eta=3)} < e^{(\\eta=0)} \\:'}</InlineMath> particularly in the{' '}
              <InlineMath>{'\\ w_{H}<w_{C} \\:'}</InlineMath> and{' '}
              <InlineMath>{'\\ 0.5<s_{H} \\:'}</InlineMath> region around (`2). (c) Consequently,
              honest emission is boosted in region (`2), but the zero weight vulnerability at (`3)
              slashes honest emission, although comparatively little.
            </p>
          </div>
          {/* image wrapper end */}
          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_7.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 7 / </span>
              Cabal slash (two-team). (a) Statistical analysis simplifies the two-player result. (b)
              The two-team generalization fully enacts the weight distribution, and a Monte Carlo
              analysis reveals the worst-case cabal slash closely following the two-player result.
              (c) Cabal incentive is boosted at the zero weight vulnerability of (`1), but weight
              trust solution ensures active cabal slash at (`2).
            </p>
          </div>
          {/* image wrapper end */}
          <p>
            We compute{' '}
            <InlineMath>{'\\ \\eta = \\frac{w_{H} + w_{C} - 1}{d w_{H} + d w_{C}} \\:'}</InlineMath>{' '}
            and compare with the correction factor
            <InlineMath>{'\\ dw_{C}/dw_{H} \\:'}</InlineMath> to identify the optimal{' '}
            <InlineMath>{'\\ \\eta \\:'}</InlineMath> avoiding over-correction in the detrimental
            <InlineMath>{'\\ w_{C}<w_{H} \\:'}</InlineMath> region where{' '}
            <InlineMath>{'\\ dw_{C}/dw_{H}<1 \\:'}</InlineMath>
          </p>
          <p>
            We observe that higher <InlineMath>{'\\ \\eta>3 \\:'}</InlineMath> values extend the
            correction further into the detrimental
            <InlineMath>{'\\ w_{C}<w_{H} \\:'}</InlineMath> region where{' '}
            <InlineMath>{'\\ dw_{C} - dw_{H} < 0 \\:'}</InlineMath> , hence an optimal{' '}
            <InlineMath>{'\\ \\eta\\approx 3 \\:'}</InlineMath> is identified, which is large enough
            to provide sufficient correction when <InlineMath>{'\\ w_{H}<w_{C} \\:'}</InlineMath> .
          </p>
          <p>
            Application of the consensus policy{' '}
            <InlineMath>{'\\ \\pi(\\mathbf{w})=f^{\\eta\\approx 3}(\\mathbf{w}) \\:'}</InlineMath>{' '}
            can partially correct the error{' '}
            <InlineMath>{'\\ \\epsilon=w^{\\prime}_{C}+w^{\\prime}_{H}-1>0 \\:'}</InlineMath> , in
            particular the previous expense <InlineMath>{'\\ w_{H}=1 \\:'}</InlineMath> is reduced
            to <InlineMath>{'\\ w_{H}<0.75 \\:'}</InlineMath> for{' '}
            <InlineMath>{'\\ s_{H}=0.6 \\:'}</InlineMath> , even at Nash equilibrium with{' '}
            <InlineMath>{'\\ w_C^{*}=0.8 \\:'}</InlineMath> . Importantly, the consensus policy{' '}
            <InlineMath>{'\\ \\pi(\\mathbf{w}) \\:'}</InlineMath> operates on anonymized weights and
            do not assume the player identities, thus behaves impartially in terms of a stake-based
            consensus.
          </p>
          <p className={styles.subsection_title}>1.5 Smoothed weight reduction</p>
          <p>
            The correction function{' '}
            <InlineMath>
              {'\\ f(\\mathbf{w})=\\min(\\overline{\\mathbf{w}}, \\mathbf{w}) \\:'}
            </InlineMath>{' '}
            should be smoothed to ensure
            <InlineMath>
              {'\\ \\lim_{dw\\rightarrow 0}f^{\\eta}(w+dw) - f^{\\eta}(w)<\\varepsilon \\:'}
            </InlineMath>{' '}
            where adjacent weights are corrected to a similar degree. The correction factor should
            also depend on the magnitude of deviation from consensus, in terms of a standard
            deviation <InlineMath>{'\\sigma'}</InlineMath>. We opt for a stake-weighted mean
            absolute deviation, since it does not make the normal assumption as strongly as mean
            square deviation, as follows <br />
          </p>
          <InlineMath>
            {
              '\\sigma(\\mathbf{w})=\\frac{\\sum_{i}s_{i}w_{ij}|w_{ij} - \\overline{\\mathbf{w}}|}{\\sum_{k}s_{k}w_{kj}}.'
            }
          </InlineMath>
          <p>
            The standard correction <InlineMath>{'\\ 0 \\le \\alpha<1 \\:'}</InlineMath> fully
            applies when
            <InlineMath>{'\\ w - \\overline{\\mathbf{w}}=\\sigma(\\mathbf{w}) \\:'}</InlineMath>,
            and amplifies when
            <InlineMath>{'\\ w - \\overline{\\mathbf{w}}>\\sigma(\\mathbf{w}) \\:'}</InlineMath> to
            a maximum correction at
            <InlineMath>{'\\ \\overline{\\mathbf{w}} \\:'}</InlineMath> with a proposed smoothed
            function iterate
          </p>
          <InlineMath>{`\\ 
            f(\\mathbf{w}\|w>\\overline{\\mathbf{w}})=\\overline{\\mathbf{w}} + (w - \\overline{\\mathbf{w}})\\alpha^{\\frac{w - \\overline{\\mathbf{w}}}{\\sigma(\\mathbf{w})}} \\\\
            =\\overline{\\mathbf{w}} + (w - \\overline{\\mathbf{w}})(1-\\delta)\\\\
            =\\overline{\\mathbf{w}} + w - \\overline{\\mathbf{w}} -\\delta (w - \\overline{\\mathbf{w}})\\\\
            =w -\\delta (w - \\overline{\\mathbf{w}})\\\\
            =w -\\delta \\cdot dw\\
          \\:`}</InlineMath>
          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_8.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 8 / </span>
              Weight trust: Majority stake should set non-zero weight to a player, otherwise its
              reward is nullified. (a) Two-team honest emission with just the consensus policy, with
              zero weight vulnerability at (`1). (b) Applying weight trust protects the{' '}
              <InlineMath>{'\\ 0.95<w_{H} \\:'}</InlineMath> region and removes the exploit. (c)
              Honest retention is now monotonically possible at increasing honest self-weight.
            </p>
          </div>
          {/* image wrapper end */}
          <p>
            The smoothed function iterate now requires more steps, where a larger{' '}
            <InlineMath>{'\\ \\alpha\\approx 1-\\delta \\:'}</InlineMath>
            results in a larger <InlineMath>{'\\ \\eta \\:'}</InlineMath> , such that{' '}
            <InlineMath>{'\\ \\eta\\delta\\approx 3 \\:'}</InlineMath> , according to
          </p>
          <InlineMath>{`\\
           w^{\\prime}_{C} = 1 - w^{\\prime}_{H}\\\\
           w_{C} - \\Delta w_{C} = 1 - (w_{H} - \\Delta w_{H})\\\\
           \\Delta w_{H} + \\Delta w_{C} = w_{H} + w_{C} - 1\\\\
           \\eta\\delta(d w_{H} + d w_{C}) = w_{H} + w_{C} - 1\\\\
           \\eta = \\frac{w_{H} + w_{C} - 1}{\\delta(d w_{H} + d w_{C})}.\\\\
          \\:`}</InlineMath>
          <p>
            We compare the previous retention graph{' '}
            <InlineMath>{'\\ (\\eta=3, \\alpha=0) \\:'}</InlineMath>
            against <InlineMath>{'\\ (\\eta=3/(1-0.95), \\alpha=0.95) \\:'}</InlineMath> and observe
            a reduced cost <InlineMath>{'\\ w_{H}=0.7<0.75 \\:'}</InlineMath>
            with the smoothed iterate with <InlineMath>{'\\ \\alpha>0 \\:'}</InlineMath>.
          </p>
          <p>
            We ensure monotonicity of the consensus policy by choosing the minimum{' '}
            <InlineMath>{'\\ \\varepsilon>0 \\:'}</InlineMath>
            added to the deviation <InlineMath>{'\\ \\sigma(\\mathbf{w}) \\:'}</InlineMath>.
          </p>
          <InlineMath>{`\\ 
          f(\\mathbf{w}|w>\\overline{\\mathbf{w}})<f(\\mathbf{w} + dw\\|w>\\overline{\\mathbf{w}})\\\\
          \\overline{\\mathbf{w}} + (w - \\overline{\\mathbf{w}})\\alpha^{\\frac{w - \\overline{\\mathbf{w}}}{\\sigma(\\mathbf{w})+\\varepsilon}}<\\overline{\\mathbf{w}} + (w + dw - \\overline{\\mathbf{w}})\\alpha^{\\frac{w + dw - \\overline{\\mathbf{w}}}{\\sigma(\\mathbf{w})+\\varepsilon}}\\\\
          (w - \\overline{\\mathbf{w}})\\alpha^{\\frac{w - \\overline{\\mathbf{w}}}{\\sigma(\\mathbf{w})+\\varepsilon}}< (w + dw - \\overline{\\mathbf{w}})\\alpha^{\\frac{w + dw - \\overline{\\mathbf{w}}}{\\sigma(\\mathbf{w})+\\varepsilon}}\\\\
          \\frac{w - \\overline{\\mathbf{w}}}{w + dw - \\overline{\\mathbf{w}}}< \\alpha^{\\frac{w + dw - \\overline{\\mathbf{w}}}{\\sigma(\\mathbf{w})+\\varepsilon} - \\frac{w - \\overline{\\mathbf{w}}}{\\sigma(\\mathbf{w})+\\varepsilon}}\\\\
          \\log\\frac{w - \\overline{\\mathbf{w}}}{w + dw - \\overline{\\mathbf{w}}}< \\log\\alpha^{\\frac{dw}{\\sigma(\\mathbf{w})+\\varepsilon}}\\\\
          \\frac{dw\\log\\alpha}{\\log\\frac{w - \\overline{\\mathbf{w}}}{w + dw - \\overline{\\mathbf{w}}}} - \\sigma(\\mathbf{w})<\\varepsilon\\\\
          \\:`}</InlineMath>

          <p className={styles.subsection_title}>1.6 Weight trust</p>
          <p>
            Weight trust <InlineMath>{'\\ T=(W>0)S \\:'}</InlineMath> is the sum of stake assigning
            a non-zero weight to a player, and a consensus
            <InlineMath>{'\\ C=(1 + \\exp(-\\rho(T-\\kappa)))^{-1} \\:'}</InlineMath> provides a
            smooth threshold at <InlineMath>{'\\kappa'}</InlineMath> where exceeding
            <InlineMath>{'\\kappa'}</InlineMath> ratio of stake quickly allows for high trust. A
            modified rank
            <InlineMath>{'\\ r^{\\prime}=rc \\:'}</InlineMath> multiplies rank with the weight trust
            consensus, which influences the emission so that zero cabal weight{' '}
            <InlineMath>{'\\ w^{\\prime}_{C}=1-w_{H}\\approx 0 \\:'}</InlineMath> receives low
            consensus thereby penalizing cabal emissions.
          </p>
          <p>
            The vulnerable region of <InlineMath>{'\\ w_{H}=1 \\:'}</InlineMath> and{' '}
            <InlineMath>{'\\ 0.8<w_{C}<0.95 \\:'}</InlineMath>
            allows for cabal stake gain when <InlineMath>{'\\ s_{H}=0.6 \\:'}</InlineMath>, but the
            weight trust consensus smoothly pads the region around
            <InlineMath>{'\\ w_{H}=1 \\:'}</InlineMath> and removes the vulnerability. The cabal can
            thus not claim reward when the honest majority deems cabal utility to be zero, despite
            the non-zero self-weight reported by the minority cabal.
          </p>
          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_9.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 9 / </span>
              Evolution smoothness: Evolving through more smaller steps with
              <InlineMath>{'\\ \\eta=59 \\:'}</InlineMath> reduces the zero weight exploit at black
              (`3), compared to <InlineMath>{'\\ \\eta=3 \\:'}</InlineMath> at (`1), since
              fine-grained correction steps more accurately track changes in consensus. The two-team
              honest emissions (Monte Carlo worst-case analysis) tend to further reduce the zero
              weight exploits at (`2), (`4) compared to the two-player case at (`1), (`3).
            </p>
          </div>
          {/* image wrapper end */}
        </section>

        <section className={styles.section}>
          <p className={styles.subtitle}>02/ Density generalization</p>
          <p className={styles.subsection_title}>2.1 Overview</p>
          <p>
            We generalize the two-player game to a two-team game with{' '}
            <InlineMath>{'\\ |H| \\:'}</InlineMath> honest and{' '}
            <InlineMath>{'\\ |C| \\:'}</InlineMath> cabal players, that have{' '}
            <InlineMath>{'\\ \\sum_{i\\in H}s_{i}=s_{H} \\:'}</InlineMath> honest stake and{' '}
            <InlineMath>{'\\ \\sum_{i\\in C}s_{i}=1-s_{H} \\:'}</InlineMath> cabal stake. Honest
            players <InlineMath>{'\\ i\\in H \\:'}</InlineMath> set{' '}
            <InlineMath>{'\\ \\sum_{j\\in H}w_{ij}=w_{H} \\:'}</InlineMath> self-weight and{' '}
            <InlineMath>{'\\ \\sum_{j\\in C}w_{ij}=1-w_{H} \\:'}</InlineMath> weight on cabal
            players, while cabal players <InlineMath>{'\\ i\\in C \\:'}</InlineMath> set{' '}
            <InlineMath>{'\\ \\sum_{j\\in C}w_{ij}=w_{C} \\:'}</InlineMath> self-weight and{' '}
            <InlineMath>{'\\ \\sum_{j\\in H}w_{ij}=1-w_{C} \\:'}</InlineMath>
            weight on honest players. The rank components result in the same aggregates in the
            two-player game.
          </p>
          <InlineMath>{`\\
          b_{HH}=\\sum_{j\\in H}w_{ij}\\sum_{i\\in H}s_{i}=w_{H}s_{H}\\\\
          b_{CH}=\\sum_{j\\in H}w_{ij}\\sum_{i\\in C}s_{i}=(1-w_{C})(1-s_{H})\\\\
          b_{HC}=\\sum_{j\\in C}w_{ij}\\sum_{i\\in H}s_{i}=(1-w_{H})s_{H}\\\\
          b_{CC}=\\sum_{j\\in C}w_{ij}\\sum_{i\\in C}s_{i}=w_{C}(1-s_{H})
          \\:`}</InlineMath>
          <p>
            In particular, the weight consensus of an individual honest player is shown to be{' '}
            <InlineMath>{'\\ \\overline{w_{h}}=\\frac{\\overline{w_{H}}}{|H|} \\:'}</InlineMath> as
            follows (and similarly for cabal players{' '}
            <InlineMath>{'\\ \\overline{w_{c}}=\\frac{\\overline{w_{C}}}{|C|} \\:'}</InlineMath>)
          </p>
          <InlineMath>{`\\
          \\overline{w_{h}}=\\sum_{i} s_{i} w^{2}_{ih}\\left/ \\sum_k s_{k} w_{kh}\\right.\\\\
          =\\frac{\\sum_{i\\in H} s_{i} w^{2}_{ih} + \\sum_{i\\in C} s_{i} w^{2}_{ih}}{\\sum_{k\\in H} s_{k} w_{kh} + \\sum_{k\\in C} s_{k} w_{kh}}\\\\
          \\approx\\frac{\\sum_{i\\in H} s_{i} w_{H}^{2}/|H|^{2} + \\sum_{i\\in C} s_{i} (1 - w_{C})^{2}/|H|^{2}}{\\sum_{k\\in H} s_{k} w_{H}/|H| + \\sum_{k\\in C} s_{k} (1 - w_{C})/|H|}\\\\
          =\\frac{1}{|H|}\\frac{s_{H} w_{H}^{2} + (1-s_{H})(1-w_{C})^{2}}{s_{H} w_{H} + (1-s_{H})(1-w_{C})}=\\frac{\\overline{w_{H}}}{|H|}\\
          \\:`}</InlineMath>
          <p>
            Under the minimal assumption that the average self-weights set on honest and cabal
            players are
            <InlineMath>{'\\ \\frac{w_{H}}{|H|} \\:'}</InlineMath> and{' '}
            <InlineMath>{'\\ \\frac{w_{C}}{|C|} \\:'}</InlineMath> we can construct weight densities
            <InlineMath>{'\\ p_h(w) = p_{hh}(w) + p_{ch}(w) \\:'}</InlineMath> and{' '}
            <InlineMath>{'\\ p_{c}(w) = p_{hc}(w) + p_{cc}(w) \\:'}</InlineMath>, here according to
            the normal assumption (other densities with a similar first moment could possibly also
            be valid)
          </p>
          <InlineMath>{`\\
            p_{hh}(w) = s_{H} w \\mathcal{N}\\left(\\frac{w_{H}}{|H|}, \\frac{w_{H}}{|H|}\\sigma\\right) \\\\
            p_{ch}(w) = (1-s_{H}) w \\mathcal{N}\\left(\\frac{1-w_{C}}{|H|}, \\frac{1-w_{C}} {|H|}\\sigma\\right)\\\\
            p_{hc}(w) = s_{H} w \\mathcal{N}\\left(\\frac{1-w_{H}}{|C|}, \\frac{1-w_{H}}{|C|}\\sigma\\right)\\\\
            p_{cc}(w) = (1-s_{H}) w \\mathcal{N}\\left(\\frac{w_{C}}{|C|}, \\frac{w_{C}}{|C|}\\sigma\\right)
          \\:`}</InlineMath>
          <p>
            The consensus and mean absolute deviations of a weight density function{' '}
            <InlineMath>{'\\ p(w) \\:'}</InlineMath> are
          </p>
          <InlineMath>
            {
              '\\ \\overline{p}=\\int w p(w) dw,\\quad {and} \\quad\\sigma(p)= \\int |w - \\overline{p}|p(w) dw. \\:'
            }
          </InlineMath>
          <p>
            We overload the iterated function <InlineMath>{'\\ f \\:'}</InlineMath> as a density
            evolution function <InlineMath>{'\\ f(p(w)) \\:'}</InlineMath> that contracts a density
            <InlineMath>{'\\ p(w) \\:'}</InlineMath> above consensus
            <InlineMath>{'\\ \\overline{p} \\:'}</InlineMath> by a nominal degree of{' '}
            <InlineMath>{'\\ \\alpha \\:'}</InlineMath> at a single deviation{' '}
            <InlineMath>{'\\ \\frac{w-\\overline{p}}{\\sigma(p)} \\:'}</InlineMath>, in order to
            correct the error <InlineMath>{'\\ \\epsilon=w_{H}+w_{C}-1 \\:'}</InlineMath>. The
            density is contracted via <InlineMath>{'\\ g(w)=f^{-1}(w) \\:'}</InlineMath>
            involving the original iterated function <InlineMath>{'\\ f \\:'}</InlineMath>.
          </p>
          <InlineMath>
            {
              '\\ f(p(w)) = p(w \\ | \\ w\\le \\overline{p}) + p(g(w) \\ | \\ \\overline{p}<w) \\frac{ w}{g(w)}\\frac{dg(w)}{dw} \\:'
            }
          </InlineMath>
          <p>
            The final rank after applying the consensus policy{' '}
            <InlineMath>{'\\ \\pi=f^{\\eta} \\:'}</InlineMath> is{' '}
            <InlineMath>{'\\ r_{i} = \\int f^{\\eta}(p_{i}(w))dw \\:'}</InlineMath>, where a single
            function iteration contracts the consensus by{' '}
            <InlineMath>{'\\ \\overline{p^{\\prime}}-\\overline{p} \\:'}</InlineMath>, which is
            equal to
          </p>
          <InlineMath>
            {
              '\\ \\int w p(w) dw - \\int w p(g(w) \\ | \\ \\mu_{p}<w) \\frac{ w}{g(w)}\\frac{dg(w)}{dw}dw. \\:'
            }
          </InlineMath>
          {/* image wrapper begin */}
          <div className={styles.image_container}>
            <img
              src='/images/consensus_v2/figure_10.png'
              alt='Emission of newly minted token vector E through subnet incentive mechanisms.'
              className={styles.image_container_image}
            />
            <p>
              <span className={styles.image_container_caption_no}>Figure 10 / </span>
              Density evolution: Weight correction through density evolution reduces cabal weight
              consensus more than honest reduction{' '}
              <InlineMath>{'\\ (s_{H}=0.6, w_{H}=0.7, w_{C}=0.8) \\:'}</InlineMath> . The honest
              weights and cabal weights have an equal starting consensus at (`1), but through
              density evolution the cabal reduces more to (`3), versus a higher end honest consensus
              at (`2). Density evolution thus succeeds in penalizing the minority cabal and allows
              for honest stake retention. The theoretical probability densities in (a), (c) closely
              match the stochastically sampled results in (b), (d). The crosshair markers indicate
              the consensus flanked by a standard deviation above and below.
            </p>
          </div>
          {/* image wrapper end */}
          <p>
            Simulating the consensus policy{' '}
            <InlineMath>{'\\ (\\eta=3/(1-0.95), \\alpha=0.95) \\:'}</InlineMath> on weight densities
            set on honest and cabal players where{' '}
            <InlineMath>{'\\ s_{H}=0.6, w_{H}=0.7, w_{C}=0.8 \\:'}</InlineMath>, we see an equal
            starting consensus weight reduce further for the cabal players{' '}
            <InlineMath>{'\\ (6.76\\rightarrow 3.8) \\:'}</InlineMath> vs honest players{' '}
            <InlineMath>{'\\ (6.76\\rightarrow 4.41) \\:'}</InlineMath>. The consensus policy acts
            as an upper-mode resiliency test where cabal self-weight with minority stake fails
            comparatively to honest self-weight with majority stake.
          </p>
          <p className={styles.subsection_title}>2.2 Stochastic sampling</p>
          <p>
            We move from theoretical density analysis to a stochastic sampling analysis, where the
            original
            <InlineMath>{'\\ \\pi(\\mathbf{w})=f^{\\eta}(\\mathbf{w}) \\:'}</InlineMath> can be
            applied directly to a weight sample <InlineMath>{'\\ \\mathbf{w} \\:'}</InlineMath> for
            a player, gradually contracting excess weight toward the consensus until an optimal
            contraction volume is reached. We observe very similar density evolution results as with
            the theoretical density analysis.
          </p>
          <p className={styles.subsection_title}>2.3 Two-team game</p>
          <p>
            We perform a worst-case Monte Carlo analysis of a full-scale two-team game by sampling
            from normal densities, primarily to confirm the accuracy of the preceding aggregate
            analysis. We run a number of Monte Carlo iterations and record the worse-case results. A
            blockchain-based consensus algorithm has space and compute limitations, which would
            favor a smaller <InlineMath>{'\\ \\eta \\:'}</InlineMath> number of density evolution
            operations, each of which requires
            <InlineMath>{'\\ O(n^{2}) \\:'}</InlineMath> operations. A small{' '}
            <InlineMath>{'\\ \\eta=3 \\:'}</InlineMath> with{' '}
            <InlineMath>{'\\ \\alpha=0 \\:'}</InlineMath> produce a full-scale result very close to
            the aggregate result.
          </p>
          <p>
            Increasing the number of density evolution steps to{' '}
            <InlineMath>{'\\ \\eta=59 \\:'}</InlineMath> with
            <InlineMath>{'\\ \\alpha=0.95 \\:'}</InlineMath> manages to remove the zero-utility
            exploit at <InlineMath>{'\\ w_{H}>0.98 \\:'}</InlineMath> seen at{' '}
            <InlineMath>{'\\  \\eta=3 \\:'}</InlineMath>. However, the aggregate result in the
            theoretical honest retention deviates slightly, likely due to deviation of the upper
            mode density below consensus not accounted for in the aggregate.
          </p>
        </section>
        <span className={styles.paper_link}>
          <Link href='/pdfs/consensus_v2/PoS_Utility_Consensus.pdf' isExternal={true}>
            Follow this link for the original version
          </Link>
        </span>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
