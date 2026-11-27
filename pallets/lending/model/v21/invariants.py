"""v2.1 invariants: v201x checks minus the deleverage ones (I5 C<1d -> delev, I9/I15 pace, I27 exit), plus
I28: no position survives an idle pass with C == 0 and interest due; every position is 'open' (no other state)."""
from v201x.invariants import Checker as _C, InvariantViolation, _f  # noqa: F401


class Checker(_C):
    def _check(self, sn):
        saved = {}
        # I5 in the base asserts 'delev' state for C < 1 day of interest; in v2.1 there is no such state.
        for p in list(sn.shorts.values()) + list(sn.longs.values()):
            _f(p.state == 'open', 'I28', f'{p.owner} state {p.state}')
        _f(sn.blk_up_start is None and sn.blk_dn_start is None, 'I28', 'forced trade executed')
        if sn.phase is None:
            for p in sn.shorts.values():
                if (sn.block - p.opened_at) % sn.sp.charge_interval == 0:
                    _f(not (p.C == 0 and p.due > 0), 'I28', f'short {p.owner} C=0 with due after charge')
            for p in sn.longs.values():
                if (sn.block - p.opened_at) % sn.sp.charge_interval == 0:
                    _f(not (p.C == 0 and p.due > 0), 'I28', f'long {p.owner} C=0 with due after charge')
        # run the base checks with I5 neutralised: base I5 only fires for state != delev when C < 1 day at flat APR
        orig = sn.day_interest_short, sn.day_interest_long
        sn.day_interest_short = lambda q: -1
        sn.day_interest_long = lambda D: -1
        try:
            super()._check(sn)
        finally:
            sn.day_interest_short, sn.day_interest_long = orig
