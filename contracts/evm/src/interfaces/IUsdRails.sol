// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/// @notice Interface of the USD rails precompile at address 0x814 on the
/// Bittensor EVM. The envelope is an opaque, SCALE-encoded, versioned byte
/// string defined by the runtime (`subtensor_runtime_common::rails`);
/// Solidity never parses its internals.
///
/// All amounts are in rails units: 9 decimals, matching rao.
interface IUsdRails {
    /// @notice Execute an inbound gateway envelope. Callable only by the
    /// registered Gateway contract. `amount` must equal the envelope's
    /// internal amount; the runtime cross-checks and reverts on mismatch.
    function gatewayExecute(uint64 amount, bytes calldata envelope) external;

    /// @notice Quote tUSD -> TAO through the canonical pool.
    function simSwapUsdForTao(uint64 amountUsd) external view returns (uint64 taoOut);

    /// @notice Quote TAO -> tUSD through the canonical pool.
    function simSwapTaoForUsd(uint64 amountTao) external view returns (uint64 usdOut);

    /// @notice tUSD ledger balance of a substrate account (public key bytes).
    function tusdBalanceOf(bytes32 account) external view returns (uint64);

    /// @notice Canonical pool state: (taoReserve, tusdReserve, feeBps).
    function poolState() external view returns (uint64, uint64, uint16);

    /// @notice The ERC-20 backing a registered PSM asset (zero if unknown).
    function assetErc20(uint32 assetId) external view returns (address);

    /// @notice Share index (1e9 fixed point) for a subnet's canonical shares.
    function shareIndexE9(uint16 netuid) external view returns (uint64);

    /// @notice Next inbound envelope nonce expected by the sequential guard.
    function nextNonce() external view returns (uint64);
}

address constant IUSD_RAILS_ADDRESS = 0x0000000000000000000000000000000000000814;
