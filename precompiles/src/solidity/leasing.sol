pragma solidity ^0.8.0;

address constant ILEASING_ADDRESS = 0x000000000000000000000000000000000000080a;

interface ILeasing {
    /**
     * @dev Retrieves the lease info for a given lease id.
     * @param leaseId The id of the lease to get info for.
     * @return The lease info.
     */
    function getLease(uint32 leaseId) external view returns (LeaseInfo memory);

    /**
     * @dev Retrieves the contributor share for a given lease id and contributor.
     * The share is returned as a tuple of two uint128 values, where the first value
     * is the integer part and the second value is the fractional part.
     * @param leaseId The id of the lease to get contributor share for.
     * @param contributor The contributor to get share for.
     * @return The contributor share.
     */
    function getContributorShare(uint32 leaseId, bytes32 contributor)
        external
        view
        returns (uint128, uint128);

    /**
     * @dev Retrieves the lease id for a given subnet.
     * @param netuid The subnet to get lease id for.
     * @return The lease id.
     */
    function getLeaseIdForSubnet(uint16 netuid) external view returns (uint32);

    /**
     * @dev Create a lease crowdloan.
     * @param crowdloanDeposit The deposit from the creator.
     * @param crowdloanMinContribution The minimum contribution required to contribute to the crowdloan.
     * @param crowdloanCap The maximum amount of funds that can be raised.
     * @param crowdloanEnd The block number at which the crowdloan will end.
     * @param leasingEmissionsShare The share of the emissions that the contributors will receive.
     * @param hasLeasingEndBlock Whether the lease has an end block.
     * @param leasingEndBlock The block number at which the lease will end.
     */
    function createLeaseCrowdloan(
        uint64 crowdloanDeposit,
        uint64 crowdloanMinContribution,
        uint64 crowdloanCap,
        uint32 crowdloanEnd,
        uint8 leasingEmissionsShare,
        bool hasLeasingEndBlock,
        uint32 leasingEndBlock
    ) external payable;

    /**
     * @dev Terminates a lease and transfers the ownership to the beneficiary.
     * @param leaseId The id of the lease to terminate.
     * @param hotkey The hotkey of beneficiary, it must be owned by the beneficiary coldkey.
     */
    function terminateLease(uint32 leaseId, bytes32 hotkey) external payable;
}

struct LeaseInfo {
    bytes32 beneficiary;
    bytes32 coldkey;
    bytes32 hotkey;
    uint8 emissions_share;
    bool has_end_block;
    uint32 end_block;
    uint16 netuid;
    uint64 cost;
}
