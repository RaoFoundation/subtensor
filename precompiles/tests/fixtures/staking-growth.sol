// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.21;

interface IStakingGrowth {
    function setRejectStakingHotkeyGrowth(bool enabled) external;
    function getRejectStakingHotkeyGrowth(bytes32 coldkey) external view returns (bool);
    function transferStake(bytes32 coldkey, bytes32 hotkey, uint256 originNetuid, uint256 destinationNetuid, uint256 amount) external;
}

// Compiled by generate-staking-growth.mjs with the repository's solc version.
contract StakingGrowthFixture {
    IStakingGrowth constant staking = IStakingGrowth(address(0x0805));

    function setAndGet(bool enabled, bytes32 coldkey) external returns (bool) {
        staking.setRejectStakingHotkeyGrowth(enabled);
        return staking.getRejectStakingHotkeyGrowth(coldkey);
    }

    function forwardStake(bytes32 receiver, bytes32 hotkey, uint256 netuid, uint256 amount) external {
        staking.transferStake(receiver, hotkey, netuid, netuid, amount);
    }
}
