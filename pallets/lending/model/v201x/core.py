"""Block-by-block reference model of SPEC-v1.md v2.0 for one subnet (the lending pallet + the pool + dereg phases B/D/E/F).

Integer rao everywhere; prices are U64F64 bits (rao TAO per rao alpha << 64). Deterministic: no floats, no randomness,
iteration over positions in sorted account order. Extrinsics are all-or-nothing (all checks are evaluated on local copies
before any state is written). This is the oracle the Rust pallet is meant to match to the rao.

v2.0 vs v1.9: no SMA-pause lapse; price refs are spot / TWAP_median / SMA30 only (EMA is not a lending
ref; T_ema of pool TAO and SubnetMovingPrice for I25 ring seed stay); shared param set; v1.9.1 one-shot
pro-rata long sale inside the prune; SMA30 rings prefilled (I25).

Block order (mirrors the runtime): begin_block (SubnetMovingPrice EMA, T_ema, TWAP/SMA sampling, clear per-block counters)
 -> environment injections (coinbase) -> extrinsics -> end_block (on_idle: interest Due, health, short up-step,
 long down-step, dissolution phases).
"""
from dataclasses import dataclass, field
from typing import Optional
from .params import (ALPHA_SUPPLY_CAP, RAO, BPD, BPY, Q64, BPS, MAX_LONG_POSITIONS,
                     SMA_PAUSE_LAPSE_BLOCKS, SynthParams, LongParams, ModelSwitches,
                     DEFAULT_SYNTH, DEFAULT_LONG, DEFAULT_SW)
from . import pool as pm
from .pool import cdiv


class LendingError(Exception):
    pass


@dataclass
class Short:
    owner: str
    q: int
    C: int
    P: int
    deposit: int
    zone_q: int
    zone_p: int
    opened_at: int
    last_accrued: int
    due: int = 0
    rem: int = 0
    sma_paused: int = 0
    state: str = 'open'          # open | delev
    delev_since: Optional[int] = None
    interest_paid: int = 0
    all_p: int = 0               # v201x: proceeds of all still-owed mints (scaled on burn), for dereg_pay_proceeds


@dataclass
class Long:
    owner: str
    D: int
    A: int
    C: int
    deposit: int
    opened_at: int
    last_accrued: int
    due: int = 0
    rem: int = 0
    sma_paused: int = 0
    state: str = 'open'          # open | delev | unwinding
    delev_since: Optional[int] = None
    interest_paid: int = 0


class Subnet:
    def __init__(self, T, X, out_alpha, ema_fp=None, sp: SynthParams = DEFAULT_SYNTH, lp: LongParams = DEFAULT_LONG,
                 sw: ModelSwitches = DEFAULT_SW, block=0, sma_seed_fp=None, holders=None, trace=False):
        self.sp, self.lp, self.sw = sp, lp, sw
        self.T, self.X = int(T), int(X)
        self.block = block
        spot = self.spot()
        self.ema = int(ema_fp) if ema_fp else spot
        self.twap = [spot] * sp.twap_samples
        self.twap_cur = 0
        seed = int(sma_seed_fp) if sma_seed_fp else self.ema
        self.sma_days = [seed] * sp.rank_days
        self.sma_cur = 0
        self.hour_sum, self.hour_n = 0, 0
        self.rank_filled = 0
        self.t_ema = self.T
        self.stakes = dict(holders or {})
        rest = int(out_alpha) - sum(self.stakes.values())
        if rest > 0:
            self.stakes['others'] = self.stakes.get('others', 0) + rest
        self.issuance = self.X + sum(self.stakes.values())
        self.ex_expected = self.issuance
        self.balances = {}
        self.custody = 0
        self.burned = 0
        self.recycled = 0
        self.emitted_tao = 0
        self.tao_in_flow = 0          # SubnetTaoFlow / volume proxies (user swaps only)
        self.volume = 0
        self.enabled = True
        self.longs_enabled = True
        self.shorts: dict[str, Short] = {}
        self.longs: dict[str, Long] = {}
        self.synth = 0
        self.residual = 0
        self.breaker = False
        self.long_lifted = 0
        self.long_locked = 0
        self.long_shortfall = 0
        self.interest_to_pool = 0
        self.net_minted_blk = 0
        self.net_lifted_blk = 0
        self.lowest_other_sma = None  # set by env: lowest non-immune SMA30 of the other subnets
        # dissolution
        self.phase = None
        self.flag_block = None
        self.t_flag = None
        self.t_a = None
        self.r_fp = None
        self.pot = 0
        self.payouts = 0
        self.settled = set()
        self.phase_log = []
        self.dissolved = False
        # per-block meters for invariants
        self.blk_up_start = None
        self.blk_up_end = None
        self.blk_dn_start = None
        self.blk_dn_end = None
        self.blk_interest = 0
        self.blk_T_interest_delta = 0
        self.events = [] if trace else None
        self.initial_tao = None
        self.stats = dict(defaults_short=0, defaults_long=0, residual_events=0, sma_lapses=0)

    # ------------------------------------------------------------------ helpers
    def spot(self):
        return pm.price(self.T, self.X)

    def twap_median(self):
        s = sorted(self.twap)
        return s[(len(s) - 1) // 2]          # lower median of 72 samples (G4)

    def sma30(self):
        # I25: default is the mean of a full prefilled ring. EMA / partial-mean fallbacks
        # exist only for finding4 ablations (rank_cold_start / rank_partial).
        if self.sw.rank_cold_start and self.rank_filled < self.sp.rank_days:
            return self.ema
        if self.sw.rank_partial and self.rank_filled < self.sp.rank_days:
            if self.rank_filled == 0:
                return self.ema
            n = self.rank_filled
            idx = [(self.sma_cur - 1 - i) % self.sp.rank_days for i in range(n)]
            return sum(self.sma_days[i] for i in idx) // n
        return sum(self.sma_days) // len(self.sma_days)

    def ev(self, *a):
        if self.events is not None:
            self.events.append((self.block,) + a)

    def bal(self, who):
        return self.balances.get(who, 0)

    def credit(self, who, amt):
        assert amt >= 0
        self.balances[who] = self.bal(who) + amt

    def debit(self, who, amt):
        if self.bal(who) < amt:
            raise LendingError('InsufficientBalance')
        self.balances[who] -= amt

    def issuance_ex(self):
        return self.issuance - self.synth - self.residual

    def in_zone(self):
        if self.lowest_other_sma is None:
            return False
        return self.sma30() * BPS <= self.sp.zone_bps * self.lowest_other_sma

    def short_cov_ref(self, spot=None, twap=None, sma=None):
        """P2 v2.0: max(spot, TWAP_median, SMA30). Ablation: max(EMA, spot, SMA30)."""
        spot = self.spot() if spot is None else spot
        twap = self.twap_median() if twap is None else twap
        sma = self.sma30() if sma is None else sma
        if self.sw.legacy_ema_price_ref:
            return max(self.ema, spot, sma)
        if self.sw.cov_ref == 'spot_sma':
            return max(spot, sma)
        return max(spot, twap, sma)

    def long_cov_ref(self, spot=None, twap=None, sma=None):
        """L2 v2.0: min(spot, TWAP_median). Optional SMA via switches."""
        spot = self.spot() if spot is None else spot
        twap = self.twap_median() if twap is None else twap
        ref = min(self.ema, spot, twap) if self.sw.legacy_ema_price_ref else min(spot, twap)
        if sma is not None:
            ref = min(ref, sma)
        return ref

    def short_notional(self, q):
        px = self.ema if self.sw.legacy_ema_price_ref else self.twap_median()
        return q * px >> 64

    def day_interest_short(self, q):
        return self.short_notional(q) * self.sp.apr_bps // (BPS * 365)

    def day_interest_long(self, D):
        return D * self.sp.apr_bps // (BPS * 365)

    def dissolving(self):
        return self.phase is not None and self.phase != 'A'

    # ------------------------------------------------------------------ block lifecycle
    def begin_block(self):
        self.block += 1
        sp = self.sp
        spot = self.spot()
        # EMA (SubnetMovingPrice, alpha 0.0003/block), truncated toward the old value
        if spot >= self.ema:
            self.ema += (spot - self.ema) * 3 // 10_000
        else:
            self.ema -= (self.ema - spot) * 3 // 10_000
        # T_ema: 7,200-block EMA of pool TAO (r denominator guard), alpha 1/7200
        if self.T >= self.t_ema:
            self.t_ema += (self.T - self.t_ema) // BPD
        else:
            self.t_ema -= (self.t_ema - self.T) // BPD
        if self.block % sp.twap_interval_blocks == 0:
            self.twap[self.twap_cur] = spot
            self.twap_cur = (self.twap_cur + 1) % sp.twap_samples
            self.hour_sum += spot
            self.hour_n += 1
        if self.block % BPD == 0 and self.hour_n:
            self.sma_days[self.sma_cur] = self.hour_sum // self.hour_n
            self.sma_cur = (self.sma_cur + 1) % sp.rank_days
            self.rank_filled += 1
            if self.sw.rank_cold_start and self.rank_filled == 1:
                self.sma_days = [self.sma_days[self.sma_cur - 1]] * sp.rank_days   # ring seeded by first full day only
            self.hour_sum, self.hour_n = 0, 0
        self.net_minted_blk = 0
        self.net_lifted_blk = 0
        self.blk_up_start = self.blk_up_end = self.blk_dn_start = self.blk_dn_end = None
        self.blk_interest = 0
        self.blk_T_interest_delta = 0

    # swap fees: 100 % to the block author (subtensor swap_step.rs), not kept in the pool
    def _fee_tao(self, tin):
        f = pm.fee_of(tin)
        self.credit('author', f)
        return f

    def _fee_alpha(self, ain):
        f = pm.fee_of(ain)
        self.stakes['author'] = self.stakes.get('author', 0) + f
        return f

    # environment (coinbase / market) -------------------------------------------------
    def inject(self, tao_in, alpha_in):
        self.T += tao_in
        self.X += alpha_in
        self.issuance += alpha_in
        self.ex_expected += alpha_in
        self.emitted_tao += tao_in

    def chain_buy(self, tao):
        if tao <= 0:
            return 0
        a = pm.out_alpha_for_tao(self.T, self.X, tao)
        self.T += tao - self._fee_tao(tao)
        self.X -= a
        self.emitted_tao += tao
        self.stakes['protocol'] = self.stakes.get('protocol', 0) + a
        return a

    def emit_out(self, who, alpha):
        self.stakes[who] = self.stakes.get(who, 0) + alpha
        self.issuance += alpha
        self.ex_expected += alpha

    def user_buy(self, who, tao):
        """Ordinary stake: TAO in, alpha out. Counts as flow/volume."""
        if self.dissolving() or tao <= 0:
            return 0
        tao = min(tao, self.bal(who))
        a = pm.out_alpha_for_tao(self.T, self.X, tao)
        if a <= 0:
            return 0
        self.debit(who, tao)
        self.T += tao - self._fee_tao(tao)
        self.X -= a
        self.stakes[who] = self.stakes.get(who, 0) + a
        self.tao_in_flow += tao
        self.volume += tao
        return a

    def user_sell(self, who, alpha):
        if self.dissolving() or alpha <= 0:
            return 0
        alpha = min(alpha, self.stakes.get(who, 0))
        o = pm.out_tao_for_alpha(self.T, self.X, alpha)
        self.stakes[who] -= alpha
        self.X += alpha - self._fee_alpha(alpha)
        self.T -= o
        self.credit(who, o)
        self.tao_in_flow -= o
        self.volume += o
        return o

    def user_buy_to_price(self, who, p_fp):
        if self.spot() >= p_fp:
            return 0
        Tp, _ = pm.pumped(self.T, self.X, p_fp)
        need = cdiv((Tp - self.T) * pm.FEE_DEN, pm.FEE_DEN - pm.FEE_NUM)
        return self.user_buy(who, need)

    def user_sell_to_price(self, who, p_fp):
        if self.spot() <= p_fp:
            return 0
        _, Xd = pm.dumped(self.T, self.X, p_fp)
        need = cdiv((Xd - self.X) * pm.FEE_DEN, pm.FEE_DEN - pm.FEE_NUM)
        return self.user_sell(who, need)

    # ------------------------------------------------------------------ interest
    def _accrue(self, pos, notional, apr_bps):
        n = self.block - pos.last_accrued
        if n <= 0:
            return
        num = notional * apr_bps * n + pos.rem
        den = BPS * BPY
        pos.due += num // den
        pos.rem = num % den
        pos.last_accrued = self.block

    def _pending(self, pos, side):
        """Interest a touch would charge now (min(due + accrued, C)); no state change."""
        n = self.block - pos.last_accrued
        notional = self.short_notional(pos.q) if side == 'S' else pos.D
        apr = self.sp.apr_bps
        due = pos.due + (notional * apr * n + pos.rem) // (BPS * BPY) if n > 0 else pos.due
        return min(due, pos.C) if self.sw.charge_on_touch else 0

    def _pending_raw(self, pos):
        n = self.block - pos.last_accrued
        due = pos.due + (self.short_notional(pos.q) * self.sp.apr_bps * n + pos.rem) // (BPS * BPY) if n > 0 else pos.due
        return due

    def _charge(self, pos, side):
        """Move min(due, C) from custody to SubnetTAO. Not flow, not volume (I19)."""
        if side == 'S':
            self._accrue(pos, self.short_notional(pos.q), self.sp.apr_bps)
        else:
            self._accrue(pos, pos.D, self.sp.apr_bps)
        c = min(pos.due, pos.C)
        if c:
            pos.C -= c
            pos.due -= c
            pos.interest_paid += c
            self.custody -= c
            self.T += c
            self.interest_to_pool += c
            self.blk_interest += c
            self.blk_T_interest_delta += c
            self.ev('interest', side, pos.owner, c)
        return c

    # ------------------------------------------------------------------ coverage
    def short_need(self, q_pos, Q, T, X, spot, twap, sma, marginal_q=0):
        """P2: pro-rata share of buying the whole book back from the pool pumped to
        Stress_eff x max(spot, TWAP_median, SMA30) (v2.0; EMA ablation via switch).
        Returns None when the buy-back is infinite (book >= pumped pool alpha)."""
        sp = self.sp
        if Q <= 0:
            return 0
        Xs = X
        if self.sw.stress_x_ema and hasattr(self, 'x_ema'):
            Xs = self.x_ema
        s_bps = sp.stress_bps * (BPS * Xs + sp.stress_slope_bps * Q) // (BPS * Xs)
        if self.sw.p21_surcharge and self.residual:
            ex = max(self.issuance_ex(), 1)
            s_bps = s_bps * (ex + 10 * self.residual) // ex
        ref = self.short_cov_ref(spot, twap, sma)
        tgt = ref * s_bps // BPS
        Tp, Xp = pm.pumped(T, X, tgt)
        cost = pm.tao_in_for_alpha(Tp, Xp, Q)
        if cost is None:
            return None
        pr = cdiv(cost * q_pos, Q)
        if marginal_q and self.sw.open_marginal:
            base = pm.tao_in_for_alpha(Tp, Xp, Q - marginal_q) if Q > marginal_q else 0
            return max(pr, (q_pos - marginal_q) * cost // Q + cost - base) if base is not None else None
        return pr

    def long_need(self, D_pos, A_pos, Abook, T, X, spot, twap, sma=None):
        lp = self.lp
        if Abook <= 0:
            return D_pos
        ref = self.long_cov_ref(spot, twap, sma)
        tgt = ref * BPS // lp.stress_bps
        Td, Xd = pm.dumped(T, X, tgt)
        proceeds = pm.out_tao_for_alpha(Td, Xd, Abook)
        return max(0, D_pos - proceeds * A_pos // Abook)

    def _long_sma_for_cov(self, purpose):
        sw = self.sw
        if sw.long_ref_includes_sma or (sw.long_withdraw_ref_sma and purpose in ('withdraw', 'increase')):
            return self.sma30()
        return None

    # ------------------------------------------------------------------ open checks shared
    def _breaker_blocks(self):
        ex = max(self.issuance_ex(), 1)
        if self.breaker:
            if self.residual * BPS < self.sp.breaker_off_bps * ex:
                self.breaker = False
        else:
            if self.residual * BPS > self.sp.breaker_on_bps * ex:
                self.breaker = True
        return self.breaker

    # ================================================================== SHORTS
    def open_short(self, who, q, C, min_tao_out=0):
        return self._short_mint(who, q, C, min_tao_out, new=True)

    def increase_short(self, who, q, C, min_tao_out=0):
        return self._short_mint(who, q, C, min_tao_out, new=False)

    def _short_mint(self, who, q, C, min_tao_out, new):
        sp = self.sp
        if not self.enabled:
            raise LendingError('Disabled')
        if self.dissolving() or self.dissolved:
            raise LendingError('DissolutionInProgress')
        if new:
            if who in self.shorts:
                raise LendingError('PositionExists')
            if who in self.longs:
                raise LendingError('OppositeSideOpen')
        else:
            if who not in self.shorts:
                raise LendingError('NoPosition')
            if self.shorts[who].state != 'open':
                raise LendingError('PositionDeleveraging')
        if q <= 0:
            raise LendingError('InvalidAmount')
        if self.T < sp.min_pool_tao:
            raise LendingError('PoolTooShallow')
        if self.sw.p21_breaker and self._breaker_blocks() and not self.sw.p21_surcharge:
            raise LendingError('DilutionBreakerActive')
        if self.sw.supply_cap and self.issuance + q > ALPHA_SUPPLY_CAP:
            raise LendingError('AlphaSupplyCap')
        if self.net_minted_blk + q > self.X * sp.open_impact_bps // BPS:
            raise LendingError('OpenImpactTooHigh')
        pos = self.shorts.get(who)
        q_tot = q + (pos.q if pos else 0)
        if self.short_notional(q_tot) < sp.min_notional:
            raise LendingError('BelowMinNotional')
        dep = sp.deposit if new else 0
        if self.bal(who) < C + dep:
            raise LendingError('InsufficientBalance')
        P = pm.out_tao_for_alpha(self.T, self.X, q)
        if P < min_tao_out:
            raise LendingError('SlippageExceeded')
        T2, X2 = self.T - P, self.X + q - pm.fee_of(q)
        if T2 < sp.min_pool_tao:
            raise LendingError('PoolTooShallow')
        spot2 = pm.price(T2, X2)
        Q2 = self.synth + q
        C_tot = C + (pos.C - self._pending(pos, 'S') if pos else 0)
        P_tot = P + (pos.P if pos else 0)
        pend = self._pending(pos, 'S') if pos else 0
        need = self.short_need(q_tot, Q2, T2 + pend, X2, pm.price(T2 + pend, X2), self.twap_median(), self.sma30(), marginal_q=q)
        if need is None or C_tot + P_tot < need:
            raise LendingError('CoverageNotMet')
        # ---- commit
        zone = self.in_zone()
        self.debit(who, C + dep)
        self.custody += C + dep + P
        self._fee_alpha(q)
        self.T, self.X = T2, X2
        self.issuance += q
        self.synth += q
        self.net_minted_blk += q
        if pos is None:
            pos = Short(who, 0, 0, 0, dep, 0, 0, self.block, self.block)
            self.shorts[who] = pos
        elif self.sw.charge_on_touch:
            self._charge(pos, 'S')
        pos.q += q
        pos.C += C
        pos.P += P
        if zone:
            pos.zone_q += q
            pos.zone_p += P
        pos.all_p += P
        self.ev('short_open' if new else 'short_increase', who, q, P, C, zone)
        self._post_cov_short(pos)
        return P

    # quotes used by agents (same formulas as the extrinsics) -----------------
    def quote_short_C(self, who, q):
        """Minimum extra collateral for open/increase of q now (None if impossible)."""
        pos = self.shorts.get(who)
        P = pm.out_tao_for_alpha(self.T, self.X, q)
        T2, X2 = self.T - P, self.X + q - pm.fee_of(q)
        q_tot = q + (pos.q if pos else 0)
        pend = self._pending(pos, 'S') if pos else 0
        need = self.short_need(q_tot, self.synth + q, T2 + pend, X2, pm.price(T2 + pend, X2), self.twap_median(), self.sma30(), marginal_q=q)
        if need is None:
            return None
        have = P + ((pos.C - self._pending(pos, 'S') + pos.P) if pos else 0)
        return max(0, need - have)

    def quote_long_C(self, who, L):
        pos = self.longs.get(who)
        V = self.sw.long_virtual
        A = pm.out_alpha_for_tao(self.T if (self.sw.long_open_curve or V) else self.T - L, self.X, L)
        T2, X2 = (self.T + L if V else self.T) - pm.fee_of(L), self.X - A
        D_tot = L + (pos.D if pos else 0)
        A_tot = A + (pos.A if pos else 0)
        need = self.long_need(D_tot, A_tot, self.long_locked + A, T2, X2, pm.price(T2, X2),
                              self.twap_median(), self._long_sma_for_cov('open' if pos is None else 'increase'))
        have = (pos.C - self._pending(pos, 'L')) if pos else 0
        return max(0, need - have)

    def _post_cov_short(self, pos):
        need = self.short_need(pos.q, self.synth, self.T, self.X, self.spot(), self.twap_median(), self.sma30())
        assert need is not None and pos.C + pos.P >= need, 'I4 coverage after short op'

    def add_collateral(self, who, amt):
        pos = self.shorts.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.dissolving():
            raise LendingError('DissolutionInProgress')
        self.debit(who, amt)
        pos.C += amt
        self.custody += amt
        self.ev('short_add', who, amt)

    def withdraw_collateral(self, who, amt):
        pos = self.shorts.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.dissolving():
            raise LendingError('DissolutionInProgress')
        if pos.state != 'open':
            raise LendingError('PositionDeleveraging')
        pend = self._pending(pos, 'S')
        if amt > pos.C - pend:
            raise LendingError('WithdrawBreaksCoverage')
        need = self.short_need(pos.q, self.synth, self.T, self.X, self.spot(), self.twap_median(), self.sma30())
        buf = self.day_interest_short(pos.q) * self.sp.withdraw_buffer_days
        if need is None or pos.C - pend - amt + pos.P < need + buf:
            raise LendingError('WithdrawBufferBreached')
        if self.sw.charge_on_touch:
            self._charge(pos, 'S')
        pos.C -= amt
        self.custody -= amt
        self.credit(who, amt)
        self.ev('short_withdraw', who, amt)

    def close_short(self, who, alpha, max_tao_in=None):
        pos = self.shorts.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.dissolving():
            raise LendingError('DissolutionInProgress')
        alpha = min(alpha, pos.q)
        pend = self._pending(pos, 'S')
        cost = pm.tao_in_for_alpha(self.T, self.X, alpha)
        if cost is None:
            raise LendingError('SlippageExceeded')
        if max_tao_in is not None and cost > max_tao_in:
            raise LendingError('SlippageExceeded')
        if cost > pos.P + pos.C - pend:
            raise LendingError('InsufficientFunds')
        full = alpha == pos.q
        fromP = min(cost, pos.P)
        fromC = cost - fromP
        T2, X2 = self.T + cost - pm.fee_of(cost), self.X - alpha
        if not full:
            q2 = pos.q - alpha
            need = self.short_need(q2, self.synth - alpha, T2, X2, pm.price(T2, X2), self.twap_median(), self.sma30())
            if need is None or (pos.C - pend - fromC) + (pos.P - fromP) < need:
                raise LendingError('CoverageNotMet')
        if pend and pos.C - pend < fromC:
            raise LendingError('InsufficientFunds')
        # commit
        self._fee_tao(cost)
        self.T, self.X = T2, X2
        if self.sw.charge_on_touch:
            self._charge(pos, 'S')
        pos.P -= fromP
        pos.C -= fromC
        self.custody -= cost
        self._burn_short(pos, alpha)
        self.ev('short_close', who, alpha, cost)
        if full:
            self._refund_short(pos)
        return cost

    def _burn_short(self, pos, alpha):
        if pos.q:
            zq = pos.zone_q * (pos.q - alpha) // pos.q
            zp = pos.zone_p * (pos.q - alpha) // pos.q
            pos.zone_q, pos.zone_p = zq, zp
            pos.all_p = pos.all_p * (pos.q - alpha) // pos.q
        pos.q -= alpha
        self.synth -= alpha
        self.issuance -= alpha
        self.net_minted_blk -= alpha

    def _refund_short(self, pos):
        # any interest still due is taken from the refund (C, then P) -- G2
        d = min(pos.due, pos.C + pos.P)
        if d:
            fc = min(d, pos.C)
            pos.C -= fc
            pos.P -= d - fc
            self.custody -= d
            self.T += d
            self.interest_to_pool += d
            self.blk_interest += d
            self.blk_T_interest_delta += d
            pos.interest_paid += d
            pos.due -= d
        amt = pos.C + pos.P + pos.deposit
        self.custody -= amt
        self.credit(pos.owner, amt)
        del self.shorts[pos.owner]
        self.ev('short_refund', pos.owner, amt)

    def _default_short(self, pos):
        self.residual += pos.q
        self.synth -= pos.q
        self.stats['defaults_short'] += 1
        self.stats['residual_events'] += 1
        self.custody -= pos.C + pos.P + pos.deposit
        self.burned += pos.deposit
        # any C/P dust left is (by construction) zero; if not, it goes to the pool like interest
        dust = pos.C + pos.P
        self.T += dust
        self.ev('short_default', pos.owner, pos.q)
        del self.shorts[pos.owner]

    def poke(self, who):
        pos = self.shorts.get(who)
        if pos and pos.state == 'open':
            self._health_short(pos)

    # health -----------------------------------------------------------------
    def short_health_cost(self, pos):
        """v1.9 H_pos before TWAP/spot scaling: ceil share of the fee-free BuyCost(SyntheticOutstanding | live pool)."""
        if self.sw.health_book and self.synth > 0:
            cb = pm.buy_cost_nofee(self.T, self.X, self.synth)
            return None if cb is None else cdiv(cb * pos.q, self.synth)
        return pm.buy_cost_nofee(self.T, self.X, pos.q)

    def long_health_value(self, pos):
        """Floor share of the fee-free SellProceeds(LongLockedAlpha | live pool), before TWAP/spot scaling."""
        if self.sw.health_book and self.long_locked > 0:
            return pm.sell_proceeds_nofee(self.T, self.X, self.long_locked) * pos.A // self.long_locked
        return pm.sell_proceeds_nofee(self.T, self.X, pos.A)

    def short_equity_twap(self, pos):
        """C + P - H_pos with H_pos = share x TWAP/spot, rounded up (favours the pool)."""
        cost = self.short_health_cost(pos)
        if cost is None:
            return None
        return pos.C + pos.P - cdiv(cost * self.twap_median(), max(self.spot(), 1))

    def _health_short(self, pos):
        eq = self.short_equity_twap(pos)
        cover = self.day_interest_short(pos.q)
        bad = eq is None or eq < 0 or pos.C < cover
        if bad and pos.state == 'open':
            pos.state = 'delev'
            pos.delev_since = self.block
            pos.sma_paused = 0
            self.ev('short_delev_start', pos.owner, eq)
        elif not bad and pos.state == 'delev' and self.sw.delev_exit_on_health:
            # v2.0.1 exit: C + P - H >= h*H and C >= (1+h)*cover
            h = self.sp.exit_hysteresis_bps
            H = pos.C + pos.P - eq
            if eq * 10_000 >= h * H and pos.C * 10_000 >= (10_000 + h) * cover:
                pos.state = 'open'
                self.ev('short_delev_end', pos.owner, eq)
        return not bad

    # ================================================================== LONGS
    def open_long(self, who, L, C, min_alpha_out=0):
        return self._long_lift(who, L, C, min_alpha_out, new=True)

    def increase_long(self, who, L, C, min_alpha_out=0):
        return self._long_lift(who, L, C, min_alpha_out, new=False)

    def _long_lift(self, who, L, C, min_alpha_out, new):
        sp, lp = self.sp, self.lp
        if not self.longs_enabled:
            raise LendingError('LongsDisabled')
        if self.phase is not None or self.dissolved:
            raise LendingError('UnwindInProgress')
        if new:
            if who in self.longs:
                raise LendingError('PositionExists')
            if who in self.shorts:
                raise LendingError('OppositeSideOpen')
            if len(self.longs) >= MAX_LONG_POSITIONS:
                raise LendingError('TooManyLongPositions')
        else:
            if who not in self.longs:
                raise LendingError('NoPosition')
            if self.longs[who].state != 'open':
                raise LendingError('PositionDeleveraging')
        if L <= 0:
            raise LendingError('InvalidAmount')
        if self.T < sp.min_pool_tao:
            raise LendingError('PoolTooShallow')
        if (self.long_lifted + L) * BPS > lp.lift_cap_bps * self.T:
            raise LendingError('LiftCapExceeded')
        if (self.net_lifted_blk + L) * BPS > lp.open_limit_bps * self.T:
            raise LendingError('LongOpenLimit')
        pos = self.longs.get(who)
        D_tot = L + (pos.D if pos else 0)
        if D_tot < sp.min_notional:
            raise LendingError('BelowMinNotional')
        dep = sp.deposit if new else 0
        if self.bal(who) < C + dep:
            raise LendingError('InsufficientBalance')
        # lift then buy in the same pool: SubnetTAO -= L, swap L in
        V = self.sw.long_virtual
        T1 = self.T - L
        A = pm.out_alpha_for_tao(self.T if (self.sw.long_open_curve or V) else T1, self.X, L)
        if A < min_alpha_out or A <= 0:
            raise LendingError('SlippageExceeded')
        T2, X2 = T1 + L - pm.fee_of(L), self.X - A   # I13 restated: dT == -fee (fee to author)
        if V:
            T2 = self.T + L - pm.fee_of(L)            # unbacked L credited, real TAO -fee
        if T2 < sp.min_pool_tao:
            raise LendingError('PoolTooShallow')
        A_tot = A + (pos.A if pos else 0)
        C_tot = C + (pos.C - self._pending(pos, 'L') if pos else 0)
        need = self.long_need(D_tot, A_tot, self.long_locked + A, T2, X2, pm.price(T2, X2),
                              self.twap_median(), self._long_sma_for_cov('open' if new else 'increase'))
        if C_tot < need:
            raise LendingError('LongCoverageNotMet')
        # commit
        self.debit(who, C + dep)
        self.custody += C + dep
        self._fee_tao(L)
        self.T, self.X = T2, X2
        self.long_lifted += L
        self.long_locked += A
        self.net_lifted_blk += L
        if pos is None:
            pos = Long(who, 0, 0, 0, dep, self.block, self.block)
            self.longs[who] = pos
        elif self.sw.charge_on_touch:
            self._charge(pos, 'L')
        pos.D += L
        pos.A += A
        pos.C += C
        self.ev('long_open' if new else 'long_increase', who, L, A, C)
        return A

    def add_long_collateral(self, who, amt):
        pos = self.longs.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.dissolving():
            raise LendingError('DissolutionInProgress')
        self.debit(who, amt)
        pos.C += amt
        self.custody += amt

    def withdraw_long_collateral(self, who, amt):
        pos = self.longs.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.phase is not None:
            raise LendingError('UnwindInProgress')
        if pos.state != 'open':
            raise LendingError('PositionDeleveraging')
        pend = self._pending(pos, 'L')
        if amt > pos.C - pend:
            raise LendingError('LongWithdrawBreaksCoverage')
        need = self.long_need(pos.D, pos.A, self.long_locked, self.T, self.X, self.spot(),
                              self.twap_median(), self._long_sma_for_cov('withdraw'))
        buf = self.day_interest_long(pos.D) * self.sp.withdraw_buffer_days
        if pos.C - pend - amt < need + buf:
            raise LendingError('LongWithdrawBreaksCoverage')
        if self.sw.charge_on_touch:
            self._charge(pos, 'L')
        pos.C -= amt
        self.custody -= amt
        self.credit(who, amt)
        self.ev('long_withdraw', who, amt)

    def max_long_withdraw(self, who):
        pos = self.longs[who]
        need = self.long_need(pos.D, pos.A, self.long_locked, self.T, self.X, self.spot(),
                              self.twap_median(), self._long_sma_for_cov('withdraw'))
        buf = self.day_interest_long(pos.D) * self.sp.withdraw_buffer_days
        return max(0, pos.C - pos.due - need - buf)

    def close_long(self, who, alpha, min_tao_out=0):
        pos = self.longs.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.dissolving():
            raise LendingError('DissolutionInProgress')
        if pos.state == 'unwinding':
            raise LendingError('UnwindInProgress')
        alpha = min(alpha, pos.A)
        pend = self._pending(pos, 'L')
        S = pm.out_tao_for_alpha(self.T, self.X, alpha)
        if S < min_tao_out:
            raise LendingError('SlippageExceeded')
        full = alpha == pos.A
        rep = min(S, pos.D)
        surplus = S - rep
        V = self.sw.long_virtual
        T2, X2 = self.T - S + (0 if V else rep), self.X + alpha - pm.fee_of(alpha)
        if full:
            if S + pos.C - pend < pos.D:
                raise LendingError('CloseInsolvent')
            fromC = pos.D - rep
            if not V:
                T2 += fromC
        else:
            fromC = 0
            need = self.long_need(pos.D - rep, pos.A - alpha, self.long_locked - alpha, T2, X2, pm.price(T2, X2),
                                  self.twap_median(), self._long_sma_for_cov('close'))
            if pos.C - pend < need:
                raise LendingError('LongCoverageNotMet')
        # commit
        self._fee_alpha(alpha)
        self.T, self.X = T2, X2
        if self.sw.charge_on_touch:
            self._charge(pos, 'L')
        pos.A -= alpha
        self.long_locked -= alpha
        pos.D -= rep + fromC
        self.long_lifted -= rep + fromC
        if not self.sw.l8_gross:
            self.net_lifted_blk -= rep + fromC
        pos.C -= fromC
        self.custody -= fromC
        self.credit(who, surplus)
        self.ev('long_close', who, alpha, S, rep + fromC, surplus)
        if full:
            self._refund_long(pos)
        return S

    def _refund_long(self, pos):
        d = min(pos.due, pos.C)
        if d:
            pos.C -= d
            pos.due -= d
            self.custody -= d
            self.T += d
            self.interest_to_pool += d
            self.blk_interest += d
            self.blk_T_interest_delta += d
            pos.interest_paid += d
        amt = pos.C + pos.deposit
        self.custody -= amt
        self.credit(pos.owner, amt)
        del self.longs[pos.owner]
        self.ev('long_refund', pos.owner, amt)

    def _default_long(self, pos):
        assert pos.A == 0
        self.long_lifted -= pos.D
        self.long_shortfall += pos.D
        self.stats['defaults_long'] += 1
        self.custody -= pos.C + pos.deposit
        self.burned += pos.deposit
        self.T += pos.C
        if self.sw.long_virtual:
            self.T -= pos.D                       # write the unbacked claim off SubnetTAO
        self.ev('long_default', pos.owner, pos.D)
        del self.longs[pos.owner]

    def poke_long(self, who):
        pos = self.longs.get(who)
        if pos and pos.state == 'open':
            self._health_long(pos)

    def long_equity_twap(self, pos):
        """C + V_pos - D with V_pos = share x TWAP/spot, rounded down."""
        return pos.C + self.long_health_value(pos) * self.twap_median() // max(self.spot(), 1) - pos.D

    def _health_long(self, pos):
        eq = self.long_equity_twap(pos)
        cover = self.day_interest_long(pos.D)
        bad = eq < 0 or pos.C < cover
        if bad and pos.state == 'open':
            pos.state = 'delev'
            pos.delev_since = self.block
            pos.sma_paused = 0
            self.ev('long_delev_start', pos.owner, eq)
        elif not bad and pos.state == 'delev' and self.sw.delev_exit_on_health:
            # v2.0.1 exit: C + V - D >= h*D and C >= (1+h)*cover
            h = self.sp.exit_hysteresis_bps
            if eq * 10_000 >= h * pos.D and pos.C * 10_000 >= (10_000 + h) * cover:
                pos.state = 'open'
                self.ev('long_delev_end', pos.owner, eq)
        return not bad

    # ================================================================== on_idle
    def end_block(self):
        sp = self.sp
        if self.dissolved:
            return
        if self.phase in (None, 'A'):
            for who in sorted(self.shorts):
                pos = self.shorts[who]
                if (self.block - pos.opened_at) % sp.charge_interval == 0:
                    self._charge(pos, 'S')
            for who in sorted(self.longs):
                pos = self.longs[who]
                if (self.block - pos.opened_at) % sp.charge_interval == 0:
                    self._charge(pos, 'L')
            for who in sorted(self.shorts):
                pos = self.shorts[who]
                if pos.state == 'open':
                    self._health_short(pos)
                elif self.sw.delev_exit_on_health:
                    self._health_short(pos)
            if self.phase is None:
                for who in sorted(self.longs):
                    pos = self.longs[who]
                    if pos.state == 'open' or self.sw.delev_exit_on_health:
                        self._health_long(pos)
            if self.sw.c_exhaust_default:
                for who in sorted(self.shorts):
                    pos = self.shorts[who]
                    if pos.state == 'delev' and pos.C == 0 and self._pending_raw(pos) > 0:
                        self.stats['c_exhaust_defaults'] = self.stats.get('c_exhaust_defaults', 0) + 1
                        self.stats['c_exhaust_P_to_pool'] = self.stats.get('c_exhaust_P_to_pool', 0) + pos.P
                        self._default_short(pos)
            self._short_up_step()
            self._long_down_step()
        self._dissolution_step()

    def _short_up_step(self):
        sp = self.sp
        if self.sw.delev_funds == 'none':
            return
        dl = [self.shorts[w] for w in sorted(self.shorts) if self.shorts[w].state == 'delev']
        if not dl:
            return
        spot0 = self.spot()
        twap, sma = self.twap_median(), self.sma30()
        elig = []
        for pos in dl:
            twap_ok = spot0 * BPS <= sp.pause_bps * twap
            sma_ok = spot0 * BPS <= sp.buy_ceiling_sma_bps * sma
            lapsed = False
            if self.sw.sma_pause_lapse:
                lapsed = pos.sma_paused >= SMA_PAUSE_LAPSE_BLOCKS
            if twap_ok and (sma_ok or lapsed):
                elig.append(pos)
            elif twap_ok and not sma_ok and self.sw.sma_pause_lapse and not lapsed:
                pos.sma_paused += 1
                if pos.sma_paused == SMA_PAUSE_LAPSE_BLOCKS:
                    self.stats['sma_lapses'] += 1
        if not elig:
            return
        p_cap = spot0 * (BPS + sp.pace_bps_per_block) // BPS
        a_tot = pm.alpha_for_up_impact(self.T, self.X, p_cap)
        Qe = sum(p.q for p in elig)
        self.blk_up_start = spot0 if self.blk_up_start is None else self.blk_up_start
        p_only = self.sw.delev_funds == 'P'
        for pos in elig:
            share = min(pos.q, a_tot * pos.q // Qe)
            live = pm.alpha_for_up_impact(self.T, self.X, p_cap)
            a = min(share, live)
            funds = pos.P if p_only else pos.P + pos.C
            if a > 0 and funds > 0:
                cost = pm.tao_in_for_alpha(self.T, self.X, a)
                if cost is None or cost > funds:
                    a = pm.out_alpha_for_tao(self.T, self.X, funds)
                    cost = funds
                    default = not p_only        # P-only: P spent, q stays owed; C keeps paying interest
                else:
                    default = False
            else:
                cost, default = 0, False
                if funds == 0 and not p_only:
                    default = True
            if cost:
                fromP = min(cost, pos.P)
                pos.P -= fromP
                pos.C -= cost - fromP
                self.custody -= cost
                self.T += cost - self._fee_tao(cost)
                self.X -= a
                self._burn_short(pos, a)
                self.net_minted_blk += a          # deleverage burns don't free open capacity (P8 net = mint - user burns)
                self.ev('short_delev_step', pos.owner, a, cost)
            if default and pos.q > 0:
                self._default_short(pos)
            elif pos.q == 0:
                self._refund_short(pos)
        self.blk_up_end = self.spot()

    def _long_down_step(self):
        lp, sp = self.lp, self.sp
        phaseA = self.phase == 'A'
        dl = [self.longs[w] for w in sorted(self.longs) if self.longs[w].state in ('delev', 'unwinding')]
        if not dl:
            return
        spot0 = self.spot()
        twap, sma = self.twap_median(), self.sma30()
        elig = []
        for pos in dl:
            twap_ok = spot0 * sp.pause_bps >= twap * BPS
            sma_ok = spot0 * sp.buy_ceiling_sma_bps >= sma * BPS
            lapsed = False
            if self.sw.sma_pause_lapse:
                lapsed = pos.sma_paused >= SMA_PAUSE_LAPSE_BLOCKS
            if not twap_ok and self.sw.twap_pause_lapse_blocks:
                pos.twap_paused = getattr(pos, 'twap_paused', 0) + 1
                if pos.twap_paused >= self.sw.twap_pause_lapse_blocks:
                    twap_ok = True
            if phaseA:
                ok = twap_ok
            else:
                ok = twap_ok and (sma_ok or lapsed)
                if twap_ok and not sma_ok and self.sw.sma_pause_lapse and not lapsed:
                    pos.sma_paused += 1
                    if pos.sma_paused == SMA_PAUSE_LAPSE_BLOCKS:
                        self.stats['sma_lapses'] += 1
            if ok:
                elig.append(pos)
        if not elig:
            return
        p_floor = cdiv(spot0 * BPS, BPS + sp.pace_bps_per_block)
        self.blk_dn_start = spot0 if self.blk_dn_start is None else self.blk_dn_start
        Ae = sum(p.A for p in elig)
        a_tot = self._alpha_for_down_impact(p_floor, Ae, sum(p.D for p in elig))
        for pos in elig:
            share = min(pos.A, a_tot * pos.A // Ae) if Ae else 0
            live = self._alpha_for_down_impact(p_floor, pos.A, pos.D)
            a = min(share, live)
            if a > 0:
                S = pm.out_tao_for_alpha(self.T, self.X, a)
                rep = min(S, pos.D)
                self.X += a - self._fee_alpha(a)
                self.T -= S if self.sw.long_virtual else S - rep
                pos.A -= a
                self.long_locked -= a
                pos.D -= rep
                self.long_lifted -= rep
                pos.C += S - rep
                self.custody += S - rep
                self.ev('long_delev_step', pos.owner, a, S, rep)
            if pos.A == 0:
                pay = min(pos.C, pos.D)
                pos.C -= pay
                pos.D -= pay
                self.long_lifted -= pay
                self.custody -= pay
                if not self.sw.long_virtual:
                    self.T += pay
                if pos.D > 0:
                    self._default_long(pos)
                elif not phaseA:
                    self._refund_long(pos)
                # phase A: surplus credited at phase B
        self.blk_dn_end = self.spot()

    def _alpha_for_down_impact(self, p_floor, a_max, D):
        """Largest alpha a <= a_max whose sale (+ repay of up to D, if G3 net) leaves spot >= p_floor."""
        lo, hi = 0, a_max
        T, X = self.T, self.X
        net = self.sw.long_pace_net_of_repay and not self.sw.long_virtual
        while lo < hi:
            mid = (lo + hi + 1) // 2
            S = pm.out_tao_for_alpha(T, X, mid)
            T2 = T - S + (min(S, D) if net else 0)
            if pm.price(T2, X + mid - pm.fee_of(mid)) >= p_floor:
                lo = mid
            else:
                hi = mid - 1
        return lo

    # ================================================================== dissolution (sections 1.8, 7.4)
    def flag(self):
        """Prune this subnet: v2.0 / v1.9.1 one-shot long sale then B/D/E/F atomically.
        ModelSwitches.phase_a restores the removed paced phase A (finding3 only)."""
        if self.phase is not None:
            return
        self.flag_block = self.block
        self.t_flag = self.T
        if self.sw.phase_a:
            for w in sorted(self.longs):
                self.longs[w].state = 'unwinding'
            self.phase = 'A'
            self.phase_log.append((self.phase, self.block))
        else:
            if self.sw.prune_sells_longs:
                self._prune_sell_longs()
            self.phase = 'B'
            self.phase_log.append((self.phase, self.block))
            while not self.dissolved:
                self._dissolution_step()
        self.ev('flag', self.T)

    def _prune_sell_longs(self):
        """v1.9.1 §7.4 phase A: accrue, one swap of A_book, pro-rata split, lender-first settle.
        Runs inside flag(), before T_A and r are read. No pace, no L9 pause."""
        for w in sorted(self.longs):
            self._accrue(self.longs[w], self.longs[w].D, self.sp.apr_bps)
        snaps = [(w, self.longs[w].A, self.longs[w].D, self.longs[w].C, self.longs[w].due)
                 for w in sorted(self.longs)]
        A_book = sum(a for _, a, _, _, _ in snaps)
        T0 = self.T
        P_book = 0
        if A_book > 0:
            P_book = pm.out_tao_for_alpha(self.T, self.X, A_book)
            self.X += A_book - self._fee_alpha(A_book)
            self.T -= P_book
            self.long_locked = 0
            for w, a, _, _, _ in snaps:
                self.longs[w].A = 0
        allocated = 0
        i24 = []
        n = len(snaps)
        for w, a_pos, D_pos, C_pos, due in snaps:
            pos = self.longs[w]
            p_pos = (P_book * a_pos // A_book) if A_book else 0
            allocated += p_pos
            claim = D_pos + due
            repay = min(claim, p_pos + pos.C)
            from_p = min(repay, p_pos)
            from_c = repay - from_p
            pos.C -= from_c
            self.custody -= from_c
            if not self.sw.long_virtual:
                self.T += repay
            else:
                self.T += min(pos.due, repay)        # interest part is real TAO for the pool; the D part retires unbacked TAO
            int_pay = min(pos.due, repay)
            if int_pay:
                pos.due -= int_pay
                pos.interest_paid += int_pay
                self.interest_to_pool += int_pay
                self.blk_interest += int_pay
                self.blk_T_interest_delta += int_pay
            d_pay = repay - int_pay
            pos.D -= d_pay
            self.long_lifted -= d_pay
            shortfall = claim - repay
            if pos.D > 0:
                self.long_shortfall += pos.D
                self.long_lifted -= pos.D
                if self.sw.long_virtual:
                    self.T -= pos.D
                self.stats['defaults_long'] += 1
                pos.D = 0
            refund = (p_pos - from_p) + pos.C
            i24.append(dict(w=w, p_pos=p_pos, repay=repay, shortfall=shortfall,
                            claim=claim, refund=refund, C=C_pos, D=D_pos, due=due))
            self.stats['prune_sell_repay'] = self.stats.get('prune_sell_repay', 0) + repay
            self.ev('long_prune_sell', w, a_pos, p_pos, repay, shortfall)
            surplus_p = p_pos - from_p
            if shortfall > 0:
                # default: forfeit deposit, refund nothing (C already applied to repay)
                assert surplus_p == 0 and pos.C == 0
                self.custody -= pos.deposit
                self.burned += pos.deposit
                del self.longs[pos.owner]
            else:
                if surplus_p:
                    self.credit(pos.owner, surplus_p)
                self._refund_long(pos)
        dust = P_book - allocated
        # Swap removed P_book from SubnetTAO; dust never belonged to any position, so put it back.
        self.T += dust
        self.stats['prune_P_book'] = P_book
        self.stats['prune_dust'] = dust
        self.stats['prune_i24'] = i24
        self.stats['prune_dT'] = self.T - T0
        # I24
        assert self.long_locked == 0 and self.long_lifted == 0 and not self.longs, 'I24 longs remain'
        if i24:
            assert 0 <= dust < n, f'I24 dust {dust}'
            assert P_book == sum(x['p_pos'] for x in i24) + dust, 'I24 P_book split'
            for x in i24:
                assert x['repay'] + x['shortfall'] == x['claim'], 'I24 claim'
                assert x['repay'] + x['refund'] == x['p_pos'] + x['C'], 'I24 refund'
                assert x['repay'] >= 0 and x['shortfall'] >= 0 and x['refund'] >= 0, 'I24 signs'
            # ΔT = -P_book + dust + Σ repay
            assert self.sw.long_virtual or self.stats['prune_dT'] == -P_book + dust + sum(x['repay'] for x in i24), 'I24 dT'

    def _dissolution_step(self):
        if self.phase is None or self.dissolved:
            return
        lp = self.lp
        if self.phase == 'A':
            if self.block - self.flag_block >= lp.unwind_blocks:
                self.phase = 'B'
                self.phase_log.append(('B', self.block))
            return
        if self.phase == 'B':
            self.t_a = self.T
            if self.sw.t_avg_for_r:
                tmin = min(self.t_ema, self.T)
            else:
                tmin = min(self.t_flag, self.t_ema, self.t_a)
            den = self.issuance
            num = tmin
            if self.sw.zone_excluded_from_r:
                zq = sum(p.zone_q for p in self.shorts.values())
                zp = sum(p.zone_p for p in self.shorts.values())
                den -= zq
                num = min(tmin + zp, self.t_flag + zp)
            self.r_fp = num * Q64 // max(den, 1)
            if self.sw.r_after_long_repay and self.longs:
                # lender-first: each long repays min(D, A*r + C) into the pot before r is final; r is the
                # largest fixed point of r = (num + sum repay_i(r)) / den (monotone, converges from below)
                for _ in range(60):
                    rep = sum(min(p.D, (p.A * self.r_fp >> 64) + p.C) for p in self.longs.values())
                    nr = (num + rep) * Q64 // max(den, 1)
                    if nr <= self.r_fp:
                        break
                    self.r_fp = nr
                self.stats['r_fix_iters'] = _ + 1
            self.pot = self.T
            self.T = 0
            # phase-A surpluses: longs fully sold in A are refunded here
            for w in sorted(self.longs):
                pos = self.longs[w]
                if pos.A == 0 and pos.D == 0:
                    self._refund_long_dissolve(pos, 0)
            self.phase = 'C'
            self.phase_log.append(('C', self.block))
            return
        r = self.r_fp
        if self.phase == 'C':
            for w in sorted(self.longs):
                pos = self.longs[w]
                assert ('L', w) not in self.settled
                self.settled.add(('L', w))
                ent = pos.A * r >> 64
                self.long_locked -= pos.A
                if ent >= pos.D:
                    self.stats['phaseC_repay'] = self.stats.get('phaseC_repay', 0) + pos.D
                    self.pot -= ent - pos.D
                    self.long_lifted -= pos.D
                    extra = ent - pos.D
                    pos.D = 0
                    self._refund_long_dissolve(pos, extra)
                else:
                    need = pos.D - ent
                    pay = min(pos.C, need)
                    pos.C -= pay
                    self.custody -= pay
                    self.pot += pay
                    self.stats['phaseC_repay'] = self.stats.get('phaseC_repay', 0) + pay + ent
                    sf = need - pay
                    self.long_lifted -= pos.D
                    self.long_shortfall += sf
                    pos.D = 0
                    self._refund_long_dissolve(pos, 0)
                pos.A = 0
            self.phase = 'D'
            self.phase_log.append(('D', self.block))
            return
        if self.phase == 'D':
            for w in sorted(self.shorts):
                pos = self.shorts[w]
                assert ('S', w) not in self.settled
                self.settled.add(('S', w))
                pay0 = pos.zone_p + ((pos.q - pos.zone_q) * r >> 64)
                if self.sw.dereg_pay_proceeds:
                    pay0 = max(pay0, pos.all_p)
                pay = min(pay0, pos.P + pos.C)        # I8 asserts this min never binds
                self.stats.setdefault('short_settle_bind', 0)
                if pay < pay0:
                    self.stats['short_settle_bind'] += 1
                fromP = min(pay, pos.P)
                pos.P -= fromP
                pos.C -= pay - fromP
                self.custody -= pay
                self.pot += pay
                self.synth -= pos.q
                amt = pos.C + pos.P + pos.deposit
                self.custody -= amt
                self.credit(w, amt)
                self.ev('short_settle', w, pos.q, pay, amt)
                del self.shorts[w]
            self.phase = 'E'
            self.phase_log.append(('E', self.block))
            return
        if self.phase == 'E':
            for w in sorted(self.stakes):
                s = self.stakes[w]
                pay = s * r >> 64
                self.payouts += pay
                self.credit(w, pay)
                self.ev('staker_paid', w, s, pay)
            assert self.pot >= self.payouts, 'I8/I17 pot < payouts'
            self.pot -= self.payouts
            self.phase = 'F'
            self.phase_log.append(('F', self.block))
            return
        if self.phase == 'F':
            self.stats['recycled_at_F'] = self.pot
            self.stats['r_fp'] = self.r_fp
            self.recycled += self.pot
            self.pot = 0
            self.dissolved = True
            self.phase_log.append(('done', self.block))

    def _refund_long_dissolve(self, pos, extra_from_pot):
        amt = pos.C + pos.deposit
        self.custody -= amt
        self.credit(pos.owner, amt + extra_from_pot)
        self.ev('long_settle', pos.owner, amt, extra_from_pot)
        del self.longs[pos.owner]

    # ------------------------------------------------------------------ accounting views
    def tao0_offset(self):
        return 0

    def tao_in_flow_user_only(self):
        # flow/volume are only written in user_buy/user_sell; lending legs never call them (structural, I18/I19)
        return True

    def total_tao(self):
        v = self.long_lifted if self.sw.long_virtual else 0
        return sum(self.balances.values()) + self.T - v + self.custody + self.burned + self.recycled + self.pot

    def wealth(self, who, mark='spot'):
        """TAO + alpha marked at a liquidation sale into the current pool (alpha only) + position equity at liquidation."""
        w = self.bal(who)
        T, X = self.T, self.X
        a = self.stakes.get(who, 0)
        if a and not self.dissolved:
            w += pm.out_tao_for_alpha(T, X, a)
        s = self.shorts.get(who)
        if s:
            c = pm.tao_in_for_alpha(T, X, s.q)
            w += s.C + s.P + s.deposit - (c if c is not None else s.C + s.P)
        l = self.longs.get(who)
        if l:
            S = pm.out_tao_for_alpha(T, X, l.A)
            w += max(0, l.C + S - l.D) + l.deposit
        return w
