"""On-chain identity: human-readable metadata for coldkeys and subnets.

Read back with the ``identity`` / ``subnet_identity`` reads. String fields are
sent as their utf-8 bytes; the chain stores raw ``Vec<u8>``.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from .._generated import calls
from .base import Intent
from .registry import register


@register
@dataclass
class SetIdentity(Intent):
    """Publish an on-chain identity (name, links, description) for the coldkey.

    Stores public, human-readable metadata against the signing coldkey so
    explorers, wallets, and delegators can recognize it — useful for validator
    operators and subnet owners who want a public face. The signing coldkey
    must own at least one hotkey registered on some subnet, else the call
    fails with ``HotKeyNotRegisteredInNetwork``. Everything submitted is
    public and permanent history on chain, so include nothing sensitive.
    Calling again overwrites the whole identity (empty fields clear their
    previous values); the chain enforces length limits on each field. Purely
    cosmetic: no effect on balances, stake, or permissions.
    """

    op = "set_identity"
    signer = "coldkey"
    wraps = (("SubtensorModule", "set_identity"),)

    name: str = field(metadata={"help": "Display name shown for this coldkey."})
    url: str = field(default="", metadata={"help": "Website associated with this identity."})
    github_repo: str = field(default="", metadata={"help": "GitHub repository URL."})
    image: str = field(default="", metadata={"help": "Avatar or logo image URL."})
    discord: str = field(default="", metadata={"help": "Discord handle or server invite."})
    description: str = field(
        default="", metadata={"help": "Short free-text description of who this key belongs to."}
    )
    additional: str = field(
        default="", metadata={"help": "Any extra free-text information to publish."}
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.set_identity(
                name=self.name.encode(),
                url=self.url.encode(),
                github_repo=self.github_repo.encode(),
                image=self.image.encode(),
                discord=self.discord.encode(),
                description=self.description.encode(),
                additional=self.additional.encode(),
            )
        )

    def summary(self) -> str:
        return f"set on-chain identity to {self.name!r}"


@register
@dataclass
class SetSubnetIdentity(Intent):
    """Publish identity metadata for a subnet (signer must be the subnet owner).

    Stores the subnet's public profile — name, links, contact, logo — so
    explorers and participants can identify it. Owner-only: the signing
    coldkey must own the subnet. Everything submitted is public and permanent
    history on chain. Calling again overwrites the whole record (empty fields
    clear their previous values); the chain enforces length limits on each
    field. Purely cosmetic — for the token ticker use ``update_symbol``, and
    for economics use ``set_hyperparameter``.
    """

    op = "set_subnet_identity"
    signer = "coldkey"
    origin = "subnet_owner"
    wraps = (("SubtensorModule", "set_subnet_identity"),)

    netuid: int = field(
        metadata={"help": "Subnet the identity is for; the signer must be its owner."}
    )
    subnet_name: str = field(metadata={"help": "Display name shown for the subnet."})
    github_repo: str = field(
        default="", metadata={"help": "GitHub repository URL for the subnet's code."}
    )
    subnet_contact: str = field(
        default="", metadata={"help": "Contact address for the subnet operators."}
    )
    subnet_url: str = field(default="", metadata={"help": "Website for the subnet."})
    discord: str = field(default="", metadata={"help": "Discord handle or server invite."})
    description: str = field(
        default="", metadata={"help": "Short free-text description of what the subnet does."}
    )
    logo_url: str = field(default="", metadata={"help": "Logo image URL for the subnet."})
    additional: str = field(
        default="", metadata={"help": "Any extra free-text information to publish."}
    )

    async def build(self, substrate, wallet: Any):
        return await substrate.compose(
            calls.SubtensorModule.set_subnet_identity(
                netuid=self.netuid,
                subnet_name=self.subnet_name.encode(),
                github_repo=self.github_repo.encode(),
                subnet_contact=self.subnet_contact.encode(),
                subnet_url=self.subnet_url.encode(),
                discord=self.discord.encode(),
                description=self.description.encode(),
                logo_url=self.logo_url.encode(),
                additional=self.additional.encode(),
            )
        )

    def summary(self) -> str:
        return f"set subnet {self.netuid} identity to {self.subnet_name!r}"
