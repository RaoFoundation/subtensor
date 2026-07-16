from typing import Any, Optional

# Wrapper pallets that report an *inner* DispatchResult while the outer
# extrinsic still emits System.ExtrinsicSuccess. Keyed by (module, event) →
# attribute holding the Result.
_NESTED_RESULT_EVENTS: dict[tuple[str, str], str] = {
    ("Sudo", "Sudid"): "sudo_result",
    ("Sudo", "SudoAsDone"): "sudo_result",
    ("Proxy", "ProxyExecuted"): "result",
    ("Multisig", "MultisigExecuted"): "result",
}


def _get_event_parts(event: dict) -> tuple[str, str, dict]:
    event_data = event["event"]
    return event_data["module_id"], event_data["event_id"], event_data["attributes"]


def nested_dispatch_error(events: list) -> Optional[Any]:
    """Inner ``Err`` payload from a Sudo/Proxy/Multisig wrapper event, if any.

    These wrappers dispatch a nested call: the outer extrinsic succeeds (and
    emits ``System.ExtrinsicSuccess``) even when the nested call fails. The
    failure lives only in the wrapper event's ``Result`` field.
    """
    for entry in events:
        record = entry.value if hasattr(entry, "value") else entry
        if not isinstance(record, dict):
            continue
        event = record.get("event", record)
        if not isinstance(event, dict):
            continue
        module_id = event.get("module_id")
        event_id = event.get("event_id")
        if not isinstance(module_id, str) or not isinstance(event_id, str):
            continue
        field = _NESTED_RESULT_EVENTS.get((module_id, event_id))
        if field is None:
            continue
        attributes = event.get("attributes")
        result = attributes.get(field) if isinstance(attributes, dict) else attributes
        if isinstance(result, dict) and "Err" in result:
            return result["Err"]
    return None


def dispatch_error_message(dispatch_error: Any, codec: Any) -> Optional[dict]:
    """Resolve a ``DispatchError`` (module or system) to the receipt error shape."""
    if dispatch_error is None:
        return None
    if isinstance(dispatch_error, str):
        return {"type": "System", "name": dispatch_error, "docs": dispatch_error}
    if not isinstance(dispatch_error, dict):
        return {"type": "System", "name": "DispatchError", "docs": str(dispatch_error)}
    module_error = normalize_module_error(dispatch_error)
    if module_error is not None:
        return codec.module_error(module_error["module_index"], module_error["error_index"])
    message = build_system_error_message(dispatch_error)
    if message is not None:
        return message
    if len(dispatch_error) == 1:
        name, detail = next(iter(dispatch_error.items()))
        return {"type": "System", "name": str(name), "docs": str(detail)}
    return {"type": "System", "name": "DispatchError", "docs": str(dispatch_error)}


def extract_total_fee_amount(events: list[dict]) -> tuple[int, bool]:
    total_fee_amount = 0
    has_transaction_fee_paid_event = False

    for event in events:
        module_id, event_id, attributes = _get_event_parts(event)
        if module_id == "TransactionPayment" and event_id == "TransactionFeePaid":
            total_fee_amount = attributes["actual_fee"]
            has_transaction_fee_paid_event = True

    return total_fee_amount, has_transaction_fee_paid_event


def extract_fallback_deposit_fee_amount(event: dict) -> int:
    module_id, event_id, attributes = _get_event_parts(event)
    if module_id == "Treasury" and event_id == "Deposit":
        return attributes["value"]

    if module_id == "Balances" and event_id == "Deposit":
        return attributes["amount"]

    return 0


def is_extrinsic_success_event(event: dict) -> bool:
    module_id, event_id, _ = _get_event_parts(event)
    return module_id == "System" and event_id == "ExtrinsicSuccess"


def is_extrinsic_failure_event(event: dict) -> bool:
    module_id, event_id, _ = _get_event_parts(event)
    return (module_id == "System" and event_id == "ExtrinsicFailed") or (
        module_id == "MevShield"
        and event_id
        in (
            # Older shield pallet event names.
            "DecryptedRejected",
            "DecryptionFailed",
            # Current shield pallet event names.
            "ExtrinsicDispatchFailed",
            "ExtrinsicDecodeFailed",
            "ExtrinsicExpired",
        )
    )


def extract_success_weight(event: dict) -> int | dict:
    _, _, attributes = _get_event_parts(event)
    if "dispatch_info" in attributes:
        return attributes["dispatch_info"]["weight"]

    # Backwards compatibility
    return attributes["weight"]


def extract_failure_details(event: dict) -> dict:
    module_id, event_id, attributes = _get_event_parts(event)
    has_weight = False
    weight = None
    dispatch_error = None
    error_message = None

    if module_id == "System":
        dispatch_info = attributes["dispatch_info"]
        has_weight = True
        weight = dispatch_info["weight"]
        dispatch_error = attributes["dispatch_error"]
    elif event_id == "DecryptedRejected":
        reason = attributes["reason"]
        has_weight = True
        weight = reason["post_info"]["actual_weight"]
        dispatch_error = reason["error"]
    elif event_id == "ExtrinsicDispatchFailed":
        dispatch_error = attributes["error"]
    elif event_id in ("ExtrinsicDecodeFailed", "ExtrinsicExpired", "DecryptionFailed"):
        error_message = {
            "type": "MevShield",
            "name": event_id,
            "docs": attributes.get("reason", event_id),
        }
    else:
        error_message = {
            "type": "MevShield",
            "name": event_id,
            "docs": str(attributes),
        }

    return {
        "has_weight": has_weight,
        "weight": weight,
        "dispatch_error": dispatch_error,
        "error_message": error_message,
    }


def normalize_module_error(dispatch_error: dict) -> Optional[dict]:
    if "Module" not in dispatch_error:
        return None

    module_dispatch_error = dispatch_error["Module"]
    if isinstance(module_dispatch_error, tuple):
        module_index = module_dispatch_error[0]
        error_index = module_dispatch_error[1]
    else:
        module_index = module_dispatch_error["index"]
        error_index = module_dispatch_error["error"]

    if isinstance(error_index, str):
        # Actual error index is first u8 in new [u8; 4] format
        error_index = int(error_index[2:4], 16)

    return {
        "module_index": module_index,
        "error_index": error_index,
    }


def build_system_error_message(dispatch_error: dict) -> Optional[dict]:
    name = None
    docs = None

    if "BadOrigin" in dispatch_error:
        name = "BadOrigin"
        docs = "Bad origin"
    elif "CannotLookup" in dispatch_error:
        name = "CannotLookup"
        docs = "Cannot lookup"
    elif "Other" in dispatch_error:
        name = "Other"
        docs = "Unspecified error occurred"
    elif "Token" in dispatch_error:
        name = "Token"
        docs = dispatch_error["Token"]

    if name is None:
        return None

    return {
        "type": "System",
        "name": name,
        "docs": docs,
    }
