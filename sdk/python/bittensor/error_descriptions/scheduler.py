"""Chain error descriptions declared (first) by the `Scheduler` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "FailedToSchedule": (
        "The scheduler could not place the call into the agenda, typically because the target "
        "block's agenda is full or the schedule parameters are unusable. Check `Agenda` at the "
        "target block against `MaxScheduledPerBlock` and pick a different block if it is "
        "saturated."
    ),
    "Named": (
        "An unnamed scheduler function was used on a task that was scheduled with a name. Use "
        "the named variants (`cancel_named`, `schedule_named`) with the task's id, which you "
        "can find via the `Lookup` storage map."
    ),
    "RescheduleNoChange": (
        "The reschedule was rejected because the new dispatch time equals the task's currently "
        "scheduled time. Check the task's existing slot in `Agenda` and pass a genuinely "
        "different `when` block."
    ),
    "TargetBlockNumberInPast": (
        "The scheduler was given a dispatch block that is not in the future. Compare the `when` "
        "argument against the current block number and choose a strictly later block."
    ),
}
