// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IGAMMA_ADDRESS = 0x0000000000000000000000000000000000000814;

/// Gamma amounts are base units (1e9 per gamma), TAO amounts rao and alpha amounts alpha base
/// units. Amounts and minGammaOut above uint64 revert, except alphaAmount, which the chain
/// clamps to the caller's stake.
interface IGammaPrecompile {
    /// Gamma the mint would credit now, base units (1e9 per gamma). Same valuation as the mint:
    /// min(gamma price, spot), then TAO/USD. Reverts on any error that stops the mint, including
    /// gamma disabled, TAO/USD older than 300 blocks, AmountTooLow, InsufficientLiquidity, SubtokenDisabled,
    /// and when that deposit cannot be credited (NonAssociatedColdKey, TooManyGammaDeposits).
    function quoteMintGamma(uint256 netuid, bytes32 depositHotkey, bytes32 recipientColdkey, uint256 amountRao)
        external view returns (uint256 gammaOut);

    /// Same for burning stake. Called off-chain, so it must NOT check the caller's stake.
    function quoteMintGammaWithStake(uint256 netuid, bytes32 stakeHotkey, bytes32 depositHotkey,
        bytes32 recipientColdkey, uint256 amountAlpha) external view returns (uint256 gammaOut);

    /// Deposit balance and owning coldkey. Balance 0 and coldkey 0 when the deposit does not exist.
    function getGammaDeposit(uint256 netuid, bytes32 depositHotkey)
        external view returns (uint256 balance, bytes32 coldkey);

    /// Not payable. Buys and burns alpha with amountRao from the caller's free TAO balance and credits
    /// recipientColdkey's deposit at depositHotkey (opening it with no daily spend cap if new;
    /// opening another coldkey's deposit takes at least 5 gamma, else AmountTooLow).
    /// Reverts below minGammaOut and with the same deposit checks as the quote.
    function mintGammaOnBehalf(uint256 netuid, bytes32 depositHotkey, bytes32 recipientColdkey,
        uint256 amountRao, uint256 minGammaOut) external returns (uint256 gammaMinted);

    /// Burns UP TO alphaAmount of the caller's stake on stakeHotkey (chain clamps to the stake) and credits
    /// recipientColdkey's deposit at depositHotkey. Same checks as mintGammaOnBehalf.
    function mintGammaWithStakeOnBehalf(uint256 netuid, bytes32 stakeHotkey, bytes32 depositHotkey,
        bytes32 recipientColdkey, uint256 alphaAmount, uint256 minGammaOut) external returns (uint256 gammaMinted);
}
