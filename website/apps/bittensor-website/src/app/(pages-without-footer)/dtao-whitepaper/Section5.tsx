import React from 'react';
import {InlineMath} from 'react-katex';
import {Equations} from './components/Equations';
import styles from './page.module.css';

const Section5 = () => {
  return (
    <section className={styles.section}>
      <h2 className={styles.subtitle}>Section 5: Appendix</h2>
      <h3 className={styles.subtitle}>Section 5.1: Tao Weight APY Differential</h3>
      <p>
        In this section, we explore an interesting heuristic to help guide us toward a good choice
        for the Tao weight <InlineMath>{`\\gamma`}</InlineMath>. In particular, this heuristic will
        be based on comparing the APY earned by root stakers versus the subnet validators. As we saw
        in section 3.4, the amount of Alpha emission dedicated to the validators is given by{' '}
        <InlineMath>{`(0.41)\\Delta\\overline{\\alpha}_i`}</InlineMath>. This is then split by the
        value of the root proportion <InlineMath>{`r`}</InlineMath>
        into the following two pieces:
      </p>
      <Equations
        equNo={49}
        minify={true}
        equ={`
          \\ (root \\ dividends \\ from i^{th} subnet)  = r(0.41)\\Delta \\overline{\\alpha}_i
        `}
      />
      <Equations
        equNo={50}
        minify={true}
        equ={`
          \\ ( dividends \\ for i^{th} subnet validators)  = (1-r) (0.41)\\Delta \\overline{\\alpha}_i
        `}
      />
      <p>From our definition (12), we can write the following:</p>
      <Equations
        equNo={51}
        minify={true}
        equ={`
          r_i = \\frac{\\gamma\\hspace{0.1pc}\\tau_0}{\\gamma\\hspace{0.1pc}\\tau_0+\\alpha_i^o}, \\hspace{1pc} (1 - r_i) = \\frac{\\alpha_i^o}{\\gamma\\hspace{0.1pc}\\tau_0+\\alpha_i^o}
        `}
      />
      <p>where we recall that </p>
      <Equations equNo={52} minify={true} equ={`\\gamma = {the \\ tao \\ weight} \\\\`} />
      <Equations
        equNo={53}
        minify={true}
        equ={`
            \\tau_0 = { TAO \\ staked \\ on \\ root} \\\\
          `}
      />
      <Equations
        equNo={54}
        minify={true}
        equ={`
          \\alpha_i^o = Alpha \\ outstanding \\ (not \\ reserves)
        `}
      />
      <p>
        Now, for the Alpha outstanding term, we will make the following assumption; while Alpha may
        be coming in and out of the subnet pool, one could argue that if the subnet price remains
        relatively stable, then the Alpha outstanding should just be equal to the total cumulative
        Alpha emissions given out as dividends. Thus, starting at a value of{' '}
        <InlineMath>{`\\alpha^o_i(0)`}</InlineMath>, over a period of <InlineMath>{`t`}</InlineMath>{' '}
        emissions, we employ the following simple model:
      </p>
      <Equations
        equNo={55}
        equ={`
        \\alpha_i^o = \\alpha_i^o (0) + t \\Delta \\overline{\\alpha}_i
      `}
      />
      <p>
        To get the APY, we would need to sum over
        <InlineMath>{`t`}</InlineMath> appearing in (55) for the duration of a year. Rather than
        committing to a fixed time period, let us just consider generic returns. Moreover, to obtain
        these returns, we would divide the total rewards by the initial token held, i.e., either{' '}
        <InlineMath>{`\\tau_0`}</InlineMath> or
        <InlineMath>{`\\alpha_i^o (0)`}</InlineMath>. We do this next, while substituting (51) and
        (55) back into the reward expressions in (49) and (50). Altogether, we get the following
        expressions for the returns APY:
      </p>
      <Equations
        equNo={56}
        minify={true}
        equ={`
          \\ APY_i^{root} = 
          \\frac{1}{\\tau_0} \\sum_t \\frac{\\gamma \\tau_0 (0.41) \\Delta \\overline{\\alpha}_i }
          {\\big( \\gamma \\tau_0 + \\alpha_i^o (0) + t \\Delta \\overline{\\alpha}_i \\big)}
        `}
      />
      <Equations
        equNo={57}
        minify={true}
        equ={`
          \\ APY_i = 
          \\frac{1}{\\alpha_i^o (0)} \\sum_t 
          \\frac{(\\alpha_i^o (0) + t \\Delta \\overline{\\alpha}_i)(0.41) \\Delta \\overline{\\alpha}_i }
          {\\big( \\gamma \\tau_0 + \\alpha_i^o (0) + t \\Delta \\overline{\\alpha}_i \\big)}
        `}
      />
      <p>If we define</p>
      <Equations
        equNo={58}
        equ={`
          D_i(t) := \\big( \\gamma \\tau_0 + \\alpha_i^o (0) + t \\Delta \\overline{\\alpha}_i \\big)
        `}
      />
      <p>then (56)&#45;(57) can be written more simply as </p>
      <Equations
        equNo={59}
        equ={`
          \\sum_t \\frac{\\gamma  (0.41) \\Delta \\overline{\\alpha}_i }{D_i(t)}
        `}
      />
      <Equations
        equNo={60}
        minify={true}
        equ={`
          \\ APY_{i} =
          \\sum_t \\left(1+\\frac{t\\Delta\\overline{\\alpha}_i}{\\alpha_i^o {(0)}}\\right) 
          \\frac{(0.41)\\Delta \\overline{\\alpha}_i }{D_i(t)}
        `}
      />
      <p>
        Now, to get the entire root returns, we would need to sum these rewards over all subnets, as
        the root stakers get dividends from each subnet:
      </p>
      <Equations
        equNo={61}
        equ={`
          \\ APY^{root} = 
          \\sum_i\\sum_t \\frac{\\gamma  (0.41)\\Delta \\overline{\\alpha}_i }{D_i(t)}
        `}
      />
      <p>
        Similarly, we could compute the average subnet returns by summing (57) over all subnets and
        dividing by the number of subnets (let&apos;s call it N):
      </p>
      <Equations
        equNo={62}
        minify={true}
        equ={`
          \\ APY^{avg} = \\frac{1}{N} \\sum_i \\sum_t \\left(1+\\frac{t\\Delta\\overline{\\alpha}_i}{\\alpha_i^o(0)}\\right) 
          \\frac{(0.41)\\Delta \\overline{\\alpha}_i }{D_i(t)}
        `}
      />
      <p>
        We next suggest the following goal; let us choose the tao weight such that the root APY is
        less than or equal to the average subnet returns:
      </p>
      <Equations
        equNo={63}
        equ={`\\ APY^{root} \\leq \\ APY^{avg}
      `}
      />
      <p>Explicitly, this becomes</p>
      <Equations
        equNo={65}
        minify={true}
        equ={`
        \\sum_i\\sum_t \\frac{\\gamma (0.41)\\Delta \\overline{\\alpha}_i }{D_i(t)} 
        \\leq \\frac{1}{N}\\sum_i\\sum_t \\left(1+\\frac{t\\Delta\\overline{\\alpha}_i}{\\alpha_i^o (0)}\\right) 
        \\frac{(0.41)\\Delta \\overline{\\alpha}_i }{D_i(t)}
      `}
      />
      <p>This can be rearranged into the following:</p>
      <Equations
        equNo={66}
        minify={true}
        equ={`
          0 \\leq \\frac{1}{N}\\sum_i\\sum_t 
          \\left(1 - \\gamma N + \\frac{t\\Delta\\overline{\\alpha}_i}{\\alpha_i^o (0)}\\right) 
          \\frac{(0.41)\\Delta \\overline{\\alpha}_i }{D_i(t)}
        `}
      />
      <p>
        We note that nearly every quantity in (65) is positive, except for the quantity{' '}
        <InlineMath>{`1 - \\gamma N`}</InlineMath>. One way to guarantee that inequality (65) holds
        is to demand that <InlineMath>{`0 \\leq 1 - \\gamma N`}</InlineMath>. But this just
        simplifies to the following condition:
      </p>
      <Equations
        equNo={67}
        equ={`
          \\gamma \\le \\frac{1}{N}
        `}
      />
      <p>Thus, while this is surely a conservative choice, we can at least say the following:</p>
      <p>
        <i>
          If we take the tao weight to be the reciprocal of the number of subnets, then the root
          returns will be less than or equal to the average subnet returns
        </i>
      </p>
      <p>
        This could be a useful heuristic for choosing the tao weight, or at least gauge the
        appropriate order of magnitude.
      </p>
      <p>
        Interestingly, we can use expressions (60) and (61) to get an approximate closed form
        expression for the return values. Specifically, if we replace the discrete sums with
        integrals (which is reasonable given how quickly the blocks happen), then we find the
        following:
      </p>
      <Equations
        equNo={67}
        minify={true}
        equ={`
            APY^{root}  =
            \\sum\\nolimits_i\\sum_t \\frac{\\gamma  (0.41)\\Delta \\overline{\\alpha}_i }
            {\\big(\\gamma\\tau_0 + \\alpha_i^o(0) + t\\Delta \\overline{\\alpha}_i \\big)} \\\\
            \\approx \\sum\\nolimits_i\\int \\frac{\\gamma  (0.41)\\Delta \\overline{\\alpha}_i }
            {\\big(\\gamma\\tau_0 + \\alpha_i^o(0) + t\\Delta \\overline{\\alpha}_i \\big)} dt \\\\
            = \\sum\\nolimits_i\\gamma(0.41)\\ln\\big(\\gamma\\tau_0 + \\alpha_i^o(0) + t\\Delta \\overline{\\alpha}_i \\big)
        `}
      />
      <Equations
        equNo={68}
        minify={true}
        equ={`
          \\ APY_i =
          \\sum_t \\left(1+\\frac{t\\Delta\\overline{\\alpha}_i}{\\alpha_i^o(0)}\\right) 
          \\frac{(0.41)\\Delta \\overline{\\alpha}_i }{\\big(\\gamma\\tau_0 + \\alpha_i^o(0) + t\\Delta \\overline{\\alpha}_i \\big)} \\\\
          \\approx \\int \\left(1+\\frac{t\\Delta\\overline{\\alpha}_i}{\\alpha_i^o(0)}\\right) 
          \\frac{(0.41)\\Delta \\overline{\\alpha}_i }{\\big(\\gamma\\tau_0 + \\alpha_i^o(0) + t\\Delta \\overline{\\alpha}_i \\big)} dt \\\\
          = (0.41)\\left( \\frac{\\Delta \\overline{\\alpha}_i}{\\alpha_i^o(0)}t - 
          \\frac{\\gamma \\tau_0}{\\alpha_i^o(0)}\\ln\\big(\\gamma\\tau_0 + \\alpha_i^o(0) + t\\Delta \\overline{\\alpha}_i \\big) \\right)
        `}
      />
      <p>
        Thus, we see that the subnet token holders have returns that grow
        <strong> linearly </strong> over time, whereas the passive root stakers have returns that
        only grow <strong> logarithmically </strong>.
      </p>
      <h3 className={styles.subtitle}>Section 5.2: The Mechanics of Halving</h3>
      <p>
        In this section, we look at the math behind the halving schedule as computed in practice. We
        consider a scenario where we have an initial quantity (say, 1) that we add up N many times.
        After this point, we halve the quantity and add it up N many times again. This is done
        indefinitely:
      </p>
      <Equations
        minify={true}
        equ={`\\big[1+1+...+1\\big]+\\big[\\frac{1}{2}+\\frac{1}{2}+...+\\frac{1}{2}\\big]+\\big[\\frac{1}{4}+\\frac{1}{4}+...+\\frac{1}{4}\\big]+...`}
      />
      <p>
        This is meant to represent the accumulating token supply. For example, if we let{' '}
        <InlineMath>{'N = 10,500,000'}</InlineMath>, then we have the TAO halving schedule
        represented as the following:
      </p>
      <Equations
        equNo={69}
        minify={true}
        equ={`
          \\big[ 10,500,000 \\big] + \\big[5,250,000\\big] + \\big[2,625,000\\big] + ...
        `}
      />
      <p>
        Now, let us denote the current accumulated supply by <InlineMath>{`S`}</InlineMath>. The{' '}
        <em>eventual total</em> token supply, which we denote by <InlineMath>{`S^*`}</InlineMath>,
        will be given by using the well&#45;known formula for summing a convergent geometric series:
      </p>
      <Equations
        minify={true}
        equ={`
          \\big[N\\times1\\big] + \\big[N\\times\\frac{1}{2}\\big] + \\big[N\\times\\frac{1}{4}\\big] + ... = N\\Big[\\frac{1}{1-1/2}\\Big] = 2N
        `}
      />
      <p>and so we see that </p>
      <Equations equNo={70} equ={`S^* = 2N`} />
      <p>
        Thus, for example, if <InlineMath>{`N = 10,500,000`}</InlineMath>, then we see that the
        total supply of TAO will approach <InlineMath>{`S^* = 21,000,000`}</InlineMath>.
      </p>
      <p>
        Now, suppose we are currently at some point in this process, where the amounts being
        currently summed are <InlineMath>{`(1/2)^k`}</InlineMath>, indicated below:
      </p>
      <Equations
        equNo={71}
        equ={`
          ... + \\big(N\\times\\frac{1}{2^k}\\big)+...
        `}
      />
      <p>
        Let <InlineMath>{`S_{\\downarrow}`}</InlineMath> denote the accumulated supply up to the
        previous halving point, which we can sum as a <em>finite</em> geometric series:
      </p>
      <Equations
        equNo={72}
        minify={true}
        equ={`
          S_{\\downarrow} = K\\times \\sum_{n=0}^{k-1} (1/2)^n =
          N \\left(\\frac{1-(1/2)^k}{1-(1/2)}  \\right)
          = 2N\\big(1-(1/2)^k\\big)
        `}
      />
      <p>
        In a similar way, we let <InlineMath>{`S_{\\uparrow}`}</InlineMath> denote the future
        accumulated supply at the point of the next future halving event.
      </p>
      <Equations
        equNo={73}
        equ={`
        S_{\\uparrow} = S^* \\big(1 - (1/2)^{k+1}\\big)
      `}
      />
      <p>We can solve equation (73) for the exponent k</p>
      <Equations
        equNo={74}
        equ={`
        k = -\\log_2\\left(1-\\frac{S_{\\downarrow}}{S^*}\\right)
      `}
      />
      <p>
        We can do the same for equation (74) and solve for <InlineMath>{`k+1`}</InlineMath>:
      </p>
      <Equations
        equNo={75}
        equ={`
          k+1 = -\\log_2\\left(1-\\frac{S_{\\uparrow}}{S^*}\\right)
        `}
      />
      <p>
        Now, because the current supply satisfies{' '}
        <InlineMath>{`S_{\\downarrow} \\leq S \\leq S_{\\uparrow}`}</InlineMath>, and because the
        logarithm is a monotonically increasing function, then we can say the following:
      </p>
      <Equations
        equNo={76}
        equ={`
          k \\leq -\\log_2\\left(1 - \\frac{S}{S^*} \\right) \\leq k+1
        `}
      />
      <p>In other words, we can compute k by the expression</p>
      <Equations
        equNo={77}
        equ={`
            k = \\left\\lfloor  -\\log_2\\left(1-\\frac{S}{S^*}\\right) \\right\\rfloor
          `}
      />
      <p>
        Thus, at any point in time, we use the cumulative supply <InlineMath>{`S`}</InlineMath> and
        the total target supply <InlineMath>{`S^*`}</InlineMath>, and we can compute the amount of
        block emission <InlineMath>{`N/2^k`}</InlineMath> according to (77).
      </p>
      <h3 className={styles.subtitle}>Section 5.3: Root Reward Claiming (Accounting)</h3>
      <p>
        The root dividends (which are taken from the Alpha emissions) may be given to the root
        stakers in several ways. Initially, it will be done by auto&#45;selling the dividends (in
        Alpha) into the subnet AMM and thus receiving TAO to then pass on to the root stakers
        (distributed on a pro&#45;rate basis).
      </p>
      <p>
        However, there may be a point when this is switched over to an alternative system where
        Alpha dividends are given directly to the root stakers as Alpha tokens. In this case, the
        Alpha dividends that are delivered to the root subnet accumulate and can be claimed at any
        point by users (again, on a pro&#45;rata basis, depending on how much TAO they hold). In
        this section, we look at a particular kind of efficient accounting done for the root
        dividends. This is straightforward to compute and store in principle, but the efficient
        accounting method in use is somewhat opaque. To describe it, we define the following
        variables:
      </p>
      <ul className={styles.unorder_list}>
        <li>
          <InlineMath>{'\\Delta \\alpha'}</InlineMath> = dividend injection of Alpha
        </li>
        <li>
          <InlineMath>{'\\tau'}</InlineMath> = TAO staked on root by a user
        </li>
        <li>
          <InlineMath>{'T'}</InlineMath> = total TAO staked on root
        </li>
        <li>
          <InlineMath>{'\\rho'}</InlineMath> = a variable called &apos;rewards_per_tao&apos;
        </li>
        <li>
          <InlineMath>{'\\delta'}</InlineMath> = a variable called &apos;debt&apos;
        </li>
        <li>
          <InlineMath>{'\\alpha_c'}</InlineMath> = Alpha that can be claimed by the user
        </li>
      </ul>
      <p>
        Now, for an injection <InlineMath>{'\\Delta \\alpha'}</InlineMath>, a user is entitled to
        the quantity:
      </p>
      <Equations equNo={78} equ={`\\alpha_c = \\frac{\\tau}{T}\\,\\Delta \\alpha`} />
      <p>
        In other words, it&apos;s the fraction of the Alpha rewards that is equal by the user&apos;s
        fraction of TAO ownership. Next we define the following update formulas (where an apostrophe
        denotes an updated value):
      </p>
      <Equations
        equNo={79}
        minify
        equ={`
         \\ (initialize)  \\hspace{0.5pc} \\rho=0, \\hspace{0.5pc} \\delta = 0
        `}
      />
      <Equations
        equNo={80}
        minify={true}
        equ={`
          \\ (at stake \\Delta \\tau) \\hspace{0.5pc} 
          \\big\\{\\tau' = \\tau\\hspace{-0.2pc}+\\hspace{-0.2pc}\\Delta \\tau, 
          \\hspace{0.5pc} \\delta' = \\delta\\hspace{-0.2pc}+\\hspace{-0.2pc}\\rho\\Delta \\tau \\big\\}
        `}
      />
      <Equations
        equNo={81}
        minify={true}
        equ={`
          \\ (at \\ inject \\Delta \\alpha) \\hspace{0.5pc} 
          \\rho' = \\rho \\hspace{-0.1pc}+\\hspace{-0.2pc} \\frac{\\Delta \\alpha}{T}
        `}
      />
      <Equations
        equNo={82}
        minify={true}
        equ={`
          \\ (to \\ claim) \\hspace{0.5pc} 
          \\alpha_c = \\tau\\rho - \\delta
        `}
      />
      <p>
        We can confirm by induction that these formulas (77)&#45;(80) achieve their desired goals.
        First, after an initial injection, equation (80) reduces to (76). Then, if we assume formula
        (80) holds for our inductive step, we consider another stake and/or injection. The amount
        that user should now be able to claim is{' '}
        <InlineMath>{`\\alpha_c + (\\tau'/T)\\Delta \\alpha`}</InlineMath>. However, we can
        rearrange this:
      </p>
      <Equations
        equNo={83}
        minify={true}
        equ={`
          \\alpha_c' 
          = \\alpha_c + (\\tau'/T)\\Delta \\alpha \\\\
          = (\\tau\\rho-\\delta) + (\\tau+\\Delta \\tau)\\Delta \\alpha/T + \\rho\\Delta \\tau - \\rho\\Delta \\tau \\\\
          = (\\tau+\\Delta \\tau)(\\rho+\\Delta \\alpha/T) - (\\delta + \\rho\\Delta \\alpha) \\\\
          = \\tau'\\rho' - \\delta'
        `}
      />

      <p>which completes the inductive step.</p>
    </section>
  );
};

export default Section5;
