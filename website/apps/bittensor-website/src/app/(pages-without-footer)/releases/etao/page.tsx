import FadeInWrapper from '@/app/components/FadeInWrapper';
import {Link} from '@raofoundation/ui';
import type {Metadata} from 'next';
import {Suspense} from 'react';
import styles from '../v436-upgrade/page.module.css';

export const metadata: Metadata = {
  title: 'Etao — Subnets Choose Their Emission Split',
  description:
    'Etao (runtime v480, finalised in v481) gives every subnet owner a five-slot emission ' +
    'weight vector: how much of each block goes to the owner, to validators, and to miners. ' +
    'Root TAO stakers keep their baseline payout no matter what the owner picks, because the ' +
    'chain pays root its baseline share off the top of the validator slot every block. One ' +
    'extrinsic, one weekly rate limit, five rules, zero change to payouts at upgrade.',
  alternates: {canonical: '/releases/etao'},
  openGraph: {images: '/images/og_thumbs/etao.png'},
};

const DocLink = ({href, children}: {href: string; children: React.ReactNode}) => (
  <Link href={href} className={styles.inline_link}>
    {children}
  </Link>
);

const page = () => {
  return (
    <Suspense fallback={<div style={{minHeight: '100vh', backgroundColor: 'white'}} />}>
      <FadeInWrapper className={styles.page_container}>
        <section className={styles.title_section}>
          <h1 className={styles.paper_title}>Etao</h1>
          <p className={styles.subtitle} style={{fontSize: '10px'}}>
            Subnets choose their emission split · Runtime v480 → v481 · October 2026
          </p>
        </section>

        <section className={styles.section}>
          <img
            className={styles.graph}
            src='/images/content/etao-emission-split.svg'
            alt="Two pie charts. Before Etao: 18% owner, 41% validators with root TAO stakers taking 5.4% of emission from that slot, 41% miners. With Etao, an SN4-style subnet lowers validators to 20% and raises miners to 62%; root's 5.4% slice is unchanged because it is paid off the top of the validator slot every block from the chain's live baseline."
          />
          <p className={styles.graph_caption}>
            Before Etao every subnet paid the same fixed split. With Etao the owner moves the dial —
            here an SN4-style subnet pays miners 62% instead of 41% — and root TAO stakers&apos;
            slice of the pie does not move: it is paid off the top of the validator slot every
            block, at the chain&apos;s live baseline. Illustrative numbers.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Introduction</h2>
          <p>
            <strong>Etao</strong> hands every subnet owner the dial that has been welded shut since
            launch: how the subnet&apos;s alpha emission is divided. Until now every subnet paid the
            same split — 18% to the owner, 41% to validators, 41% to miners. From spec{' '}
            <strong>480</strong> a subnet that lives on its miners can pay them 62%. A subnet that
            needs deep validation can push validators to the 50% cap. An owner who wants to signal
            commitment can set their own cut to zero.
          </p>
          <p>
            One extrinsic, one rate limit, five rules, and a guarantee underneath all of it:{' '}
            <strong>
              root TAO stakers are paid exactly what they would be on an untouched subnet, whatever
              the owner picks.
            </strong>{' '}
            The guarantee is not a promise; it is arithmetic the chain runs every block.
          </p>
          <p>
            Etao ships to mainnet as a sudo runtime upgrade from <code>main</code>. Payouts in the
            first post-upgrade block are identical to the last pre-upgrade block; nothing moves
            until an owner moves it. The complete, standalone release note — every rule, error,
            storage item and worked CLI example — is the{' '}
            <DocLink href='/docs/guides/etao'>Etao release guide</DocLink>.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Why</h2>
          <p>
            Subnets are not alike. Some are miner-heavy compute markets where validation is cheap
            and the scarce resource is miner effort. Some are judgment-heavy and need validators to
            spend real money to score work. Some are owner-run products where the owner cut funds
            the team; others want that cut gone so every token flows to participants. A single fixed
            split cannot serve all of them, and until now the only knob — the owner cut — was a
            global sudo setting, the same for every subnet.
          </p>
          <p>
            The obstacle to letting owners move the validator share was root. Root TAO stakers are
            paid from the validator slot of every subnet. Cut that slot from 41% to 20% and, with
            nothing else changing, root&apos;s income from the subnet would halve: owners could tax
            a network-wide stakeholder to pay their own participants. Etao removes that coupling.
            The validator slot becomes a choice about <em>validators</em>, and root&apos;s payout is
            pinned to a baseline the owner cannot move.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>The emission weight vector</h2>
          <p>
            Every subnet now carries a five-slot vector{' '}
            <code>[burn, recycle, owner, validator, miner]</code>. Each slot is a <code>u64</code>{' '}
            share of <code>u64::MAX</code>, so the five slots sum to exactly one with no rounding
            dust. Per block, <code>alpha_out × owner</code> accrues to the owner,{' '}
            <code>alpha_out × validator</code> to the validator bucket and{' '}
            <code>alpha_out × miner</code> to the miner bucket.
          </p>
          <table className={styles.metrics_table}>
            <thead>
              <tr>
                <th>Slot</th>
                <th>Paid to</th>
                <th>Rule in v1</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>
                  <code>burn</code>
                </td>
                <td>nothing — alpha is destroyed</td>
                <td>must be 0 (reserved)</td>
              </tr>
              <tr>
                <td>
                  <code>recycle</code>
                </td>
                <td>nothing — alpha returns to the unissued pool</td>
                <td>must be 0 (reserved)</td>
              </tr>
              <tr>
                <td>
                  <code>owner</code>
                </td>
                <td>the subnet owner&apos;s hotkey</td>
                <td>0% to 33%; lease-created subnets at least 18%</td>
              </tr>
              <tr>
                <td>
                  <code>validator</code>
                </td>
                <td>root&apos;s baseline share first, then alpha validator dividends</td>
                <td>at most 50%, and strictly above root&apos;s live baseline share</td>
              </tr>
              <tr>
                <td>
                  <code>miner</code>
                </td>
                <td>miner incentive</td>
                <td>at least 33%</td>
              </tr>
            </tbody>
          </table>
          <p>
            The setter is{' '}
            <DocLink href='/docs/tx/set-subnet-emission-weights'>
              <code>AdminUtils.sudo_set_subnet_emission_weights(netuid, uids, values)</code>
            </DocLink>
            . It is sparse: name the slots you want to change and their new shares; the others keep
            their value. The merged vector must pass every rule, and each rule has its own error so
            a rejection names the constraint. Callers are the subnet owner coldkey or root. Owners
            may change a subnet&apos;s vector <strong>once every 50,400 blocks</strong> (about seven
            days). The limit is keyed by subnet, not by coldkey, so handing a subnet to a new owner
            does not reset it; a subnet&apos;s first change is free, and root is exempt.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>How root stays whole</h2>
          <p>
            Root TAO is weighted against alpha by the global TAO weight <code>w</code> (
            <code>TaoWeight</code>, 0.18 on mainnet today). Before Etao, the alpha root stakers
            received from a subnet each block was
          </p>
          <pre
            className={styles.code_block}
          >{`root_alpha = V × x / (x + A) × alpha_out        with x = root_tao × w`}</pre>
          <p>
            where <code>V</code> was the fixed 41% validator slot and <code>A</code> the
            subnet&apos;s alpha issuance. Etao keeps exactly that payout and detaches it from the
            owner&apos;s choice of <code>V</code>. Every block, for every subnet, the chain computes
            root&apos;s <strong>baseline share</strong> and pays it{' '}
            <strong>off the top of the validator slot</strong>:
          </p>
          <pre
            className={styles.code_block}
          >{`# root's baseline share of this subnet's emission, recomputed every block
r          = V0 × x / (x + A)          x  = root_tao × TaoWeight (global)
                                       V0 = validator share of the default vector (0.41)

# what root is paid, and what alpha validators share
root_alpha       = min(V, r) × alpha_out
validator_alpha  = (V − min(V, r)) × alpha_out

# a set is rejected with ValidatorEmissionTooLowToPreserveRoot if V_new ≤ r
# (validators would get nothing); the floor at this block is r itself`}</pre>
          <p>
            <code>r</code> is what root would earn from the subnet right now if the owner had never
            touched the vector. Because it is recomputed from live state on every block, root&apos;s
            payout tracks the baseline <em>continuously</em> — as issuance grows against root TAO,
            as root stake moves, or as governance changes <code>TaoWeight</code> — with no further
            call from anyone. A subnet whose owner never changes the vector pays exactly what the
            chain paid before Etao.
          </p>
          <ul className={styles.list}>
            <li>
              <strong>The owner cannot move root&apos;s payout.</strong> Lowering <code>V</code>{' '}
              from 41% to 20% takes the whole 21 points from alpha validators; root&apos;s slice is
              the same before and after. Raising <code>V</code> likewise pays only alpha validators
              more. There is no per-subnet weight to solve, store or ratchet.
            </li>
            <li>
              <strong>
                Everything else is the plain global <code>TaoWeight</code>.
              </strong>{' '}
              Stake weighting inside Yuma Consensus, the TAO-versus-alpha split of validator
              dividends, child/parent stake and the pool side all use the global knob exactly as
              before Etao.
            </li>
          </ul>
          <p>
            On a mainnet subnet with 5.44M TAO on root, 6.46M alpha issued and{' '}
            <code>TaoWeight = 0.18</code>, root&apos;s baseline is about 5.4% of emission, so the
            validator slot can go down to anything strictly above <strong>5.4%</strong>. The floor
            moves with root stake, issuance and the global weight, so{' '}
            <code>btcli subnets emission show</code> prints the live value and every write checks it
            locally before anything is signed.
          </p>
          <p>
            <strong>The capped case.</strong> The check runs at set time while the baseline keeps
            moving. If <code>r</code> later grows past a subnet&apos;s <code>V</code>, root&apos;s
            cut is capped at the whole validator slot and alpha validators receive nothing until the
            owner raises <code>V</code>; root is never paid more than the slot. <code>show</code>{' '}
            flags the capped state and the runtime API reports both the baseline and the share
            actually paid.
          </p>
          <p>
            <strong>What does not change.</strong> The pool side is untouched. <code>tao_in</code>,{' '}
            <code>alpha_in</code>, the alpha-injection cap and <code>RootProp</code> use the global{' '}
            <code>TaoWeight</code>, exactly as before. Changing a subnet&apos;s vector changes how
            its alpha is <em>distributed</em>; it does not change how much TAO or alpha is{' '}
            <em>injected</em> into its pool, prices, swaps, or the emission the subnet receives.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Migration and compatibility</h2>
          <p>
            The v480 upgrade ran one migration, <code>seed_subnet_emission_weights_v1</code>, which
            seeds every live subnet from its legacy owner-cut state so that the first post-upgrade
            block pays out exactly what the last pre-upgrade block did:
          </p>
          <ul className={styles.list}>
            <li>
              <code>owner</code> is the legacy <code>SubnetOwnerCut</code> when{' '}
              <code>OwnerCutEnabled</code> was true, else 0; <code>validator</code> is half the
              remainder; <code>miner</code> is what is left, absorbing the rounding ulp.
            </li>
            <li>
              <code>SubnetCreatedViaLease</code> is set for subnets with a live lease mapping, and{' '}
              <code>SubnetOwnerCutAccrued</code> is seeded to the exact deduction the legacy lock
              refund applied — including subnets already mid-dissolution — so no refund changes.
            </li>
          </ul>
          <p>
            The v481 upgrade runs <code>remove_subnet_tao_weight_v1</code>, which deletes the{' '}
            <code>SubnetTaoWeight</code> entries the v480 set-time solve had written — the per-block
            baseline replaces them — and touches nothing else. Subnets whose legacy owner cut was
            disabled were seeded with a 50% validator slot and paid root{' '}
            <code>0.50 × x / (x + A)</code> under v480; from v481 they pay the global baseline{' '}
            <code>0.41 × x / (x + A)</code> like every other subnet, about 18% less root alpha on
            those subnets and correspondingly more to their alpha validators.
          </p>
          <p>
            New subnets are seeded with the default vector <code>[0, 0, 18%, 41%, 41%]</code>.
            Dissolving a subnet clears its vector, lease marker, accrued owner cut and rate-limit
            key, so a reused netuid starts clean.
          </p>
          <p>
            <strong>Deprecated extrinsics.</strong> <code>sudo_set_subnet_owner_cut</code> and{' '}
            <code>sudo_set_owner_cut_enabled</code> keep their call indices and signatures but
            always return <code>Deprecated</code>. Set the owner slot — including to zero — through{' '}
            <code>sudo_set_subnet_emission_weights</code> instead. <code>SubnetOwnerCut</code> and{' '}
            <code>OwnerCutEnabled</code> remain readable as legacy views but are no longer written.
          </p>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>New chain surface</h2>
          <ul className={styles.list}>
            <li>
              <strong>Storage:</strong> <code>SubnetEmissionWeights</code> (the vector),{' '}
              <code>SubnetCreatedViaLease</code>, <code>SubnetOwnerCutAccrued</code>.
            </li>
            <li>
              <strong>Event:</strong>{' '}
              <code>
                SubtensorModule.SubnetEmissionWeightsSet {'{'} netuid, weights {'}'}
              </code>{' '}
              with the full post-merge vector.
            </li>
            <li>
              <strong>Rate limit:</strong>{' '}
              <code>OwnerHyperparamUpdate(Hyperparameter::EmissionWeights)</code> keyed by subnet in{' '}
              <code>LastRateLimitedBlock</code>; constant{' '}
              <code>SubnetEmissionWeightsRateLimit = 50_400</code>.
            </li>
            <li>
              <strong>Runtime API:</strong>{' '}
              <code>SubnetInfoRuntimeApi.get_subnet_emission_weights(netuid)</code> (API version 3)
              returning the vector, <code>root_baseline_share</code> and the <code>root_share</code>{' '}
              actually paid; <code>get_subnet_hyperparams_v3</code> adds <code>owner_emission</code>
              , <code>validator_emission</code> and <code>miner_emission</code>.
            </li>
            <li>
              <strong>EVM</strong> (subnet precompile, index 2051):{' '}
              <code>setSubnetEmissionWeights(netuid, uids, values)</code> and{' '}
              <code>getSubnetEmissionWeights(netuid)</code> returning the vector and the lease flag.
            </li>
            <li>
              <strong>Python SDK:</strong> the{' '}
              <DocLink href='/docs/query/subnet-emission-weights'>
                <code>subnet_emission_weights</code>
              </DocLink>{' '}
              read (vector, shares, the global TAO weight, root&apos;s baseline share and the share
              paid, what alpha validators keep, the capped flag, the live validator floor,
              rate-limit state), the{' '}
              <DocLink href='/docs/tx/set-subnet-emission-weights'>
                <code>SetSubnetEmissionWeights</code>
              </DocLink>{' '}
              intent with a structured before/after preview, and the pure rules module{' '}
              <code>bittensor.emission_weights</code>.
            </li>
            <li>
              <strong>CLI:</strong> <code>btcli subnets emission</code> with <code>show</code>,{' '}
              <code>set</code>, <code>set-value</code>, <code>increase</code> and{' '}
              <code>decrease</code>; <code>--json</code> and <code>--dry-run</code> everywhere;
              fixed exit codes (0 done, 1 missing subnet or blocked, 2 rule violation).
            </li>
          </ul>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>For subnet owners</h2>
          <p>Look before you touch. The Allowed table is live for this block:</p>
          <pre className={styles.code_block}>{`btcli subnets emission show --netuid 4

Vector
      owner  18.00%
  validator  41.00%
      miner  41.00%

Root
  root share of emission  5.4015%    paid off the top of the validator slot
     root baseline share  5.4015%    default validator share, global TaoWeight (18.00%); recomputed every block
   validators after root  35.5987%   what alpha validators share once root is paid
         validator floor  > 5.4015%  a change must leave the validator share above the baseline
       next owner change  now        first change; owners may change once per 50,400 blocks

Allowed
      owner  0.00% to 33.00%
  validator  5.4015% to 50.0000%    must exceed root's baseline share, which is paid off the top of this slot
      miner  >= 33.00%`}</pre>
          <p>
            Preview with <code>--dry-run</code>; the plan shows the before/after vector, root&apos;s
            share before and after, and what alpha validators keep:
          </p>
          <pre
            className={styles.code_block}
          >{`btcli subnets emission set --netuid 4 --owner 18% --validator 20% --miner 62% --dry-run

dry run: set emission weights on netuid 4: owner=18.00% validator=20.00% miner=62.00%
  effect  vector: owner 18.00% / validator 41.00% / miner 41.00% -> owner 18.00% / validator 20.00% / miner 62.00%
  effect  root keeps its baseline 5.4015% of this subnet's emission (default validator share,
          global TaoWeight) before and after; alpha validators get 35.5987% -> 14.5985%`}</pre>
          <p>
            Then drop <code>--dry-run</code>, or nudge one slot at a time:
          </p>
          <pre
            className={styles.code_block}
          >{`btcli subnets emission increase  --netuid 4 --slot miner --by 5%
btcli subnets emission decrease  --netuid 4 --slot owner --by 0.02
btcli subnets emission set-value --netuid 4 --slot owner --value 0    # owner cut off`}</pre>
          <p>
            A rejected vector exits 2 before anything is signed and names the rule and the allowed
            range:
          </p>
          <pre
            className={styles.code_block}
          >{`error: the chain would reject this change: ValidatorEmissionTooLowToPreserveRoot: validator (slot 3)
       is 5.0000%; root is paid its baseline share of emission off the top of this slot every block,
       so right now the validator share must be above 5.4015% or validators would get nothing
  note: allowed on subnet 4 right now: validator 5.4015% to 50.0000%
  help: btcli subnets emission show --netuid 4 prints the vector and every allowed range`}</pre>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Notes by role</h2>
          <ul className={styles.list}>
            <li>
              <strong>Validators.</strong> Your dividends come from the validator slot after
              root&apos;s baseline share is taken off the top. Where an owner lowers the slot, every
              point removed comes out of what alpha validators and their alpha stakers receive;
              root&apos;s slice does not move. The floor only guarantees the slot is above the
              baseline when it is set; if the baseline later grows past it, alpha validators receive
              nothing until the owner raises the slot (<code>show</code> reports this as capped).
              Watch <code>SubnetEmissionWeightsSet</code> events.
            </li>
            <li>
              <strong>Miners.</strong> The miner slot is at least 33% and can grow to whatever is
              left after the owner slot and the validator floor. Weight setting, incentive and the
              epoch are unchanged; only the size of the bucket moves.
            </li>
            <li>
              <strong>Root TAO stakers.</strong> Your share of every subnet&apos;s emission is its
              baseline — recomputed every block from live root stake, issuance and the global{' '}
              <code>TaoWeight</code> — and paid before alpha validators see anything. Owners cannot
              move it in either direction and it needs no call from anyone to stay current; the only
              cap is the validator slot itself. The global <code>TaoWeight</code> remains the
              governance lever over root&apos;s weight.
            </li>
            <li>
              <strong>Subnet owners.</strong> One change per week per subnet; the first is free of
              the limit. Lease-created subnets cannot drop the owner slot below 18%. Every{' '}
              <code>btcli</code> write validates locally and shows the exact floor, so test with{' '}
              <code>--dry-run</code> first.
            </li>
            <li>
              <strong>Integrators.</strong> Read the vector, the baseline and the share paid through
              the runtime API or the SDK read; there is no per-subnet TAO weight; treat{' '}
              <code>SubnetOwnerCut</code> / <code>OwnerCutEnabled</code> as legacy views; expect{' '}
              <code>SubnetEmissionWeightsSet</code> events and <code>Deprecated</code> from the two
              retired extrinsics.
            </li>
          </ul>
        </section>

        <section className={styles.section}>
          <h2 className={styles.subtitle}>Release details</h2>
          <p>
            Runtime <code>spec_version</code> <strong>481</strong> (the vector shipped in 480),{' '}
            <code>transaction_version</code> unchanged. Delivered to mainnet as a sudo runtime
            upgrade from <code>main</code>. Migrations <code>seed_subnet_emission_weights_v1</code>{' '}
            (480) and <code>remove_subnet_tao_weight_v1</code> (481), both idempotent. Reference
            pages: <DocLink href='/docs/guides/etao'>Etao release guide</DocLink> (the complete
            standalone note),{' '}
            <DocLink href='/docs/concepts/emission-weights'>Emission weights</DocLink> (the concept
            page),{' '}
            <DocLink href='/docs/tx/set-subnet-emission-weights'>
              <code>set-subnet-emission-weights</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/query/subnet-emission-weights'>
              <code>subnet-emission-weights</code>
            </DocLink>
            ,{' '}
            <DocLink href='/docs/hyperparameters/validator-emission'>
              <code>validator_emission</code>
            </DocLink>
            , and the source in{' '}
            <DocLink href='/code/pallets/subtensor/src/subnets/emission_weights.rs'>
              <code>emission_weights.rs</code>
            </DocLink>
            .
          </p>
        </section>
      </FadeInWrapper>
    </Suspense>
  );
};

export default page;
