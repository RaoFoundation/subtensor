"""Reference model of SPEC-v2.1.md (one subnet). Subclass of v201x.core.Subnet with bundle-C switches pinned.

v2.1 deltas vs v2.0.1 (+ Igor bundle C):
  * No forced trades on either side: no health trigger, no Deleveraging state, no paced buy-back / sell-down,
    no pause bands, no equity_floor / exit_hysteresis / min_interest_cover. poke / poke_long are no-ops.
  * Interest on current debt value (shorts q x TWAP_median, longs D) at
        rate_bps = min(cap_bps, apr_bps * u^k)         (iterated: r <- r*num//den, k times, saturating at cap)
    short u = q*TWAP_median / (C + P), long u = D / (A*TWAP_median + C). Same accrual/charge schedule, 100% to SubnetTAO.
    The rate is read at each accrual (every charge and every touch, before the touch mutates the position).
  * Default when an interest charge leaves C == 0 with interest still due.
      short: P -> SubnetTAO, q -> ResidualAlpha, deposit burned.
      long:  locked A sold in one swap; lender-first (interest, then D); surplus to owner; gap is a lender shortfall.
  * Partial close: allowed if coverage holds afterwards OR the position's u does not rise (pure de-risking).
"""
from v201x.core import Subnet as _Base, LendingError, Short, Long  # noqa: F401
from v201x import pool as pm
from .params import BPS, BPY, BPD, DEFAULT_SYNTH, DEFAULT_LONG, DEFAULT_SW


class Subnet(_Base):
    def __init__(self, T, X, out_alpha, ema_fp=None, sp=DEFAULT_SYNTH, lp=DEFAULT_LONG, sw=DEFAULT_SW, **kw):
        super().__init__(T, X, out_alpha, ema_fp=ema_fp, sp=sp, lp=lp, sw=sw, **kw)
        self.stats.update(c_exhaust_defaults=0, c_exhaust_P_to_pool=0, long_default_sales=0,
                          rate_max_bps=0, rate_capped_charges=0)

    # ------------------------------------------------------------------ rate curve (P5 v2.1)
    def rate_from(self, num, den):
        sp = self.sp
        cap = sp.rate_cap_bps
        if num <= 0:
            return sp.apr_bps if self.sw.rate_floor else 0
        if den <= 0:
            return cap
        r = sp.apr_bps
        for _ in range(sp.rate_k):
            r = r * num // den
            if r >= cap:
                return cap
        if self.sw.rate_floor:
            r = max(r, sp.apr_bps)
        return min(r, cap)

    def short_u(self, pos, C=None, P=None, q=None):
        q = pos.q if q is None else q
        C = pos.C if C is None else C
        P = pos.P if P is None else P
        if self.sw.u_price_slow:
            return q * min(self.twap_median(), self.sma30()) >> 64, C + P
        return self.short_notional(q), C + P

    def long_u(self, pos, C=None, A=None, D=None):
        A = pos.A if A is None else A
        C = pos.C if C is None else C
        D = pos.D if D is None else D
        px = max(self.twap_median(), self.sma30()) if self.sw.u_price_slow else self.twap_median()
        return D, (A * px >> 64) + C

    def rate_bps(self, pos, side):
        num, den = self.short_u(pos) if side == 'S' else self.long_u(pos)
        return self.rate_from(num, den)

    def _debt(self, pos, side):
        return self.short_notional(pos.q) if side == 'S' else pos.D

    def day_interest(self, pos, side):
        return self._debt(pos, side) * self.rate_bps(pos, side) // (BPS * 365)

    # ------------------------------------------------------------------ interest
    def _accrue_pos(self, pos, side):
        r = self.rate_bps(pos, side)
        if r > self.stats['rate_max_bps']:
            self.stats['rate_max_bps'] = r
        self._accrue(pos, self._debt(pos, side), r)
        return r

    def _pending(self, pos, side):
        n = self.block - pos.last_accrued
        due = pos.due
        if n > 0:
            due += (self._debt(pos, side) * self.rate_bps(pos, side) * n + pos.rem) // (BPS * BPY)
        return min(due, pos.C)

    def _pending_raw(self, pos, side='S'):
        n = self.block - pos.last_accrued
        due = pos.due
        if n > 0:
            due += (self._debt(pos, side) * self.rate_bps(pos, side) * n + pos.rem) // (BPS * BPY)
        return due

    def _charge(self, pos, side):
        r = self._accrue_pos(pos, side)
        if r >= self.sp.rate_cap_bps:
            self.stats['rate_capped_charges'] += 1
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
            self.ev('interest', side, pos.owner, c, r)
        return c

    # touches accrue at the pre-touch u, then mutate
    def add_collateral(self, who, amt):
        pos = self.shorts.get(who)
        if pos is not None and not self.dissolving():
            self._charge(pos, 'S')
        return super().add_collateral(who, amt)

    def add_long_collateral(self, who, amt):
        pos = self.longs.get(who)
        if pos is not None and not self.dissolving():
            self._charge(pos, 'L')
        return super().add_long_collateral(who, amt)

    # ------------------------------------------------------------------ withdraw: coverage + 7 days at the current rate
    def day_interest_short(self, q):
        # base helper (used by v201x withdraw / I5); only called with an existing position's q in v2.1 paths below
        return self.short_notional(q) * self.sp.apr_bps // (BPS * 365)

    def withdraw_collateral(self, who, amt):
        pos = self.shorts.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.dissolving():
            raise LendingError('DissolutionInProgress')
        pend = self._pending(pos, 'S')
        if amt > pos.C - pend:
            raise LendingError('WithdrawBreaksCoverage')
        need = self.short_need(pos.q, self.synth, self.T, self.X, self.spot(), self.twap_median(), self.sma30())
        Cafter = pos.C - pend - amt
        buf = self.short_notional(pos.q) * self.rate_from(*self.short_u(pos, C=Cafter)) // (BPS * 365) * self.sp.withdraw_buffer_days
        if need is None or Cafter + pos.P < need + buf:
            raise LendingError('WithdrawBufferBreached')
        self._charge(pos, 'S')
        pos.C -= amt
        self.custody -= amt
        self.credit(who, amt)
        self.ev('short_withdraw', who, amt)

    def withdraw_long_collateral(self, who, amt):
        pos = self.longs.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.phase is not None:
            raise LendingError('DissolutionInProgress')
        pend = self._pending(pos, 'L')
        if amt > pos.C - pend:
            raise LendingError('LongWithdrawBreaksCoverage')
        need = self.long_need(pos.D, pos.A, self.long_locked, self.T, self.X, self.spot(), self.twap_median(), None)
        Cafter = pos.C - pend - amt
        buf = pos.D * self.rate_from(*self.long_u(pos, C=Cafter)) // (BPS * 365) * self.sp.withdraw_buffer_days
        if Cafter < need + buf:
            raise LendingError('LongWithdrawBreaksCoverage')
        self._charge(pos, 'L')
        pos.C -= amt
        self.custody -= amt
        self.credit(who, amt)
        self.ev('long_withdraw', who, amt)

    def max_long_withdraw(self, who):
        pos = self.longs[who]
        need = self.long_need(pos.D, pos.A, self.long_locked, self.T, self.X, self.spot(), self.twap_median(), None)
        buf = self.day_interest(pos, 'L') * self.sp.withdraw_buffer_days
        return max(0, pos.C - pos.due - need - buf)

    # ------------------------------------------------------------------ close (partial: coverage OR u non-increasing)
    def close_short(self, who, alpha, max_tao_in=None):
        pos = self.shorts.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.dissolving():
            raise LendingError('DissolutionInProgress')
        alpha = min(alpha, pos.q)
        pend = self._pending(pos, 'S')
        cost = pm.tao_in_for_alpha(self.T, self.X, alpha)
        if cost is None or (max_tao_in is not None and cost > max_tao_in):
            raise LendingError('SlippageExceeded')
        if cost > pos.P + pos.C - pend:
            raise LendingError('InsufficientFunds')
        full = alpha == pos.q
        fromP = min(cost, pos.P)
        fromC = cost - fromP
        T2, X2 = self.T + cost - pm.fee_of(cost), self.X - alpha
        if not full:
            q2 = pos.q - alpha
            C2, P2 = pos.C - pend - fromC, pos.P - fromP
            need = self.short_need(q2, self.synth - alpha, T2, X2, pm.price(T2, X2), self.twap_median(), self.sma30())
            cov_ok = need is not None and C2 + P2 >= need
            # u2 <= u1  <=>  q2 * (C1 + P1) <= q1 * (C2 + P2)   (same TWAP both sides)
            derisk = self.sw.partial_close_derisk and q2 * (pos.C - pend + pos.P) <= pos.q * (C2 + P2)
            if not (cov_ok or derisk):
                raise LendingError('CoverageNotMet')
        self._fee_tao(cost)
        self.T, self.X = T2, X2
        self._charge(pos, 'S')
        pos.P -= fromP
        pos.C -= fromC
        self.custody -= cost
        self._burn_short(pos, alpha)
        self.ev('short_close', who, alpha, cost)
        if full:
            self._refund_short(pos)
        return cost

    def close_long(self, who, alpha, min_tao_out=0):
        pos = self.longs.get(who)
        if pos is None:
            raise LendingError('NoPosition')
        if self.dissolving():
            raise LendingError('DissolutionInProgress')
        alpha = min(alpha, pos.A)
        pend = self._pending(pos, 'L')
        S = pm.out_tao_for_alpha(self.T, self.X, alpha)
        if S < min_tao_out:
            raise LendingError('SlippageExceeded')
        full = alpha == pos.A
        rep = min(S, pos.D)
        surplus = S - rep
        T2, X2 = self.T - S, self.X + alpha - pm.fee_of(alpha)      # virtual long: repay retires the unbacked credit
        if full:
            if S + pos.C - pend < pos.D:
                raise LendingError('CloseInsolvent')
            fromC = pos.D - rep
        else:
            fromC = 0
            D2, A2, C2 = pos.D - rep, pos.A - alpha, pos.C - pend
            need = self.long_need(D2, A2, self.long_locked - alpha, T2, X2, pm.price(T2, X2), self.twap_median(), None)
            cov_ok = C2 >= need
            tw = self.twap_median()
            # u2 <= u1  <=>  D2 * (A1*tw + C1) <= D1 * (A2*tw + C2)
            derisk = self.sw.partial_close_derisk and D2 * ((pos.A * tw >> 64) + C2) <= pos.D * ((A2 * tw >> 64) + C2)
            if not (cov_ok or derisk):
                raise LendingError('LongCoverageNotMet')
        self._fee_alpha(alpha)
        self.T, self.X = T2, X2
        self._charge(pos, 'L')
        pos.A -= alpha
        self.long_locked -= alpha
        pos.D -= rep + fromC
        self.long_lifted -= rep + fromC
        pos.C -= fromC
        self.custody -= fromC
        self.credit(who, surplus)
        self.ev('long_close', who, alpha, S, rep + fromC, surplus)
        if full:
            self._refund_long(pos)
        return S

    # ------------------------------------------------------------------ no health, no forced trades
    def poke(self, who):
        return None

    def poke_long(self, who):
        return None

    def end_block(self):
        sp = self.sp
        if self.dissolved:
            return
        if self.phase in (None, 'A'):
            for who in sorted(self.shorts):
                pos = self.shorts[who]
                if (self.block - pos.opened_at) % sp.charge_interval == 0:
                    self._charge(pos, 'S')
                    if pos.C == 0 and pos.due > 0:
                        self.stats['c_exhaust_defaults'] += 1
                        self.stats['c_exhaust_P_to_pool'] += pos.P
                        self._default_short(pos)
            for who in sorted(self.longs):
                pos = self.longs[who]
                if (self.block - pos.opened_at) % sp.charge_interval == 0:
                    self._charge(pos, 'L')
                    if pos.C == 0 and pos.due > 0:
                        self._default_long_sale(pos)
        self._dissolution_step()

    def _default_long_sale(self, pos):
        """C exhausted: sell the locked A in one swap, lender-first (interest then D), surplus to owner, gap = shortfall."""
        a = pos.A
        S = pm.out_tao_for_alpha(self.T, self.X, a) if a else 0
        if a:
            self.X += a - self._fee_alpha(a)
            self.T -= S
            self.long_locked -= a
            pos.A = 0
        int_pay = min(pos.due, S)
        if int_pay:
            self.T += int_pay
            pos.due -= int_pay
            pos.interest_paid += int_pay
            self.interest_to_pool += int_pay
            self.blk_interest += int_pay
            self.blk_T_interest_delta += int_pay
        d_pay = min(pos.D, S - int_pay)
        pos.D -= d_pay
        self.long_lifted -= d_pay
        surplus = S - int_pay - d_pay
        self.stats['long_default_sales'] += 1
        self.ev('long_default_sale', pos.owner, a, S, d_pay, surplus)
        if pos.D > 0:
            self.long_shortfall += pos.D
            self.long_lifted -= pos.D
            self.T -= pos.D                         # write the unbacked (virtual) claim off SubnetTAO
            self.stats['defaults_long'] += 1
            self.ev('long_default', pos.owner, pos.D)
            pos.D = 0
        pos.due = 0                                  # unpaid interest beyond S is forgiven (lender shortfall counts D only)
        if surplus:
            self.credit(pos.owner, surplus)
        self.custody -= pos.C + pos.deposit
        self.burned += pos.deposit
        self.T += pos.C                              # C is 0 by construction
        del self.longs[pos.owner]

    def _default_short(self, pos):
        # P to SubnetTAO, q to ResidualAlpha, deposit burned (v201x _default_short already sends C + P dust to T)
        return super()._default_short(pos)

    def _prune_sell_longs(self):
        for w in sorted(self.longs):
            self._accrue_pos(self.longs[w], 'L')      # convex rate up to the prune block; base accrue then adds 0 blocks
        return super()._prune_sell_longs()
