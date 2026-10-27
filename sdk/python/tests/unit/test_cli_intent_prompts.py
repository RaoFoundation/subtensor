from __future__ import annotations

from types import SimpleNamespace

from bittensor.cli import intent_prompts
from bittensor.cli.prompt import PromptSpec


def _spec(field: str) -> PromptSpec:
    return PromptSpec(
        field=field,
        flag=f"--{field.replace('_', '-')}",
        help=None,
        parse=lambda _ctx, value: value,
    )


def _context():
    return SimpleNamespace(
        assume_yes=False,
        uses_extension_signer=lambda: False,
        output=SimpleNamespace(message=lambda _message: None),
    )


def test_add_stake_policy_places_target_before_decorated_amount(monkeypatch):
    monkeypatch.setattr(intent_prompts, "interactive", lambda _ctx: True)
    missing = [_spec("netuid"), _spec("hotkey_ss58"), _spec("amount_tao")]

    result = intent_prompts.apply_intent_prompt_policy(_context(), "add_stake", missing, {})

    assert [spec.field for spec in result] == ["netuid", "hotkey_ss58", "amount_tao"]
    assert result[1].custom is not None
    assert result[2].custom is not None


def test_remove_stake_policy_moves_source_picker_first(monkeypatch):
    monkeypatch.setattr(intent_prompts, "interactive", lambda _ctx: True)
    missing = [_spec("netuid"), _spec("hotkey_ss58"), _spec("amount_alpha")]

    result = intent_prompts.apply_intent_prompt_policy(_context(), "remove_stake", missing, {})

    assert [spec.field for spec in result] == ["hotkey_ss58", "netuid", "amount_alpha"]
    assert result[0].custom is not None
