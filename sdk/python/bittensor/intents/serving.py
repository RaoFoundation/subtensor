"""Publishing a neuron's endpoint on-chain (axon, TLS axon, prometheus, service url).

This publishes connection info as chain data only — the SDK does not implement
the neuron networking layer itself.
"""

from __future__ import annotations

import ipaddress
from dataclasses import dataclass, field
from typing import Any

from .._generated import calls
from ..result import BittensorError
from .base import Intent
from .registry import register

NETUID_HELP = "Subnet on which to publish the endpoint."
IP_HELP = "Public IPv4 or IPv6 address of the endpoint, in standard dotted or colon notation."
PORT_HELP = "TCP port the endpoint listens on."
PROTOCOL_HELP = (
    "Application protocol tag stored alongside the endpoint; its meaning is subnet-defined."
)
VERSION_HELP = "Version number of the serving neuron's software, stored with the endpoint."

# Longest service url the chain stores, in UTF-8 bytes.
MAX_SERVICE_URL_LEN = 256


@register
@dataclass
class ServeAxon(Intent):
    """Publish this hotkey's axon endpoint (ip:port) for a subnet.

    Writes the connection info to chain storage so validators on the subnet
    know where to reach this miner or validator; it does not start any server —
    running the actual axon service is up to the caller. Signed by the hotkey,
    which must be registered on the subnet, and subject to the chain's serving
    rate limit (re-publishing too soon fails). Use ``serve_axon_tls`` if peers
    should verify a TLS certificate, and ``reset_axon`` to stop advertising the
    endpoint.
    """

    op = "serve_axon"
    signer = "hotkey"
    wraps = (("SubtensorModule", "serve_axon"),)

    netuid: int = field(metadata={"help": NETUID_HELP})
    ip: str = field(metadata={"help": IP_HELP})
    port: int = field(metadata={"help": PORT_HELP})
    protocol: int = field(default=4, metadata={"help": PROTOCOL_HELP})
    version: int = field(default=1, metadata={"help": VERSION_HELP})

    async def build(self, substrate, wallet: Any):
        address = ipaddress.ip_address(self.ip)
        return await substrate.compose(
            calls.SubtensorModule.serve_axon(
                netuid=self.netuid,
                version=self.version,
                ip=int(address),
                port=self.port,
                ip_type=address.version,
                protocol=self.protocol,
                placeholder1=0,
                placeholder2=0,
            )
        )

    def summary(self) -> str:
        return f"serve axon {self.ip}:{self.port} on netuid {self.netuid}"


@register
@dataclass
class ServeAxonTls(Intent):
    """Publish this hotkey's axon endpoint with a neuron certificate.

    Same as ``serve_axon`` plus a compact neuron certificate stored on chain:
    one algorithm byte followed by up to 64 bytes of public key — not an X.509
    TLS certificate blob (anything else fails to decode). The chain only
    publishes the key for peers to fetch; there is no chain-side TLS
    handshake, and running any TLS endpoint is up to the caller. Signed by
    the hotkey, which must be registered on the subnet. Use plain
    ``serve_axon`` when peers do not need a published key.
    """

    op = "serve_axon_tls"
    signer = "hotkey"
    wraps = (("SubtensorModule", "serve_axon_tls"),)

    netuid: int = field(metadata={"help": NETUID_HELP})
    ip: str = field(metadata={"help": IP_HELP})
    port: int = field(metadata={"help": PORT_HELP})
    certificate: str = field(
        metadata={
            "help": "Neuron certificate as 0x-prefixed hex: 1 algorithm byte followed "
            "by up to 64 bytes of public key. Not an X.509 certificate; other formats "
            "fail to decode on chain."
        }
    )
    protocol: int = field(default=4, metadata={"help": PROTOCOL_HELP})
    version: int = field(default=1, metadata={"help": VERSION_HELP})

    async def build(self, substrate, wallet: Any):
        address = ipaddress.ip_address(self.ip)
        return await substrate.compose(
            calls.SubtensorModule.serve_axon_tls(
                netuid=self.netuid,
                version=self.version,
                ip=int(address),
                port=self.port,
                ip_type=address.version,
                protocol=self.protocol,
                placeholder1=0,
                placeholder2=0,
                certificate=bytes.fromhex(self.certificate.removeprefix("0x")),
            )
        )

    def summary(self) -> str:
        return f"serve TLS axon {self.ip}:{self.port} on netuid {self.netuid}"


@register
@dataclass
class ServePrometheus(Intent):
    """Publish this hotkey's prometheus metrics endpoint (ip:port) for a subnet.

    Advertises where this neuron's prometheus metrics can be scraped, separate
    from the axon endpoint used for inter-neuron traffic. Signed by the hotkey,
    which must be registered on the subnet, and subject to the same serving
    rate limit as ``serve_axon``. Like the axon calls, this only writes chain
    data — running the metrics server is up to the caller.
    """

    op = "serve_prometheus"
    signer = "hotkey"
    wraps = (("SubtensorModule", "serve_prometheus"),)

    netuid: int = field(metadata={"help": NETUID_HELP})
    ip: str = field(metadata={"help": IP_HELP})
    port: int = field(metadata={"help": PORT_HELP})
    version: int = field(default=1, metadata={"help": VERSION_HELP})

    async def build(self, substrate, wallet: Any):
        address = ipaddress.ip_address(self.ip)
        return await substrate.compose(
            calls.SubtensorModule.serve_prometheus(
                netuid=self.netuid,
                version=self.version,
                ip=int(address),
                port=self.port,
                ip_type=address.version,
            )
        )

    def summary(self) -> str:
        return f"serve prometheus {self.ip}:{self.port} on netuid {self.netuid}"


@register
@dataclass
class SetServiceUrl(Intent):
    """Publish the url where this hotkey describes its services on a subnet.

    The chain stores the url only; the document it points to (the services,
    their interfaces, and the spender to name on gamma deposits) is the
    publisher's. Signed by the hotkey, which must be registered on the subnet.
    An empty url clears it.
    """

    op = "set_service_url"
    signer = "hotkey"
    wraps = (("SubtensorModule", "set_service_url"),)

    netuid: int = field(metadata={"help": "Subnet on which to publish the url."})
    url: str = field(
        metadata={"help": "Url of the services document, up to 256 bytes; empty clears it."}
    )

    def __post_init__(self):
        if len(self.url.encode()) > MAX_SERVICE_URL_LEN:
            raise BittensorError(
                f"set_service_url: the url is {len(self.url.encode())} bytes, "
                f"at most {MAX_SERVICE_URL_LEN} are allowed"
            )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.set_service_url(netuid=self.netuid, url=self.url.encode())
        )

    def summary(self) -> str:
        if not self.url:
            return f"clear service url on netuid {self.netuid}"
        return f"set service url {self.url} on netuid {self.netuid}"


@register
@dataclass
class ResetAxon(Intent):
    """Reset (stop serving) this hotkey's axon endpoint on a subnet.

    Republishes the axon record as a placeholder (ip 0, port 1, protocol 4) so
    validators stop routing traffic to the old address; the storage entry is
    not removed. Signed by the hotkey. Use this when taking a miner offline or
    before moving it, then publish the new address with ``serve_axon`` when it
    is back up.
    """

    op = "reset_axon"
    signer = "hotkey"
    wraps = (("SubtensorModule", "serve_axon"),)

    netuid: int = field(metadata={"help": "Subnet whose published axon endpoint to clear."})

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.serve_axon(
                netuid=self.netuid,
                version=0,
                ip=0,
                port=1,
                ip_type=4,
                protocol=4,
                placeholder1=0,
                placeholder2=0,
            )
        )

    def summary(self) -> str:
        return f"reset axon on netuid {self.netuid}"
