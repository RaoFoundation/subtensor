// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

// USD rails precompile: the single EVM door into the canonical rails kernel
// (PSM + internal tUSD/TAO pool + share accounting). The Gateway contract
// delivers bridge envelopes through gatewayExecute; everything else is a
// read-only view.
address constant IUSD_RAILS_ADDRESS = 0x0000000000000000000000000000000000000814;

interface IUsdRails {
    /// Execute an inbound bridge envelope (SCALE-encoded, versioned).
    /// Callable only by the registered Gateway contract.
    function gatewayExecute(uint64 amount, bytes calldata envelope) external;

    /// Quote tUSD -> TAO through the canonical pool (9 decimals both sides).
    function simSwapUsdForTao(uint64 amountUsd) external view returns (uint64);

    /// Quote TAO -> tUSD through the canonical pool (9 decimals both sides).
    function simSwapTaoForUsd(uint64 amountTao) external view returns (uint64);

    /// tUSD ledger balance of a substrate account (public key bytes).
    function tusdBalanceOf(bytes32 account) external view returns (uint64);

    /// Canonical pool state: (taoReserve, tusdReserve, feeBps).
    function poolState() external view returns (uint64, uint64, uint16);

    /// The ERC-20 contract backing a registered PSM asset (zero if unknown).
    function assetErc20(uint32 assetId) external view returns (address);

    /// Share index (1e9 fixed point) for a subnet's canonical shares.
    function shareIndexE9(uint16 netuid) external view returns (uint64);

    /// Next inbound envelope nonce expected by the sequential guard.
    function nextNonce() external view returns (uint64);
}
