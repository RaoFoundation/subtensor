"""I1-I26 of SPEC-v1.md v2.0 (sections 5 and 7.6) plus model-level conservation checks, asserted every block.
check(sn) raises InvariantViolation with the invariant id. Static per-call postconditions (I4, I13, I14, I24)
are asserted inside the extrinsics in core.py; dissolution solvency (I8/I17) is asserted inside the phases.
I20 (SMA-pause lapse counter) and I22 (P24) are removed. I26 (sudo setters) is pallet-only."""
from .params import BPS, Q64, ALPHA_SUPPLY_CAP
from . import pool as pm


class InvariantViolation(AssertionError):
    pass


def _f(cond, iid, msg=''):
    if not cond:
        raise InvariantViolation(f'{iid}: {msg}')


class Checker:
    def __init__(self, sn):
        self.sn = sn
        self.tao0 = sn.total_tao()
        self.emitted0 = sn.emitted_tao
        self.interest0 = sn.interest_to_pool
        self.flow0 = sn.tao_in_flow
        self.seen_episode = {}
        self.violations = []

    def check(self, strict=True):
        sn = self.sn
        try:
            self._check(sn)
        except InvariantViolation as e:
            self.violations.append((sn.block, str(e)))
            if strict:
                raise

    def _check(self, sn):
        sp, lp = sn.sp, sn.lp
        # I1
        _f(sum(p.q for p in sn.shorts.values()) == sn.synth, 'I1', f'{sum(p.q for p in sn.shorts.values())} != {sn.synth}')
        # I2 / I7: Issuance_ex moves only with emission; all alpha is somewhere
        if not sn.dissolved and sn.phase not in ('C', 'D', 'E', 'F'):
            held = sn.X + sum(sn.stakes.values()) + sn.long_locked
            _f(held == sn.issuance, 'I2', f'held {held} != issuance {sn.issuance}')
            _f(sn.issuance_ex() == sn.ex_expected, 'I7', f'ex {sn.issuance_ex()} != {sn.ex_expected}')
        # I3 custody and no negative balances
        cust = sum(p.C + p.P + p.deposit for p in sn.shorts.values()) + sum(p.C + p.deposit for p in sn.longs.values())
        _f(cust == sn.custody, 'I3', f'custody {sn.custody} != sum {cust}')
        for p in sn.shorts.values():
            _f(min(p.C, p.P, p.q, p.zone_q, p.zone_p) >= 0, 'I3', 'negative short field')
            _f(p.zone_q <= p.q, 'I11', 'zone_q > q')
        for p in sn.longs.values():
            _f(min(p.C, p.A, p.D) >= 0, 'I3', 'negative long field')
        _f(min(sn.balances.values(), default=0) >= 0, 'I3', 'negative balance')
        _f(sn.T >= 0 and sn.X >= 0, 'I3', 'negative reserve')
        # TAO conservation (model): created only by emission
        _f(sn.total_tao() - sn.tao0_offset() == self.tao0 + (sn.emitted_tao - self.emitted0), 'CONS',
           f'total {sn.total_tao()} vs {self.tao0 + sn.emitted_tao - self.emitted0}')
        # I5: C below one day of interest -> deleveraging after the idle pass
        if sn.phase is None:
            for p in sn.shorts.values():
                if p.C < sn.day_interest_short(p.q):
                    _f(p.state == 'delev', 'I5', f'short {p.owner} C<1d interest but {p.state}')
            for p in sn.longs.values():
                if p.C < sn.day_interest_long(p.D):
                    _f(p.state in ('delev', 'unwinding'), 'I5', f'long {p.owner} C<1d interest but {p.state}')
        # I9: up-impact of short deleverage <= pace
        if sn.blk_up_start is not None and sn.blk_up_end is not None:
            _f(sn.blk_up_end * BPS <= sn.blk_up_start * (BPS + sp.pace_bps_per_block), 'I9',
               f'up impact {sn.blk_up_end / sn.blk_up_start - 1:.6f}')
        # P8 net minted per block
        # (checked in the extrinsic against the pre-trade SubnetAlphaIn)
        # I12
        _f(sum(p.D for p in sn.longs.values()) == sn.long_lifted, 'I12', 'sum D != LongLiftedTao')
        _f(sum(p.A for p in sn.longs.values()) == sn.long_locked, 'I12', 'sum A != LongLockedAlpha')
        # I14 is a postcondition at open; I15 down-impact
        if sn.blk_dn_start is not None and sn.blk_dn_end is not None:
            _f(sn.blk_dn_end * (BPS + sp.pace_bps_per_block) >= sn.blk_dn_start * BPS, 'I15',
               f'down impact {sn.blk_dn_end / sn.blk_dn_start - 1:.6f}')
        # I16 shortfall stat equals cumulative write-off (by construction); I18/I19: interest never touches flow
        _f(sn.tao_in_flow_user_only(), 'I18/I19', 'flow counter touched by a lending leg')
        _f(sn.blk_T_interest_delta == sn.blk_interest, 'I19', 'interest credit != charged')
        # I20 removed in v2.0 (no SMA-pause lapse / counter)
        if not sn.sw.sma_pause_lapse:
            for p in list(sn.shorts.values()) + list(sn.longs.values()):
                _f(p.sma_paused == 0, 'I20-removed', f'{p.owner} sma_paused written without lapse switch')
        # I21 (v1.7)
        _f(sn.issuance <= ALPHA_SUPPLY_CAP, 'I21', 'alpha supply above 21M')
        # I23 (v1.9): per-position health costs / values sum to the whole-book figure within rounding
        if sn.sw.health_book and not sn.dissolved and sn.phase in (None, 'A'):
            if sn.shorts and sn.synth:
                cb = pm.buy_cost_nofee(sn.T, sn.X, sn.synth)
                if cb is not None:
                    tot = sum(sn.short_health_cost(p) for p in sn.shorts.values())
                    _f(0 <= tot - cb <= len(sn.shorts), 'I23', f'short sum {tot} vs book {cb}')
                    if len(sn.shorts) == 1:
                        p = next(iter(sn.shorts.values()))
                        _f(sn.short_health_cost(p) == pm.buy_cost_nofee(sn.T, sn.X, p.q), 'I23', 'lone short')
            if sn.longs and sn.long_locked:
                vb = pm.sell_proceeds_nofee(sn.T, sn.X, sn.long_locked)
                tot = sum(sn.long_health_value(p) for p in sn.longs.values())
                _f(0 <= vb - tot <= len(sn.longs), 'I23', f'long sum {tot} vs book {vb}')
        # P21 never lets residual grow through opens while tripped: residual only grows at default (no check needed)
        # I24 (v1.9.1): after a prune sale, no long book remains
        if sn.stats.get('prune_P_book') is not None and sn.flag_block is not None and not sn.sw.phase_a:
            _f(sn.long_locked == 0 and sn.long_lifted == 0, 'I24', 'long book remains after prune sale')
        # I25: rank ring is always full; default sma30 is the full-ring mean (not EMA / partial)
        _f(len(sn.sma_days) == sn.sp.rank_days, 'I25', f'ring length {len(sn.sma_days)}')
        if not sn.sw.rank_cold_start and not sn.sw.rank_partial:
            _f(sn.sma30() == sum(sn.sma_days) // len(sn.sma_days), 'I25', 'sma30 is not the full-ring mean')
