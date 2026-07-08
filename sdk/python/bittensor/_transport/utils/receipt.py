from typing import Optional


def _get_event_parts(event: dict) -> tuple[str, str, dict]:
    event_data = event["event"]
    return event_data["module_id"], event_data["event_id"], event_data["attributes"]


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
