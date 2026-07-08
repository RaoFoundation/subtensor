"""The ``btcli`` command-line interface.

Built entirely on the SDK's public surface (``bittensor.Client`` and friends);
nothing in here may reach into private attributes. Requires the optional
``cli`` extra (typer + rich), which the base library deliberately does not
depend on.
"""

try:
    import rich as _rich  # noqa: F401
    import typer as _typer  # noqa: F401
except ModuleNotFoundError as _error:
    raise ModuleNotFoundError(
        f"the btcli command needs the optional CLI dependencies ({_error.name} "
        "is not installed); install them with: pip install 'bittensor[cli]'"
    ) from _error
