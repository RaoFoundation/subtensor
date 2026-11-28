// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title The messages between a portal and the hub of the canonical rails.
/// @notice Both directions are plain ABI encodings. The hub's side of this format is
/// `precompiles/src/rails_wire.rs` in the subtensor repository; the two must agree, and
/// `test/Wire.t.sol` holds the same test vectors as that file.
///
/// All amounts have 9 decimals, whatever a token has on its chain.
library Wire {
    /// @notice The version of the order encoding.
    uint8 internal constant ORDER_VERSION = 1;

    /// @notice What an order pays with, and what a credit pays.
    uint8 internal constant ASSET_STABLE = 0;
    uint8 internal constant ASSET_TAO = 1;
    uint8 internal constant ASSET_ALPHA = 2;

    /// @notice What an order asks for.
    uint8 internal constant TO_STABLE = 0;
    uint8 internal constant TO_TAO = 1;
    uint8 internal constant TO_ALPHA = 2;
    uint8 internal constant TO_CREDITS = 3;
    uint8 internal constant TO_NATIVE_TUSD = 4;
    uint8 internal constant TO_NATIVE_TAO = 5;

    /// @notice The kinds of message the hub sends.
    uint8 internal constant KIND_CREDIT = 1;
    uint8 internal constant KIND_REGISTER_TOKEN = 2;
    uint8 internal constant KIND_SLOTS = 3;

    /// @notice What a payer wants for what it pays.
    /// @param kind One of the `TO_` constants.
    /// @param netuid The subnet of an alpha token or of credits, else 0.
    /// @param recipient `TO_STABLE`, `TO_TAO`, `TO_ALPHA`: an address on this chain,
    /// left-padded. `TO_CREDITS`: the service's account on Bittensor. `TO_NATIVE_*`: an
    /// account on Bittensor.
    /// @param beneficiary `TO_CREDITS`: the address the credits are for, else 0.
    struct Destination {
        uint8 kind;
        uint16 netuid;
        bytes32 recipient;
        address beneficiary;
    }

    /// @notice An order: the portal has locked or burned `amount` of `source` for `payer`.
    struct Order {
        uint8 version;
        uint64 nonce;
        address payer;
        uint8 source;
        uint16 sourceNetuid;
        uint64 amount;
        uint8 destination;
        uint16 destinationNetuid;
        bytes32 recipient;
        address beneficiary;
        uint64 minOut;
        uint64 deadline;
    }

    /// @notice The hub tells the portal to pay `amount` of an asset to `to`.
    /// @param nonce The hub's sequence number of the message for this portal.
    /// @param answersOrder Whether the credit answers the order `order` of this portal.
    /// @param refund Whether it returns what that order took, not what it bought.
    /// @param rate For an alpha token: the alpha one unit is worth now, times 1e18. Else 0.
    /// @param slots How many more orders the portal may take: the hub has set aside what
    /// it needs to answer that many.
    struct Credit {
        uint64 nonce;
        uint8 asset;
        uint16 netuid;
        address to;
        uint64 amount;
        bool answersOrder;
        uint64 order;
        bool refund;
        uint128 rate;
        uint32 slots;
    }

    /// @notice The hub tells the portal to create the alpha token of `netuid`.
    /// @param token The address the hub's registry lists for it.
    struct RegisterToken {
        uint64 nonce;
        uint16 netuid;
        address token;
        string name;
        string symbol;
    }

    error NotAnAddress(bytes32 recipient);
    error NoRecipient();
    error UnknownDestination(uint8 kind);
    error UnusedField();

    /// @notice Reverts unless `destination` is one the hub can decode and the spoke can pay:
    /// the hub accepts an order in exactly one encoding, with every unused field zero, and
    /// nothing can be paid to the zero address. An order to the zero address would settle on
    /// the hub and then never pay out: a token cannot be minted to it.
    function check(Destination calldata destination) internal pure {
        bool isAddress = destination.kind <= TO_ALPHA;
        if (destination.kind > TO_NATIVE_TAO) revert UnknownDestination(destination.kind);
        if (destination.recipient == bytes32(0)) revert NoRecipient();
        if (isAddress && uint256(destination.recipient) >> 160 != 0) {
            revert NotAnAddress(destination.recipient);
        }
        bool hasNetuid = destination.kind == TO_ALPHA || destination.kind == TO_CREDITS;
        if (!hasNetuid && destination.netuid != 0) revert UnusedField();
        if (destination.kind == TO_CREDITS) {
            if (destination.beneficiary == address(0)) revert NoRecipient();
        } else if (destination.beneficiary != address(0)) {
            revert UnusedField();
        }
    }

    function encode(Order memory order) internal pure returns (bytes memory) {
        return abi.encode(order);
    }

    function decodeOrder(bytes calldata body) internal pure returns (Order memory) {
        return abi.decode(body, (Order));
    }

    /// @notice The kind of a message of the hub: its first word.
    function kind(bytes calldata body) internal pure returns (uint8) {
        return abi.decode(body, (uint8));
    }

    /// @dev A struct of static fields is encoded as its fields in a row, so the kind and
    /// the struct decode what the hub wrote as eleven words.
    function decodeCredit(bytes calldata body) internal pure returns (Credit memory credit) {
        (, credit) = abi.decode(body, (uint8, Credit));
    }

    function decodeRegisterToken(bytes calldata body)
        internal
        pure
        returns (RegisterToken memory register)
    {
        (, register.nonce, register.netuid, register.token, register.name, register.symbol) =
            abi.decode(body, (uint8, uint64, uint16, address, string, string));
    }

    /// @notice A message that only brings order slots: the hub's sequence number of the
    /// message, and how many more orders the portal may take.
    function decodeSlots(bytes calldata body) internal pure returns (uint64 nonce, uint32 slots) {
        (, nonce, slots) = abi.decode(body, (uint8, uint64, uint32));
    }
}
