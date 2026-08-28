// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import {CanonicalShareToken} from "./CanonicalShareToken.sol";
import {IMessageRecipient} from "./interfaces/IMessageRecipient.sol";
import {IUsdRails, IUSD_RAILS_ADDRESS} from "./interfaces/IUsdRails.sol";

/// @notice The single inbound door on the Bittensor EVM. Receives Hyperlane
/// messages, secures the deposit (mints canonical USD to the PSM escrow for
/// portal deposits), then hands the opaque SCALE envelope to the runtime via
/// the 0x814 precompile.
///
/// Failure discipline: this contract only reverts while funds are still safe
/// at origin (untrusted sender, rate-limited mint) — Hyperlane retries
/// delivery. Once `gatewayExecute` is reached, the runtime never reverts:
/// action failures fall back to a tUSD credit.
contract Gateway is IMessageRecipient {
    enum SenderKind {
        None,
        UsdPortal, // origin locks USD; we mint canonical USD backing here
        RemoteToken // origin burned canonical shares; runtime releases escrow
    }

    address public owner;
    address public mailbox;
    CanonicalShareToken public canonicalUsd;
    /// Keyless PSM escrow account (H160 mirror of the pallet escrow).
    address public psmEscrow;

    /// Trusted remote senders per (origin domain, sender address).
    mapping(uint32 => mapping(bytes32 => SenderKind)) public trustedSenders;

    event TrustedSenderSet(uint32 indexed origin, bytes32 indexed sender, SenderKind kind);
    event InboundExecuted(uint32 indexed origin, bytes32 indexed sender, uint64 amount);

    modifier onlyOwner() {
        require(msg.sender == owner, "not owner");
        _;
    }

    constructor(address owner_, address mailbox_, address canonicalUsd_, address psmEscrow_) {
        require(owner_ != address(0), "owner zero");
        owner = owner_;
        mailbox = mailbox_;
        canonicalUsd = CanonicalShareToken(canonicalUsd_);
        psmEscrow = psmEscrow_;
    }

    function setTrustedSender(uint32 origin, bytes32 sender, SenderKind kind) external onlyOwner {
        trustedSenders[origin][sender] = kind;
        emit TrustedSenderSet(origin, sender, kind);
    }

    /// @notice Hyperlane delivery. Body: abi.encode(uint64 amount, bytes envelope).
    function handle(uint32 origin, bytes32 sender, bytes calldata body) external payable override {
        require(msg.sender == mailbox, "not mailbox");
        SenderKind kind = trustedSenders[origin][sender];
        require(kind != SenderKind.None, "untrusted sender");

        (uint64 amount, bytes memory envelope) = abi.decode(body, (uint64, bytes));

        if (kind == SenderKind.UsdPortal && amount > 0) {
            // Secure the backing first: canonical USD into the PSM escrow.
            // Reverts here (e.g. mint window exhausted) leave funds locked at
            // origin and let the relayer retry later.
            canonicalUsd.mint(psmEscrow, amount);
        }

        IUsdRails(IUSD_RAILS_ADDRESS).gatewayExecute(amount, envelope);
        emit InboundExecuted(origin, sender, amount);
    }
}
