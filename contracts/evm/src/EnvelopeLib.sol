// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/// @notice SCALE encoders for the runtime's v1 gateway envelope, built
/// on-chain so buys and sells are single-transaction UX. Byte-for-byte
/// mirror of `subtensor_runtime_common::rails::GatewayEnvelope::to_wire`
/// (see the golden vectors in `common/src/rails.rs` and
/// `ts-tests/suites/rails/fixtures/golden-envelopes.json`).
///
/// Layout: [version=1] ++ SCALE(asset, amount, dest, action, nonce), with
/// all integers little-endian and `dest` a raw 32-byte account (zero for
/// buy/sell: the runtime derives the fallback destination from the EVM
/// recipient).
library EnvelopeLib {
    /// AssetId::Usd(id) codec index.
    uint8 internal constant ASSET_USD = 3;
    /// AssetId::Alpha(netuid) codec index.
    uint8 internal constant ASSET_ALPHA = 2;
    /// GatewayAction::BuyShares codec index.
    uint8 internal constant ACTION_BUY_SHARES = 4;
    /// GatewayAction::SellShares codec index.
    uint8 internal constant ACTION_SELL_SHARES = 5;

    function u16le(uint16 v) internal pure returns (bytes2) {
        return bytes2(uint16((v >> 8) | (v << 8)));
    }

    function u32le(uint32 v) internal pure returns (bytes4) {
        v = ((v >> 8) & 0x00FF00FF) | ((v & 0x00FF00FF) << 8);
        v = (v >> 16) | (v << 16);
        return bytes4(v);
    }

    function u64le(uint64 v) internal pure returns (bytes8) {
        v = ((v >> 8) & 0x00FF00FF00FF00FF) | ((v & 0x00FF00FF00FF00FF) << 8);
        v = ((v >> 16) & 0x0000FFFF0000FFFF) | ((v & 0x0000FFFF0000FFFF) << 16);
        v = (v >> 32) | (v << 32);
        return bytes8(v);
    }

    /// @notice BuyShares envelope: USD deposit -> staked alpha -> shares
    /// minted to `recipient` on `domain` (the spoke chain's own domain).
    function buyShares(
        uint32 usdAssetId,
        uint64 amountUsd,
        uint16 netuid,
        address recipient,
        uint64 minAlpha,
        uint32 domain,
        uint64 nonce
    ) internal pure returns (bytes memory) {
        return abi.encodePacked(
            uint8(1), // envelope version
            ASSET_USD,
            u32le(usdAssetId),
            u64le(amountUsd),
            bytes32(0), // dest: runtime uses the recipient's mirror account
            ACTION_BUY_SHARES,
            u16le(netuid),
            recipient,
            u64le(minAlpha),
            u32le(domain),
            u64le(nonce)
        );
    }

    /// @notice SellShares envelope: `shares` were burned on the spoke; the
    /// runtime unstakes escrow alpha and releases USD to `recipient` on
    /// `domain`.
    function sellShares(
        uint16 netuid,
        uint64 shares,
        address recipient,
        uint32 usdAssetId,
        uint64 minUsd,
        uint32 domain,
        uint64 nonce
    ) internal pure returns (bytes memory) {
        return abi.encodePacked(
            uint8(1), // envelope version
            ASSET_ALPHA,
            u16le(netuid),
            u64le(shares),
            bytes32(0),
            ACTION_SELL_SHARES,
            u16le(netuid),
            recipient,
            u32le(usdAssetId),
            u64le(minUsd),
            u32le(domain),
            u64le(nonce)
        );
    }
}
