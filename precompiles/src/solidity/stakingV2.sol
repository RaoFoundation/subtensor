pragma solidity ^0.8.0;

address constant ISTAKING_ADDRESS = 0x0000000000000000000000000000000000000805;

interface IStaking {
    /// A coldkey's non-zero alpha stake position on a subnet.
    struct StakeInfo {
        bytes32 hotkey;
        uint256 stake;
    }

    /// One coldkey's stake lock, rolled forward to the queried block.
    struct ColdkeyLockInfo {
        bool exists;
        bytes32 hotkey;
        uint256 locked_mass;
        uint128 conviction;
        bool is_perpetual;
    }

    /// Rolled aggregate lock state targeting one hotkey.
    struct HotkeyLockInfo {
        bool exists;
        uint256 locked_mass;
        uint128 conviction;
    }

    /**
     * @dev Adds a subtensor stake `amount` associated with the `hotkey`.
     *
     * This function allows external accounts and contracts to stake TAO into the subtensor pallet,
     * which effectively calls `add_stake` on the subtensor pallet with specified hotkey as a parameter
     * and coldkey being the hashed address mapping of H160 sender address to Substrate ss58 address as
     * implemented in Frontier HashedAddressMapping:
     * https://github.com/polkadot-evm/frontier/blob/2e219e17a526125da003e64ef22ec037917083fa/frame/evm/src/lib.rs#L739
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param amount The amount to stake in rao.
     * @param netuid The subnet to stake to (uint256).
     *
     * Requirements:
     * - `hotkey` must be a valid hotkey registered on the network, ensuring that the stake is
     *   correctly attributed.
     */
    function addStake(
        bytes32 hotkey,
        uint256 amount,
        uint256 netuid
    ) external payable;

    /**
     * @dev Removes a subtensor stake `amount` from the specified `hotkey`.
     *
     * This function allows external accounts and contracts to unstake TAO from the subtensor pallet,
     * which effectively calls `remove_stake` on the subtensor pallet with specified hotkey as a parameter
     * and coldkey being the hashed address mapping of H160 sender address to Substrate ss58 address as
     * implemented in Frontier HashedAddressMapping:
     * https://github.com/polkadot-evm/frontier/blob/2e219e17a526125da003e64ef22ec037917083fa/frame/evm/src/lib.rs#L739
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param amount The amount to unstake in alpha.
     * @param netuid The subnet to stake to (uint256).
     *
     * Requirements:
     * - `hotkey` must be a valid hotkey registered on the network, ensuring that the stake is
     *   correctly attributed.
     * - The existing stake amount must be not lower than specified amount
     */
    function removeStake(
        bytes32 hotkey,
        uint256 amount,
        uint256 netuid
    ) external payable;

    /**
     * @dev Moves a subtensor stake `amount` associated with the `hotkey` to a different hotkey
     * `destination_hotkey`.
     *
     * This function allows external accounts and contracts to move staked TAO from one hotkey to another,
     * which effectively calls `move_stake` on the subtensor pallet with specified origin and destination
     * hotkeys as parameters being the hashed address mappings of H160 sender address to Substrate ss58
     * address as implemented in Frontier HashedAddressMapping:
     * https://github.com/polkadot-evm/frontier/blob/2e219e17a526125da003e64ef22ec037917083fa/frame/evm/src/lib.rs#L739
     *
     * @param origin_hotkey The origin hotkey public key (32 bytes).
     * @param destination_hotkey The destination hotkey public key (32 bytes).
     * @param origin_netuid The subnet to move stake from (uint256).
     * @param destination_netuid The subnet to move stake to (uint256).
     * @param amount The amount to move in rao.
     *
     * Requirements:
     * - `origin_hotkey` and `destination_hotkey` must be valid hotkeys registered on the network, ensuring
     * that the stake is correctly attributed.
     */
    function moveStake(
        bytes32 origin_hotkey,
        bytes32 destination_hotkey,
        uint256 origin_netuid,
        uint256 destination_netuid,
        uint256 amount
    ) external payable;

    /**
     * @dev Transfer a subtensor stake `amount` associated with the transaction signer to a different coldkey
     * `destination_coldkey`.
     *
     * This function allows external accounts and contracts to transfer staked TAO to another coldkey,
     * which effectively calls `transfer_stake` on the subtensor pallet with specified destination
     * coldkey as a parameter being the hashed address mapping of H160 sender address to Substrate ss58
     * address as implemented in Frontier HashedAddressMapping:
     * https://github.com/polkadot-evm/frontier/blob/2e219e17a526125da003e64ef22ec037917083fa/frame/evm/src/lib.rs#L739
     *
     * @param destination_coldkey The destination coldkey public key (32 bytes).
     * @param hotkey The hotkey public key (32 bytes).
     * @param origin_netuid The subnet to move stake from (uint256).
     * @param destination_netuid The subnet to move stake to (uint256).
     * @param amount The amount to move in rao.
     *
     * Requirements:
     * - `origin_hotkey` and `destination_hotkey` must be valid hotkeys registered on the network, ensuring
     * that the stake is correctly attributed.
     */
    function transferStake(
        bytes32 destination_coldkey,
        bytes32 hotkey,
        uint256 origin_netuid,
        uint256 destination_netuid,
        uint256 amount
    ) external payable;

    /**
     * @dev Returns the amount of RAO staked by the coldkey.
     *
     * This function allows external accounts and contracts to query the amount of RAO staked by the coldkey
     * which effectively calls `get_total_coldkey_stake` on the subtensor pallet with
     * specified coldkey as a parameter.
     *
     * @param coldkey The coldkey public key (32 bytes).
     * @return The amount of RAO staked by the coldkey.
     */
    function getTotalColdkeyStake(
        bytes32 coldkey
    ) external view returns (uint256);

    /**
     * @dev Returns the coldkey's total alpha stake on one subnet.
     *
     * @param coldkey The coldkey public key (32 bytes).
     * @param netuid The subnet containing the stake position (uint256).
     * @return The coldkey's total alpha stake on the subnet.
     */
    function getTotalColdkeyStakeOnSubnet(
        bytes32 coldkey,
        uint256 netuid
    ) external view returns (uint256);

    /**
     * @dev Returns the total amount of stake under a hotkey (delegative or otherwise)
     *
     * This function allows external accounts and contracts to query the total amount of RAO staked under a hotkey
     * which effectively calls `get_total_hotkey_stake` on the subtensor pallet with
     * specified hotkey as a parameter.
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @return The total amount of RAO staked under the hotkey.
     */
    function getTotalHotkeyStake(
        bytes32 hotkey
    ) external view returns (uint256);

    /**
     * @dev Returns the stake amount associated with the specified `hotkey` and `coldkey`.
     *
     * This function retrieves the current stake amount linked to a specific hotkey and coldkey pair.
     * It is a view function, meaning it does not modify the state of the contract and is free to call.
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param coldkey The coldkey public key (32 bytes).
     * @param netuid The subnet the stake is on (uint256).
     * @return The current stake amount in uint256 format.
     */
    function getStake(
        bytes32 hotkey,
        bytes32 coldkey,
        uint256 netuid
    ) external view returns (uint256);

    /**
     * @dev Returns non-zero alpha stake positions for up to 64 caller-supplied
     * distinct hotkeys. Duplicate hotkeys revert. Callers that need more must
     * split their hotkeys across calls.
     * This function does not read the coldkey's unbounded historical hotkey index.
     *
     * @param coldkey The coldkey public key (32 bytes).
     * @param netuid The subnet to query.
     * @param hotkeys The distinct candidate hotkeys to query (maximum 64).
     * @return positions Non-zero hotkey and alpha stake pairs.
     */
    function getStakeInfoForColdkeyAndNetuid(
        bytes32 coldkey,
        uint256 netuid,
        bytes32[] calldata hotkeys
    ) external view returns (StakeInfo[] memory positions);

    /**
     * @dev Delegates staking to a proxy account.
     *
     * @param delegate The public key (32 bytes) of the delegate.
     */
    function addProxy(bytes32 delegate) external payable;

    /**
     * @dev Removes staking proxy account.
     *
     * @param delegate The public key (32 bytes) of the delegate.
     */
    function removeProxy(bytes32 delegate) external payable;

    /**
     * @dev Returns the validators that have staked alpha under a hotkey.
     *
     * This function retrieves the validators that have staked alpha under a specific hotkey.
     * It is a view function, meaning it does not modify the state of the contract and is free to call.
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param netuid The subnet the stake is on (uint256).
     * @return An array of validators that have staked alpha under the hotkey.
     */
    function getAlphaStakedValidators(
        bytes32 hotkey,
        uint256 netuid
    ) external view returns (uint256[] memory);

    /**
     * @dev Returns the total amount of alpha staked under a hotkey.
     *
     * This function retrieves the total amount of alpha staked under a specific hotkey.
     * It is a view function, meaning it does not modify the state of the contract and is free to call.
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param netuid The subnet the stake is on (uint256).
     * @return The total amount of alpha staked under the hotkey.
     */
    function getTotalAlphaStaked(
        bytes32 hotkey,
        uint256 netuid
    ) external view returns (uint256);

    /**
     * @dev Returns the minimum required stake for a nominator.
     *
     * This function retrieves the minimum required stake for a nominator.
     * It is a view function, meaning it does not modify the state of the contract and is free to call.
     *
     * @return The minimum required stake for a nominator.
     */
    function getNominatorMinRequiredStake() external view returns (uint256);

    /**
     * @dev Returns DefaultMinStake. This is a base value only; operation fees,
     * price conversion, and full-unstake rules can change the accepted amount.
     *
     * @return defaultMinStake The current DefaultMinStake value in rao.
     */
    function getDefaultMinStake()
        external
        view
        returns (uint256 defaultMinStake);

    /**
     * @dev Locks existing alpha stake on a subnet and builds conviction for a hotkey.
     *
     * The lock is a subnet-wide unstaking floor for the caller. It does not
     * move stake and may target a different hotkey from the staked positions.
     * Repeated calls top up the same lock; use `moveLock` to change its target.
     *
     * @param hotkey Hotkey that receives the conviction.
     * @param amount Alpha to add to the lock.
     * @param netuid Subnet on which the alpha is locked.
     */
    function lockStake(
        bytes32 hotkey,
        uint256 amount,
        uint256 netuid
    ) external payable;

    /**
     * @dev Moves the caller's existing subnet lock to another hotkey.
     *
     * Current decayed mass is preserved. Conviction is preserved when the
     * source and destination hotkeys share an owner; otherwise it resets.
     */
    function moveLock(
        bytes32 destinationHotkey,
        uint256 netuid
    ) external payable;

    /**
     * @dev Selects perpetual or decaying behavior for the caller's subnet lock.
     *
     * There is no direct unlock operation. Disabling perpetual mode lets the
     * locked mass decay according to the runtime unlock rate.
     */
    function setPerpetualLock(
        uint256 netuid,
        bool enabled
    ) external payable;

    /**
     * @dev Sets whether the caller rejects incoming alpha that carries a lock.
     *
     * Rejection is enabled by default. Pass false to opt into receiving locked
     * alpha through compatible stake transfers or coldkey swaps.
     */
    function setRejectLockedAlpha(bool enabled) external payable;

    /**
     * @dev Returns a coldkey's lock rolled forward to the current block.
     *
     * `conviction` is exact unsigned Q64.64 bits. Divide it by 2^64 to obtain
     * conviction in alpha rao. `exists` is false once the rolled lock crosses
     * the runtime cleanup threshold, even if its stale storage row remains.
     */
    function getColdkeyLock(
        bytes32 coldkey,
        uint256 netuid
    ) external view returns (ColdkeyLockInfo memory lockInfo);

    /**
     * @dev Returns rolled aggregate lock state targeting a hotkey.
     *
     * Perpetual and decaying buckets are combined. For the subnet owner hotkey,
     * the owner-specific buckets are included as well. `exists` is derived
     * from the rolled aggregate, so fully expired stale buckets do not count.
     */
    function getHotkeyLock(
        bytes32 hotkey,
        uint256 netuid
    ) external view returns (HotkeyLockInfo memory lockInfo);

    /**
     * @dev Returns exact rolled Q64.64 conviction for distinct candidate hotkeys.
     *
     * Results align with `hotkeys`. The list is capped at 64 to keep gas and
     * storage work bounded; larger metagraphs can be queried in batches.
     */
    function getHotkeyConvictions(
        uint256 netuid,
        bytes32[] calldata hotkeys
    ) external view returns (uint128[] memory convictions);

    /**
     * @dev Returns `(unlockRate, maturityRate)` in blocks.
     */
    function getLockRates()
        external
        view
        returns (uint64 unlockRate, uint64 maturityRate);

    /**
     * @dev Returns whether a coldkey rejects incoming locked alpha.
     */
    function getRejectLockedAlpha(
        bytes32 coldkey
    ) external view returns (bool);

    /**
     * @dev Adds a subtensor stake `amount` associated with the `hotkey` within a price limit.
     *
     * This function allows external accounts and contracts to stake TAO into the subtensor pallet,
     * which effectively calls `add_stake_limit` on the subtensor pallet with specified hotkey as a parameter
     * and coldkey being the hashed address mapping of H160 sender address to Substrate ss58 address as
     * implemented in Frontier HashedAddressMapping:
     * https://github.com/polkadot-evm/frontier/blob/2e219e17a526125da003e64ef22ec037917083fa/frame/evm/src/lib.rs#L739
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param amount The amount to stake in rao.
     * @param limit_price The price limit to stake at in rao. Number of rao per alpha.
     * @param allow_partial Whether to allow partial stake.
     * @param netuid The subnet to stake to (uint256).
     *
     * Requirements:
     * - `hotkey` must be a valid hotkey registered on the network, ensuring that the stake is
     *   correctly attributed.
     */
    function addStakeLimit(
        bytes32 hotkey,
        uint256 amount,
        uint256 limit_price,
        bool allow_partial,
        uint256 netuid
    ) external payable;

    /**
     * @dev Removes a subtensor stake `amount` from the specified `hotkey` within a price limit.
     *
     * This function allows external accounts and contracts to unstake TAO from the subtensor pallet,
     * which effectively calls `remove_stake_limit` on the subtensor pallet with specified hotkey as a parameter
     * and coldkey being the hashed address mapping of H160 sender address to Substrate ss58 address as
     * implemented in Frontier HashedAddressMapping:
     * https://github.com/polkadot-evm/frontier/blob/2e219e17a526125da003e64ef22ec037917083fa/frame/evm/src/lib.rs#L739
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param amount The amount to unstake in alpha.
     * @param limit_price The price limit to unstake at in rao. Number of rao per alpha.
     * @param allow_partial Whether to allow partial unstake.
     * @param netuid The subnet to stake to (uint256).
     *
     * Requirements:
     * - `hotkey` must be a valid hotkey registered on the network, ensuring that the stake is
     *   correctly attributed.
     * - The existing stake amount must be not lower than specified amount
     */
    function removeStakeLimit(
        bytes32 hotkey,
        uint256 amount,
        uint256 limit_price,
        bool allow_partial,
        uint256 netuid
    ) external payable;

    /**
     * @dev Removes all stake from a hotkey on a subnet with a price limit.
     *
     * This function allows external accounts and contracts to remove all stake from a specified hotkey
     * on a subnet, with an optional limit price for alpha token at which or better (higher) the staking
     * should execute. Without a limit price, it removes all the stake similar to `removeStake` function.
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param netuid The subnet to remove stake from (uint256).
     */
    function removeStakeFull(bytes32 hotkey, uint256 netuid) external payable;

    /**
     * @dev Removes all stake from a hotkey on a subnet with a price limit.
     *
     * This function allows external accounts and contracts to remove all stake from a specified hotkey
     * on a subnet, with an optional limit price for alpha token at which or better (higher) the staking
     * should execute. Without a limit price, it removes all the stake similar to `removeStake` function.
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param netuid The subnet to remove stake from (uint256).
     * @param limitPrice The limit price for alpha token (uint256).
     */
    function removeStakeFullLimit(
        bytes32 hotkey,
        uint256 netuid,
        uint256 limitPrice
    ) external payable;

    /**
     * @dev Burns alpha tokens from the specified hotkey's stake on a subnet.
     *
     * This function allows external accounts and contracts to permanently burn (destroy) alpha tokens
     * from their stake on a specified hotkey and subnet. The burned tokens are removed from circulation
     * and cannot be recovered.
     *
     * @param hotkey The hotkey public key (32 bytes).
     * @param amount The amount of alpha to burn (uint256).
     * @param netuid The subnet to burn from (uint256).
     *
     * Requirements:
     * - `hotkey` must be a valid hotkey registered on the network.
     * - The caller must have sufficient alpha staked to the specified hotkey on the subnet.
     * - `amount` must be greater than zero and not exceed the staked amount.
     */
    function burnAlpha(
        bytes32 hotkey,
        uint256 amount,
        uint256 netuid
    ) external payable;

    /**
     * @dev Set how much the caller approves the spender to use the provided amount of subnet tokens
     * on its behalf in a later call.
     *
     * This is similar to ERC20 approve, and then allows smart contract to transfer with permission from
     * other accounts during their execution. They can then act as escrows while knowing from whom
     * the funds comes from, which is not possible if the spender called `transfer` towards the contract
     * (no callback).
     *
     * @param spenderAddress Address allowed to spend funds from the caller.
     * @param netuid The approved subnet token.
     * @param absoluteAmount New approval amount, will overwrite previous value.
     */
    function approve(
        address spenderAddress,
        uint256 netuid,
        uint256 absoluteAmount
    ) external;

    /**
     * @dev Get how much the source allows the spender to use their subnet tokens
     *
     * @param sourceAddress Address of the source making the allowance.
     * @param spenderAddress Address allowed to spend funds from the source.
     * @param netuid The approved subnet token.
     */
    function allowance(
        address sourceAddress,
        address spenderAddress,
        uint256 netuid
    ) external view returns (uint256);

    /**
     * @dev Increase how much the caller approves the spender to use the provided amount of subnet tokens
     * on its behalf in a later call.
     *
     * This is similar to ERC20 increaseAllowance, and then allows smart contract to transfer with permission from
     * other accounts during their execution. They can then act as escrows while knowing from whom
     * the funds comes from, which is not possible if the spender called `transfer` towards the contract
     * (no callback).
     *
     * @param spenderAddress Address allowed to spend funds from the caller.
     * @param netuid The approved subnet token.
     * @param increaseAmount How much the approval amount should be increased.
     */
    function increaseAllowance(
        address spenderAddress,
        uint256 netuid,
        uint256 increaseAmount
    ) external;

    /**
     * @dev Decrease how much the caller approves the spender to use the provided amount of subnet tokens
     * on its behalf in a later call.
     *
     * This is similar to ERC20 decreaseAllowance, and then allows smart contract to transfer with permission from
     * other accounts during their execution. They can then act as escrows while knowing from whom
     * the funds comes from, which is not possible if the spender called `transfer` towards the contract
     * (no callback).
     *
     * @param spenderAddress Address allowed to spend funds from the caller.
     * @param netuid The approved subnet token.
     * @param decreaseAmount How much the approval amount should be decreased.
     */
    function decreaseAllowance(
        address spenderAddress,
        uint256 netuid,
        uint256 decreaseAmount
    ) external;

    /**
     * @dev Transfer a subtensor stake `amount` associated with the `sourceAddress` to a different
     * destination address. The `sourceAddress` must have approved beforehand the transaction signer
     * (spender) to spend at least the `amount` (allowance). The allowance towards that spender will be
     * decreased by this amount.
     *
     * This function allows external accounts and contracts to transfer staked TAO to another EVM
     * address, which effectively calls `transfer_stake` on the subtensor pallet. Both the source and
     * destination EVM addresses are converted to their Substrate ss58 representation using Frontier
     * HashedAddressMapping:
     * https://github.com/polkadot-evm/frontier/blob/2e219e17a526125da003e64ef22ec037917083fa/frame/evm/src/lib.rs#L739
     *
     * @param sourceAddress The source address (20 bytes).
     * @param destinationAddress The destination EVM address (20 bytes).
     * @param hotkey The hotkey public key (32 bytes).
     * @param originNetuid The subnet to move stake from (uint256).
     * @param destinationNetuid The subnet to move stake to (uint256).
     * @param amount The amount to move in rao.
     *
     * Requirements:
     * - `origin_hotkey` and `destination_hotkey` must be valid hotkeys registered on the network, ensuring
     * that the stake is correctly attributed.
     */
    function transferStakeFrom(
        address sourceAddress,
        address destinationAddress,
        bytes32 hotkey,
        uint256 originNetuid,
        uint256 destinationNetuid,
        uint256 amount
    ) external;

    function decreaseTake(bytes32 hotkey, uint16 take) external;
    function increaseTake(bytes32 hotkey, uint16 take) external;
    function setChildkeyTake(bytes32 hotkey, uint16 netuid, uint16 take) external;
    function unstakeAll(bytes32 hotkey) external;
    function unstakeAllAlpha(bytes32 hotkey) external;
    function swapStake(
        bytes32 hotkey,
        uint16 originNetuid,
        uint16 destinationNetuid,
        uint64 alphaAmount
    ) external;
    function swapStakeLimit(
        bytes32 hotkey,
        uint16 originNetuid,
        uint16 destinationNetuid,
        uint64 alphaAmount,
        uint64 limitPrice,
        bool allowPartial
    ) external;
    /**
     * @notice Moves alpha between hotkeys and subnets subject to a relative price limit.
     * @param originHotkey Hotkey from which alpha is removed.
     * @param destinationHotkey Hotkey to which the resulting alpha is credited.
     * @param originNetuid Subnet from which alpha is removed.
     * @param destinationNetuid Subnet into which TAO is staked.
     * @param alphaAmount Origin alpha requested to move, in raw alpha units.
     * @param limitPrice Minimum destination-alpha per origin-alpha ratio, scaled by 1e9.
     * @param allowPartial Whether to execute only the amount available before the limit.
     */
    function moveStakeLimit(
        bytes32 originHotkey,
        bytes32 destinationHotkey,
        uint16 originNetuid,
        uint16 destinationNetuid,
        uint64 alphaAmount,
        uint64 limitPrice,
        bool allowPartial
    ) external;
    function recycleAlpha(bytes32 hotkey, uint64 amount, uint16 netuid) external;
    function setColdkeyAutoStakeHotkey(uint16 netuid, bytes32 hotkey) external;
    function claimRoot(uint16[] calldata subnets) external;
    function claimRootWithHotkey(bytes32 hotkey) external;
    function setRootClaimThreshold(uint16 netuid, uint64 threshold) external;
    function addStakeBurn(
        bytes32 hotkey,
        uint16 netuid,
        uint64 amount,
        bool hasLimit,
        uint64 limit
    ) external;
    function setAutoParentDelegationEnabled(
        bytes32 hotkey,
        bool enabled
    ) external;
    function transferStakeAndHotkey(
        bytes32 destinationColdkey,
        bytes32 originHotkey,
        bytes32 destinationHotkey,
        uint16 originNetuid,
        uint16 destinationNetuid,
        uint64 alphaAmount
    ) external;
    function addCollateral(
        uint16 netuid,
        bytes32 hotkey,
        uint64 alpha,
        uint64 limitPrice
    ) external;
    function setMinCollateral(
        uint16 netuid,
        bytes32 hotkey,
        uint64 minLocked
    ) external;
    function setMinChildkeyTakePerSubnet(uint16 netuid, uint16 take) external;
    function setCollateralLockShare(uint16 netuid, uint16 lockShare) external;
    /// Raw U64F64 bits.
    function setCollateralDrainRatio(
        uint16 netuid,
        uint128 rawRatio
    ) external;

    struct KeyLink {
        uint64 proportion;
        bytes32 account;
    }

    function getDelegate(bytes32 hotkey) external view returns (bool exists, uint16 take);
    function getChildkeyTake(bytes32 hotkey, uint16 netuid) external view returns (uint16);
    function getPendingChildKeys(
        bytes32 parent,
        uint16 netuid
    ) external view returns (KeyLink[] memory children, uint64 cooldownBlock);
    function getChildKeys(
        bytes32 parent,
        uint16 netuid
    ) external view returns (KeyLink[] memory);
    function getParentKeys(
        bytes32 child,
        uint16 netuid
    ) external view returns (KeyLink[] memory);
    function getPendingChildKeyCooldown() external view returns (uint64);
    function getTakeLimits()
        external
        view
        returns (
            uint16 minDelegateTake,
            uint16 maxDelegateTake,
            uint16 minChildkeyTake,
            uint16 maxChildkeyTake
        );
    function getMinChildkeyTakePerSubnet(uint16 netuid) external view returns (uint16);
    function getHotkeyOwner(bytes32 hotkey) external view returns (bool exists, bytes32 owner);
    function getOwnedHotkeys(bytes32 coldkey) external view returns (bytes32[] memory);

    /**
     * @dev Returns at most 64 staking hotkeys beginning at `offset`.
     * Each call reads the existing staking-hotkey vector and slices it in stored order.
     * `total` is the vector length; an offset greater than or equal to `total` returns an empty
     * page.
     */
    function getStakingHotkeys(
        bytes32 coldkey,
        uint64 offset,
        uint16 limit
    ) external view returns (bytes32[] memory hotkeys, uint64 total);

    function getAutoStakeDestination(
        bytes32 coldkey,
        uint16 netuid
    ) external view returns (bool exists, bytes32 hotkey);
    function getAutoStakeDestinationColdkeys(
        bytes32 hotkey,
        uint16 netuid
    ) external view returns (bytes32[] memory);
    function getHotkeySuccessor(
        bytes32 hotkey,
        uint16 netuid
    ) external view returns (bool exists, bytes32 successor);
    function getHotkeyRoot(
        bytes32 hotkey,
        uint16 netuid
    ) external view returns (bool exists, bytes32 root);
    function getColdkeySuccessor(
        bytes32 coldkey
    ) external view returns (bool exists, bytes32 successor);
    function getColdkeyRoot(
        bytes32 coldkey
    ) external view returns (bool exists, bytes32 root);
    function getColdkeySwapStatus(
        bytes32 coldkey
    )
        external
        view
        returns (
            bool hasAnnouncement,
            uint64 announcementBlock,
            bytes32 callHash,
            bool hasDispute,
            uint64 disputeBlock
        );
    function getColdkeySwapDelays()
        external
        view
        returns (uint64 announcementDelay, uint64 reannouncementDelay);
    function getLastHotkeySwapOnSubnet(
        bytes32 coldkey,
        uint16 netuid
    ) external view returns (uint64);
    function getStakeAccounting()
        external
        view
        returns (uint64 totalIssuance, uint64 totalStake);
    function getMinerCollateral(
        uint16 netuid,
        bytes32 hotkey,
        bytes32 coldkey
    )
        external
        view
        returns (
            bool exists,
            uint64 locked,
            uint128 drainRatio,
            uint64 minLocked,
            uint64 earned
        );
    function getColdkeyCollateral(
        uint16 netuid,
        bytes32 coldkey
    ) external view returns (uint64 locked, bytes32[] memory hotkeys);
    function getCollateralConfig(
        uint16 netuid
    ) external view returns (uint16 lockShare, uint128 drainRatio);
    /// Realizable TAO owed by one validator basket, in 18-decimal EVM units.
    function getUnclaimedRootTaoByHotkey(
        bytes32 coldkey,
        bytes32 hotkey
    ) external view returns (uint256 taoValue);
    /// A bounded subtotal for a coldkey's owed value on a subnet.
    function getUnclaimedRootTaoBySubnet(
        bytes32 coldkey,
        uint16 netuid,
        bytes32[] calldata hotkeys
    ) external view returns (uint256 taoValue);
}
