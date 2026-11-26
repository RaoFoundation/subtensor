// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IGAMMA_ADDRESS = 0x0000000000000000000000000000000000000814;

/// Gamma deposits, balances and spends, each call dispatched as the caller's mapped account.
/// TAO amounts are in rao, alpha amounts in alpha rao, and gamma amounts in gamma units
/// (10^9 per US dollar). An optional argument is a `has…` flag and its value; the value is
/// ignored when the flag is false.
interface IGamma {
    /// Buys alpha on `netuid` with `amount` rao of the caller's TAO, burns it, and credits its
    /// gamma to the caller's deposit at `hotkey`, which must have the given daily spend cap, or
    /// opens with it; no cap means none. With no `hotkey` it credits the caller's balance, which
    /// takes no cap. Fails below `minAmountOut` gamma. Returns the gamma credited.
    function mintGamma(
        bool hasHotkey,
        bytes32 hotkey,
        uint16 netuid,
        uint64 amount,
        uint64 minAmountOut,
        bool hasDailySpendCap,
        uint64 dailySpendCap
    ) external returns (uint64 credited);

    /// Burns up to `alphaAmount` of the caller's stake on `originHotkey` and credits its gamma
    /// as `mintGamma` does, to the deposit at `destinationHotkey` or, with none, the balance.
    /// Returns the gamma credited.
    function mintGammaWithStake(
        bytes32 originHotkey,
        bool hasDestinationHotkey,
        bytes32 destinationHotkey,
        uint16 netuid,
        uint64 alphaAmount,
        uint64 minAmountOut,
        bool hasDailySpendCap,
        uint64 dailySpendCap
    ) external returns (uint64 credited);

    /// Moves up to `amount` gamma from the caller's deposit at `originHotkey` to
    /// `destinationHotkey`: the whole balance, or at least 0.1 gamma leaving at least as much.
    /// Returns the gamma moved.
    function moveGamma(
        bytes32 originHotkey,
        bytes32 destinationHotkey,
        uint16 netuid,
        uint64 amount
    ) external returns (uint64 moved);

    /// Closes the caller's empty deposit at `hotkey`.
    function closeGammaDeposit(bytes32 hotkey, uint16 netuid) external;

    /// Sets the daily spend cap of the caller's deposit at `hotkey`; no cap removes it.
    function setGammaDepositDailySpendCap(
        bytes32 hotkey,
        uint16 netuid,
        bool hasDailySpendCap,
        uint64 dailySpendCap
    ) external;

    /// Spends `amount` of the deposit whose hotkey is the caller at `service`, and adds it to
    /// what the caller has spent there. Costs gas, unlike the native spend.
    function spendGamma(bytes32 service, uint16 netuid, uint64 amount) external;

    /// Moves up to `amount` gamma from the caller's balance to its deposit at `hotkey`, opening
    /// one with no cap. Returns the gamma moved.
    function depositGamma(
        bytes32 hotkey,
        uint16 netuid,
        uint64 amount
    ) external returns (uint64 moved);

    /// Moves up to `amount` gamma from the caller's deposit at `hotkey` to its balance. Returns
    /// the gamma moved.
    function withdrawGamma(
        bytes32 hotkey,
        uint16 netuid,
        uint64 amount
    ) external returns (uint64 moved);

    /// Sends exactly `amount` gamma from the caller's balance, or its deposit at
    /// `originHotkey`, to `destinationColdkey`'s balance, or its deposit at
    /// `destinationHotkey`, opening one with no daily spend cap. Another coldkey opens one only
    /// while the receiver holds fewer than 32 across subnets, and not during its coldkey swap.
    function transferGamma(
        bytes32 destinationColdkey,
        bool hasOriginHotkey,
        bytes32 originHotkey,
        bool hasDestinationHotkey,
        bytes32 destinationHotkey,
        uint16 netuid,
        uint64 amount
    ) external;

    /// Sets the service of the caller's hotkey on `netuid`: the url, at most 256 bytes, where
    /// it describes its services. The hotkey needs a validator permit there, or must be the
    /// subnet owner's hotkey with the caller the subnet owner. An empty url removes it.
    function setService(uint16 netuid, bytes32 hotkey, string calldata url) external;

    /// The deposit at `hotkey` and what its hotkey can spend from it now; `exists` is false
    /// when there is none.
    function getGammaDeposit(
        uint16 netuid,
        bytes32 hotkey
    )
        external
        view
        returns (
            bool exists,
            bytes32 coldkey,
            uint64 balance,
            bool hasDailySpendCap,
            uint64 dailySpendCap,
            uint64 spendable
        );

    /// The hotkeys holding `coldkey`'s deposits, at most 64.
    function getGammaDepositHotkeys(
        uint16 netuid,
        bytes32 coldkey
    ) external view returns (bytes32[] memory hotkeys);

    /// `coldkey`'s gamma held at no hotkey.
    function getGammaBalance(
        uint16 netuid,
        bytes32 coldkey
    ) external view returns (uint64);

    /// The url of `hotkey`'s service on `netuid`; `exists` is false when it has none.
    function getService(
        uint16 netuid,
        bytes32 hotkey
    ) external view returns (bool exists, string memory url);

    /// The gamma `hotkey` has spent at `service`.
    function getGammaSpent(
        uint16 netuid,
        bytes32 service,
        bytes32 hotkey
    ) external view returns (uint64);

    /// Whether gamma can be minted at all; a subnet also needs its own flag
    /// (`ISubnet.getSubnetGammaEnabled`).
    function getGammaEnabled() external view returns (bool);

    /// The alpha burned, TAO value in rao and gamma `mintGamma` would mint for `amount` now.
    /// Fails, with no return data, when the mint would fail.
    function simMintGamma(
        uint16 netuid,
        uint64 amount
    ) external view returns (uint64 alpha, uint64 value, uint64 gamma);

    /// `alphaAmount` and the TAO value in rao and gamma of burning it now. Fails, with no
    /// return data, when the mint would fail; it reads no stake.
    function simMintGammaWithStake(
        uint16 netuid,
        uint64 alphaAmount
    ) external view returns (uint64 alpha, uint64 value, uint64 gamma);
}
