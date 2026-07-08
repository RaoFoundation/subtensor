import React from 'react';
import {BlockMath, InlineMath} from 'react-katex';
import {Equations} from './components/Equations';
import styles from './page.module.css';

const Section4 = () => {
  return (
    <section className={styles.section}>
      <h2 className={styles.subtitle}>Section 4: Analysis</h2>
      <p>
        In this section, we construct a simple mathematical toy model of the DTAO system, from which
        we may extract useful formulas for long term predictions. We will begin by establishing the
        governing rules for the model. We then make a few simplifying assumptions/approximations,
        and arrive at some useful formulas. Finally, we compare these formulas to some numerical
        simulations.
      </p>

      <h3 className={styles.subtitle}>Section 4.1: Establishing a Dynamical System</h3>
      <p>
        To set the stage, we will assume that we have <InlineMath>{`N`}</InlineMath>
        &#45;many subnets (indexed by <code>i</code>) with constant product AMMs to swap between
        Alpha and TAO tokens. The token reserve quantities are denoted by
        <InlineMath>{`\\{\\alpha_i, \\tau_i\\}`}</InlineMath>, as functions of time:
      </p>
      <Equations
        equNo={12}
        minify={true}
        equ={`\\tau_i(t) = \\text{TAO reserves at time } t \\text{ (}i^{th}\\text{ subnet)}`}
      />
      <Equations
        equNo={13}
        minify={true}
        equ={`\\alpha_i(t) = \\text{Alpha reserves at time } t \\text{ (}i^{th}\\text{ subnet)}`}
      />

      <p>As constant product pools, they obey the following:</p>
      <Equations equNo={14} equ={`p_i(t) = \\frac{\\tau_i(t)}{\\alpha_i(t)}`} />
      <Equations equNo={15} equ={`L_i(t) = \\sqrt{\\alpha_i(t)\\tau_i(t)}`} />
      <p>
        where <InlineMath>p_i</InlineMath> is the price of Alpha in terms of TAO, and{' '}
        <InlineMath>L_i</InlineMath> is the liquidity scale factor for the pool. At regular
        intervals (blocks), the reserves update according to section 3.2:
      </p>
      <Equations
        equNo={16}
        equ={`\\Delta \\tau_i(t) = \\frac{ p_i(t)}{\\sum_j p_j(t)}\\Delta \\overline{\\tau}`}
      />
      <Equations
        equNo={17}
        equ={`\\Delta \\alpha_i(t) = \\min\\left\\{\\frac{\\Delta \\overline{\\tau}}{\\sum_j p_j(t)}, \\Delta \\overline{\\alpha}_i\\right\\}`}
      />
      <p>
        The pool reserves are altered anytime a swap is executed, and these swaps then determine the
        pool price. However, for our mathematical model, we will suppose that the price trajectories{' '}
        <InlineMath>{'p_i(t)'}</InlineMath>
        are <em>given</em> (at least, probabilistically), and thus the pool reserves are determined
        implicitly. In particular, we note that while the liquidity factor{' '}
        <InlineMath>{'L_i'}</InlineMath> is given by (16), it will also be determined simply by the
        cumulative amount of tokens injected. This is because swaps do not alter the liquidity
        constant (indeed, that is the <em>whole point</em> of the constant product), and so only the
        injection can determine <InlineMath>{'L'}</InlineMath>. Thus, we define the cumulative token
        injections
        <InlineMath>{'(\\alpha^*_i,\\tau^*_i)'}</InlineMath> by the following:
      </p>
      <Equations
        equNo={18}
        minify={true}
        equ={`\\tau_i^*(t) := \\sum_s^t \\Delta \\tau_i(s) \\quad \\text{(cumulative injected TAO)}`}
      />
      <Equations
        equNo={19}
        minify={true}
        equ={`\\alpha_i^*(t) := \\sum_s^t \\Delta \\alpha_i(s) \\quad \\text{(cumulative injected Alpha)}`}
      />

      <p>With these defined, we can then restate L:</p>
      <Equations
        equNo={19}
        minify={true}
        equ={`L_i(t) = \\sqrt{\\big[\\alpha_i(0)+\\alpha_i^*(t)\\big]\\big[\\tau_i(0)+\\tau_i^*(t)\\big]}`}
      />
      <p>
        Finally, we define one more bit of notation for convenience. Specifically, we denote the sum
        of prices by
        <InlineMath>{`\\ S(t)`}</InlineMath>
      </p>
      <Equations equNo={21} minify={true} equ={`S(t) := \\sum\\nolimits_i p_i(t)`} />

      <p>Then our deltas in (17)&#45;(18) can be rewritten as </p>

      <Equations
        equNo={22}
        minify={true}
        equ={`\\Delta \\tau_i(t) = p_i(t) \\frac{\\Delta \\overline{\\tau}}{S(t)}`}
      />
      <Equations
        equNo={23}
        minify={true}
        equ={` \\left\\{ \\frac{\\Delta \\overline{\\tau} }{S(t)}, \\hspace{0.1pc} \\Delta \\overline{\\alpha}_i \\right\\}`}
      />
      <p>which will be slightly more convenient later on.</p>

      <h3 className={styles.subtitle}>Section 4.2: Three Assumptions/Approximations</h3>
      <p>
        For our first simplifying assumption,{' '}
        <i>
          we will assume that the prices
          <InlineMath>{'\\ {p_i(t)}'}</InlineMath> each evolve according to Geometric Brownian
          Motion (GBM)
        </i>
        . Specifically, this means that for each subnet, we specify
      </p>
      <ul className={styles.unorder_list}>
        <li>
          an initial price <InlineMath>{`\\ p_i`}</InlineMath> (0)
        </li>
        <li>
          a drift parameter <InlineMath>{`\\ \\mu_i`}</InlineMath>
        </li>
        <li>
          a volatility parameter<InlineMath>{`\\ \\sigma_i`}</InlineMath>
        </li>
      </ul>
      <p>
        and then the price <InlineMath>{'p_i(t)'}</InlineMath> at time
        <InlineMath>{'t'}</InlineMath> will be distributed with the following log&#45;normal
        probability density:
      </p>
      <Equations
        equNo={24} // Adjust the equation number as needed
        minify={true}
        equ={`
             p_i(t) 
            \\sim \\frac{1}{\\sqrt{2\\pi\\sigma_i^2 t}} \\frac{1}{p} 
            \\exp\\left( \\frac{-\\big(\\log[p/p_i(0)] - \\mu_i t\\big)^2}{2\\sigma_i^2 t} \\right) \\\\
            \\\\ 
            \\hspace{2pc} \\text{(for } 0 < p < \\infty\\text{)}
            `}
      />
      <p>
        Now, for a given set of parameters{' '}
        <InlineMath>{`\\{ p_i(0), \\mu_i, \\sigma_i \\}_i^N`}</InlineMath>, our first goal will be
        to compute the <em>expected values</em> of{' '}
        <InlineMath>{`(\\alpha_i^*,\\tau_i^*)`}</InlineMath>, i.e., the cumulative injected tokens,
        up to some time <InlineMath>{`T`}</InlineMath>:
      </p>

      <Equations
        equNo={25}
        minify={true}
        equ={`E[\\tau_i^*(T)] = \\text{expected cumulative TAO injected}`}
      />
      <Equations
        equNo={26}
        minify={true}
        equ={`E[\\alpha_i^*(T)] = \\text{expected cumulative Alpha injected}`}
      />
      <p>
        To compute these, we accept the following approximation: we suppose the sums in
        (19)&#45;(20) can be approximated as integrals. For example, with the expected cumulative
        Alpha in (27), we develop as follows:
      </p>
      <Equations
        equNo={27}
        minify={true}
        equ={`
          E\\big[ \\alpha_i^*(T)\\big] 
          = E\\left[\\sum_t^T \\Delta \\alpha_i(t)\\right]  \\\\
          = \\sum_t^T E\\left[\\Delta \\alpha_i(t)\\right]  \\\\
          =  \\sum_t^T E\\Big[\\min\\{\\Delta \\overline{\\tau}/S(t),\\Delta \\overline{\\alpha}_i\\}\\Big] \\\\
          =  \\sum_t^T E\\left[\\min\\left\\{\\frac{(\\Delta \\overline{\\tau}/\\Delta t)}{S(t)},(\\Delta \\overline{\\alpha}_i/\\Delta t)\\right\\}\\right] \\Delta t \\\\
          \\approx \\int_0^T E\\Big[\\min\\{\\Delta \\overline{\\tau}/S(t),\\Delta \\overline{\\alpha}_i\\}\\Big] dt
        `}
      />
      <p>
        This approximation should be reasonable since the sums happen over frequently occurring
        blocks, and are hence nearly continuous, while the rates{' '}
        <InlineMath>{`(\\Delta \\overline{\\tau}/\\Delta t)`}</InlineMath> and{' '}
        <InlineMath>{`(\\Delta \\overline{\\alpha}_i/\\Delta t)`}</InlineMath> are constant and can
        simply be replaced with <InlineMath>{`\\Delta \\overline{\\tau}_i`}</InlineMath> and{' '}
        <InlineMath>{`\\Delta \\overline{\\alpha}_i`}</InlineMath>. Next, we approximate the
        expected cumulative TAO:
      </p>
      <Equations
        equNo={28}
        equ={`E\\left[\\tau_i^*(T)\\right] \\approx \\int_0^T E\\Bigg[ \\frac{p_i(t)}{S(t)} \\Delta \\overline{\\tau} \\Bigg] dt`}
      />
      <p>
        Unfortunately, the appearance <InlineMath>{`S(t)`}</InlineMath> in the integrand of (28) and
        (29) makes them exceedingly difficult to calculate. In particular,{' '}
        <InlineMath>{`S(t)`}</InlineMath> depends on the entire set of prices{' '}
        <InlineMath>{`\\{p_i(t)\\}`}</InlineMath>, and so the expectation{' '}
        <InlineMath>{`E[\\cdot]`}</InlineMath> must be taken over the <em>joint</em> probability
        distribution of all of the <InlineMath>{`N`}</InlineMath>&#45;many price paths. Instead, we
        make our next approximation; <em>we will replace </em>
        <InlineMath>{`S(t)`}</InlineMath> <em> with its expectation </em>
        <InlineMath>{`E[S(t)]`}</InlineMath>, denoted by{' '}
        <InlineMath>{`\\overline{S}(t)`}</InlineMath>.
      </p>
      <Equations
        equNo={29}
        minify={true}
        equ={`
          \\overline{S}(t) := E[S(t)] = E\\Big[\\sum_i p_i(t)\\Big] = \\sum_i E[p_i(t)]
        `}
      />

      <p>
        The substitution <InlineMath>{`S(t) \\approx \\overline{S}(t)`}</InlineMath> is not
        completely unjustified; <InlineMath>{`S(t)`}</InlineMath> is a sum of approximately normal
        random variables, and by the generalized Central Limit Theorem, it will also be
        approximately normally distributed around its mean, with a relatively small variance over
        reasonably small time scales. Indeed, we will ultimately check it numerically.
      </p>
      <p>
        Now, to compute the expectation of each price{' '}
        <InlineMath>{`E\\big[p_i(t)\\big]`}</InlineMath>, we use the probability density given in
        (25).
      </p>
      <Equations
        equNo={30}
        minify={true}
        equ={`
          \\int_0^{\\infty} \\frac{p}{\\sqrt{2\\pi\\sigma_i^2 t}} \\frac{1}{p} 
          \\exp\\left(\\frac{-\\big(\\log[p/p_i(0)] - \\mu_i t\\big)^2}{2\\sigma_i^2 t}\\right) dp
        `}
      />
      <p>
        With the change of variables{' '}
        <InlineMath>{`x := \\log\\left[\\frac{p}{p_i(0)}\\right]`}</InlineMath>, the integral in
        (31) can then be written as:
      </p>
      <Equations
        equNo={31}
        minify={true}
        equ={`
          E[p_i(t)] = p_i(0) 
          \\int_{-\\infty}^{\\infty} e^{x} 
          \\frac{1}{\\sqrt{2\\pi\\sigma_i^2 t}} 
          \\exp\\left(\\frac{-(x - \\mu_i t)^2}{2\\sigma_i^2 t}\\right)dx
        `}
      />
      <p>We can evaluate (32) by use of the standard formula</p>
      <Equations
        equNo={32}
        minify={true}
        equ={`
            \\int_{-\\infty}^{\\infty} e^{cx} 
            \\frac{1}{\\sqrt{2\\pi a}} 
            \\exp\\left(\\frac{-(x - d)^2}{2a}\\right)dx 
            = \\exp\\left(cd + \\frac{1}{2}ac^2\\right)
          `}
      />
      <p>
        which can easily be derived by completing the square inside the exponential. Applying (33)
        to (32), we then find{' '}
      </p>
      <Equations
        equNo={33}
        equ={`
          E\\big[p_i(t)\\big] =  p_i {\\footnotesize (\\hspace{-0.05pc}0\\hspace{-0.05pc})} 
           e^{(\\mu_i+\\sigma_{i}^2\\hspace{-0.1pc}/2)t}
          `}
      />
      <p>
        Thus, expression (30) for <InlineMath>{`\\overline{S}(t)`}</InlineMath> then becomes
      </p>
      <Equations
        equNo={34}
        equ={`
          \\overline{S}(t) = \\sum_i p_i{( 0 )} e^{(\\mu_i+\\sigma_{i}^2/2)t}
        `}
      />
      <p>
        We note that, unlike the quantity <InlineMath>{`S(t)`}</InlineMath>, the quantity{' '}
        <InlineMath>{`\\overline{S}(t)`}</InlineMath> is <em>not</em> a random variable distributed
        with GBM, but is instead a deterministic function of <InlineMath>{`t`}</InlineMath>. Thus,
        using our third approximation, we replace <InlineMath>{`S(t)`}</InlineMath> with{' '}
        <InlineMath>{`\\overline{S}(t)`}</InlineMath> in (28)&#45;(29) and we find that it can now
        slip outside the expectation value:
      </p>
      <Equations
        equNo={35}
        minify
        equ={`
            E\\left[\\tau_i^*(T)\\right] \\approx \\int_0^T E\\Big[\\frac{p_i(t)}{S(t)} \\Delta \\overline{\\tau}\\Big] dt \\\\
            \\approx \\int_0^T E\\big[p_i(t)\\big]\\frac{\\Delta \\overline{\\tau}}{\\overline{S}(t)} dt
          `}
      />
      <Equations
        equNo={36}
        minify={true}
        equ={`
          E\\left[ \\alpha_i^*(T)\\right] \\approx \\int_0^T E\\Big[\\min\\{\\frac{\\Delta \\overline{\\tau}}{\\overline{S}(t)},\\Delta \\overline{\\alpha}_i\\}\\Big] dt \\\\
          \\approx \\int_0^T E\\big[1\\big] \\min\\{\\frac{\\Delta \\overline{\\tau}}{\\overline{S}(t)},\\Delta \\overline{\\alpha}_i\\} dt
        `}
      />
      <p>
        We know that <InlineMath>{`E[1] = 1`}</InlineMath>, and we know
        <InlineMath>{`E[p_i(t)]`}</InlineMath> from (34). Thus, (36)&#45;(37) become:
      </p>
      <Equations
        equNo={37}
        minify={true}
        equ={`
        E\\left[\\tau_i^*(T)\\right] \\approx p_i {(0)}
        \\int_0^T \\frac{\\Delta \\overline{\\tau}}{\\overline{S}(t)} 
        e^{(\\mu_i+\\sigma_{i}^2/2)t} dt
      `}
      />
      <Equations
        equNo={38}
        minify={true}
        equ={`
          E\\left[ \\alpha_i^*(T)\\right] \\approx \\int_0^T  
          \\min\\left\\{ \\frac{\\Delta \\overline{\\tau}}{\\overline{S}(t)}, \\Delta \\overline{\\alpha}_i \\right\\} dt
        `}
      />
      <p>Finally, we note that using</p>
      <Equations
        equNo={39}
        minify={true}
        equ={`
            \\min\\left\\{ \\frac{\\Delta \\overline{\\tau}}{\\overline{S}(t)}, \\Delta \\overline{\\alpha}_i \\right\\} = 
            \\frac{\\Delta \\overline{\\tau}}{\\max\\big\\{ \\overline{S}(t), \\Delta \\overline{\\tau} / \\Delta \\overline{\\alpha}_i \\big\\}}
          `}
      />
      <p>we can rewrite (38)&#45;(39) in a more explicit way:</p>
      <Equations
        equNo={40}
        minify={true}
        equ={`
          E\\left[ \\tau_i^*(T) \\right] \\approx \\int_0^T \\frac{ \\Delta \\overline{\\tau} \\hspace{0.15pc} p_i ( 0 ) e^{(\\mu_i+\\sigma_{i}^2/2)t} }
          { \\sum_j p_j ( 0 ) e^{(\\mu_j+\\sigma_{j}^2/2)t} } dt
        `}
      />
      <Equations
        equNo={41}
        minify={true}
        equ={`
    E\\left[ \\alpha_i^*(T) \\right] \\approx \\int_0^T \\frac{\\Delta \\overline{\\tau}}
    { \\max \\left\\{ \\sum_j p_j ( 0 ) e^{(\\mu_j+\\sigma_{j}^2/2)t}, \\Delta \\overline{\\tau} / \\Delta \\overline{\\alpha}_i \\right\\} } dt
  `}
      />
      <p>
        Our final approximation will be made in an effort to compute the expected market cap for
        each subnet pool. When denominated in TAO, the market cap of the{' '}
        <InlineMath>{`i^{th}`}</InlineMath> subnet pool, denoted
        <InlineMath>{`M_i(t)`}</InlineMath>, is given by:
      </p>
      <Equations
        equNo={42}
        minify={true}
        equ={`
          M_i(t) = \\left[ \\begin{array}{c} \\text{total Alpha supply} \\\\ \\text{at time } t \\end{array} \\right] \\times \\text{ price }
        `}
      />
      <p>
        The price at time <InlineMath>{`t`}</InlineMath> is of course given by{' '}
        <InlineMath>{`p_i(t)`}</InlineMath>. To calculate the total supply, we note that all the
        Alpha that can ever exist must come from the emissions. Thus, we simply need to add the
        cumulative emissions <InlineMath>{`\\alpha_i^*(t)`}</InlineMath> to any initial Alpha{' '}
        <InlineMath>{`\\alpha_i(0)`}</InlineMath>, and this will give us our total supply. However,
        for good measure we will also include the emissions that go to users as rewards. These
        rewards were not included in our toy model so far, but it is a trivial thing to include. We
        know from section 3.5 that an amount{' '}
        <InlineMath>{`\\Delta \\overline{\\alpha}_i`}</InlineMath> is emitted every block, and so
        the total amount up to time <InlineMath>{`t`}</InlineMath> is{' '}
        <InlineMath>{`\\Delta \\overline{\\alpha}_i t`}</InlineMath> (assuming{' '}
        <InlineMath>{`t`}</InlineMath> is measured in blocks). Thus, our total expression for market
        cap will be given by:
      </p>
      <Equations
        equNo={43}
        minify={true}
        equ={`
        M_i(t) = \\Big( \\alpha_i(0) + \\alpha_i^*(t) + \\Delta \\overline{\\alpha}_i t \\Big) p_i(t)
      `}
      />
      <p>
        Our interest will be in the expected market cap at time <InlineMath>{`t=T`}</InlineMath>,
        and so we write the following:
      </p>
      <Equations
        equNo={44}
        minify={true}
        equ={`
        E\\big[M_i(T)\\big] = E\\Big[  \\Big(\\alpha_i (0) + \\alpha_i^*(T) + \\Delta \\overline{\\alpha}_i T\\Big)p_i(T) \\Big] \\\\
        =  \\big(\\alpha_i (0) + \\Delta \\overline{\\alpha}_i T\\big)E\\big[p_i(T)\\big] + E\\big[\\alpha_i^*(T) p_i(T) \\big] 
      `}
      />
      <p>
        Now, the expression <InlineMath>{`E\\big[\\alpha_i^*(T) p_i(T) \\big]`}</InlineMath> would,
        in principle, be difficult to compute, as the expectation would be taken over the{' '}
        <em>joint</em> probability distribution of <InlineMath>{`\\alpha_i^*(T)`}</InlineMath> and{' '}
        <InlineMath>{`p_i(T)`}</InlineMath>. However, because of our approximation (30), the
        quantity <InlineMath>{`\\alpha_i^*(T)`}</InlineMath> is not actually a random variable.
        Thus, we may distribute the expectation over{' '}
        <InlineMath>{`E[\\alpha_i^*(T) p_i(T)]`}</InlineMath>, obtaining:
      </p>
      <Equations
        equNo={45}
        minify={true}
        equ={`
          E\\big[M_i(T)\\big] \\approx \\big(\\alpha_i (0) + \\Delta \\overline{\\alpha}_i T\\big) E\\big[p_i(T)\\big] + E\\big[\\alpha_i^*(T)\\big]E\\big[p_i(T)\\big]  \\\\
          = E\\big[p_i(T)\\big] \\Big( \\alpha_i (0) + \\Delta \\overline{\\alpha}_i T + E\\big[\\alpha_i^*(T)\\big] \\Big)
        `}
      />
      <p>
        Importantly, we see that expression (46) is written in terms of quantities that we have
        already worked out, namely (34) and (42). This will be our expression for the final market
        cap.
      </p>
      <p>Summary So Far</p>
      <p>
        Given prices <InlineMath>{`p_i(t)`}</InlineMath> with GBM parameters{' '}
        <InlineMath>{`\\{p_i(0), \\mu_i, \\sigma_i\\}`}</InlineMath>, and injection per unit time{' '}
        <InlineMath>{`b`}</InlineMath>, the expected accumulated alpha and tao tokens{' '}
        <InlineMath>{`\\{\\overline{\\alpha}_i(T), \\overline{\\tau}_i(T)\\}`}</InlineMath>, as well
        as the subnet market caps <InlineMath>{`M_i(T)`}</InlineMath>, at some time{' '}
        <InlineMath>{`T`}</InlineMath>, are approximated by:
      </p>
      <Equations
        equNo={46}
        minify={true}
        equ={`
          E\\left[\\tau_i^*(t)\\right] \\approx \\int_0^T 
          \\frac{ \\Delta \\overline{\\tau} \\hspace{0.15pc} p_i ( 0 ) e^{(\\mu_i+\\sigma_{i}^2/2)t} }
          { \\sum_j p_j ( 0 ) e^{(\\mu_j+\\sigma_{j}^2/2)t} } dt
        `}
      />
      <Equations
        equNo={47}
        minify={true}
        equ={`
        E\\left[ \\alpha_i^*(t) \\right] \\approx \\int_0^T 
        \\frac{ \\Delta \\overline{\\tau} }
        { \\max\\left\\{ \\sum_j p_j ( 0 ) e^{(\\mu_j+\\sigma_{j}^2/2)t}, \\Delta \\overline{\\tau} / \\Delta \\overline{\\alpha}_i \\right\\} } dt
      `}
      />
      <Equations
        equNo={48}
        minify={true}
        equ={`
        E\\big[M_i (T)\\big] \\approx  E\\big[p_i(T)\\big] 
        \\Big( \\alpha_i (0) + \\Delta \\overline{\\alpha}_i T + E\\big[\\alpha_i^* (T)\\big] \\Big)
      `}
      />
      <h3 className={styles.subtitle}>Section 4.3: Numerical Verification</h3>

      <p>
        To test our formulas, we run some numerical simulations. In particular, the purpose of the
        simulations in this section is simply to test the validity of our approximate expectation
        values given in (47)&#45;(49). One should note, these will not be thorough agent&#45;based
        simulations of the DTAO system in its full generality, but rather they are simulations of
        our idealized mathematical model articulated by the update rules (17)&#45;(18), driven by
        simulated Geometric Brownian Motion price movements.
      </p>
      <p>
        In order to completely visualize the results, we start with an unrealistically low number of
        subnets at
      </p>
      <BlockMath>{`N = 4 \\ subnets`}</BlockMath>
      <p>
        We use select the following drift and volatility parameters to generate a variety of price
        trajectories:
      </p>
      <BlockMath>{`\\mu = [\\text{-}4,0,10,2] \\times 10^{\\text{-}8} \\\\ \\sigma = [5,6,5,6] \\times 10^{\\text{-}5}`}</BlockMath>
      <p>
        Over a period of <InlineMath>{'T = 4'}</InlineMath> years, and{' '}
        <InlineMath>{'\\Delta \\overline{\\tau} = \\Delta \\overline{\\alpha}_i = 1'}</InlineMath>{' '}
        for all <InlineMath>{'i'}</InlineMath>.We also note that this simulation does not include
        any halving events (indeed, the halving schedule was not factored in to our mathematical
        model from the previous section at all).
      </p>

      <p>
        We now run 1,000 trials. For each trial, we generate four separate independent GBM price
        paths (one for each subnet) according to the values of <InlineMath>{'\\mu '}</InlineMath>{' '}
        and <InlineMath>{`\\sigma `}</InlineMath> just given. First, we simply confirm the correct
        nature of our GBM by collecting the final prices for each subnet during each trial, and plot
        the distribution of final prices. For good measure, we also plot the log&#45;normal
        distribution that we would mathematically expect for the probability density given in
        formula (25). The results are plotted in figure 6.
      </p>
      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_6.jpg'
          alt='The distribution of final prices under GBM'
          className={styles.image_container_image}
        />
        <p>
          <span className={styles.image_container_caption_no}>Figure 6: </span>
          The distribution of final prices under GBM
        </p>
      </div>
      <p>
        We next plot the distributions for our three quantities of interest; the cumulative injected
        Alpha and TAO tokens, and the final market cap, shown in figure 7 below. Indeed, we have
        markers to indicate the mean values from our raw simulation data, along with the values that
        our formulas (47)&#45;(49) would predict. We make three observations:
      </p>
      <ul className={styles.unorder_list}>
        <li>The data means agree excellently with our formulas</li>
        <li>
          The Alpha injections are identical for all subnets (this should be clear from the
          injection formula <InlineMath>{'(7)'}</InlineMath> &#45; it does not depend on{' '}
          <InlineMath>{'i'}</InlineMath>, outside of{' '}
          <InlineMath>{'\\Delta \\overline{\\alpha}_i'}</InlineMath>).
        </li>
        <li>
          The distributions of final prices, cumulative TAO, and market cap are all qualitatively
          similar.
        </li>
      </ul>
      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_7.jpg'
          alt='The distributions of final quantities'
          className={styles.image_container_image}
          style={{maxWidth: '100%'}}
        />
        <p>
          <span className={styles.image_container_caption_no}>Figure 7: </span>
          The distributions of final quantities
        </p>
      </div>

      <h3 className={styles.subtitle}>Section 4.4: Some Case Studies</h3>
      <p>
        Now that we feel confident in our expectation formulas{' '}
        <InlineMath>{'(47)-(49)'}</InlineMath>, let us use them in specific case studies. We begin
        with a simple scenario wherein one subnet has a noticeable upwards drift in price, while the
        other subnet prices remain relatively stable. Specifically, we will use{' '}
        <InlineMath>{'N=64'}</InlineMath> subnets, over a period <InlineMath>{'T=1'}</InlineMath>{' '}
        year, with block emission of{' '}
        <InlineMath>{'\\Delta \\overline{\\tau}, \\Delta \\overline{\\alpha}_i=1'}</InlineMath>. We
        assign a normal spread of negligible drift parameters to the first 63 subnets, and we give
        the <InlineMath>{'64^{th}'}</InlineMath> a noticeably higher drift. Moreover, we assign
        similarly mild random volatility parameters to all subnets. We can visualize these parameter
        assignments below:
      </p>

      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_8.jpg'
          alt='Case study parameters'
          className={styles.image_container_image}
        />
      </div>
      <p>
        The drift parameter <InlineMath>{'\\mu'}</InlineMath> is hard to appreciate intuitively, but
        we convert it to the more relatable quantity{' '}
        <InlineMath>{'\\lambda := p(T)/p(0)'}</InlineMath> (the price growth factor) by the equation{' '}
        <InlineMath>{'\\lambda = e^{\\mu T}'}</InlineMath>. Using this, we plot the equivalent
        spread of <InlineMath>{'\\lambda'}</InlineMath> values:
      </p>

      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_9.jpg'
          alt='Lambda values spread'
          className={styles.image_container_image}
        />
      </div>
      <p>
        Thus, for example, our scenario corresponds with a subnet whose price <em>doubles</em> over
        the course of the year, while all other subnets move by about{' '}
        <InlineMath>{'\\pm 5\\%'}</InlineMath>. Moreover, now that we have established the GBM
        setting, we use our formulas to compute expected values after one year. We plot the
        cumulative Alpha and TAO, and the final market cap:
      </p>

      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_10.jpg'
          alt='Expected values for the end of the year'
          className={styles.image_container_image}
        />
      </div>
      <p>
        We can make a few observations from the previous figure. For our scenario with one subnet
        experiencing a doubling in price over the year, we can say the following:
      </p>
      <ul className={styles.unorder_list}>
        <li>
          It will receive about <InlineMath>{'45\\%'}</InlineMath> more TAO tokens than the other
          subnets (note the <InlineMath>{'E[\\tau]'}</InlineMath> values of{' '}
          <InlineMath>{'\\approx 40,000'}</InlineMath>
          for the general subnets, compared to <InlineMath>{'\\approx 58,000'}</InlineMath> for our
          special subnet; this is a <InlineMath>{'\\approx 45\\%'}</InlineMath> increase).
        </li>
        <li>
          All subnets have the same expected cumulative Alpha tokens — again, not a surprise, as we
          previously noted.
        </li>
        <li>Our subnet can expect about 70% greater market cap at the end of the year</li>
      </ul>
      <p>
        What if we give our subnet some competition? Let's take our previous scenario but suppose
        that there is a group of, say, ten other subnets who have similarly desirable drift
        parameters. We find the following:
      </p>
      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_11.jpg'
          alt='Expected values for the end of the year'
          className={styles.image_container_image}
        />
      </div>
      <p>
        Not surprisingly, our subnet now earns slightly less TAO and overall market cap than in the
        previous example.
      </p>
      <p>
        Finally, let us suppose that the subnets have a wide distribution of drift parameters, and
        that our subnet, while still experiencing a doubling of price, is situated somewhere in the
        middle of the pack. We find the following:
      </p>
      <div className={styles.image_container}>
        <img
          src='/images/new_dtao_paper/figure_12.jpg'
          alt='Expected values for the end of the year'
          className={styles.image_container_image}
        />
      </div>
      <p>
        Armed with the formulas (47)&#45;(49), the interested reader may investigate many more kinds
        of market trajectories and case studies beyond these simple scenarios.
      </p>
    </section>
  );
};

export default Section4;
