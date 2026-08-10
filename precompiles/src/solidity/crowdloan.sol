pragma solidity ^0.8.0;

address constant ICROWDLOAN_ADDRESS = 0x0000000000000000000000000000000000000809;

interface ICrowdloan {
    /**
     * @dev Retrieves the crowdloan info for a given crowdloan id.
     * @param crowdloanId The id of the crowdloan to get info for.
     * @return The crowdloan info.
     */
    function getCrowdloan(uint32 crowdloanId)
        external
        view
        returns (CrowdloanInfo memory);

    /**
     * @dev Retrieves the contribution for a given crowdloan id and coldkey.
     * @param crowdloanId The id of the crowdloan to get contribution for.
     * @param coldkey The coldkey of the contributor.
     * @return The contribution.
     */
    function getContribution(uint32 crowdloanId, bytes32 coldkey)
        external
        view
        returns (uint64);

    /**
     * @dev Creates a crowdloan that will raise funds up to a maximum cap and if successful, transfer the funds to the target address.
     * @param deposit The initial deposit from the creator.
     * @param minContribution The minimum contribution required to contribute to the crowdloan.
     * @param cap The maximum amount of funds that can be raised.
     * @param end The block number at which the crowdloan will end.
     * @param targetAddress The address to transfer the raised funds to.
     */
    function create(
        uint64 deposit,
        uint64 minContribution,
        uint64 cap,
        uint32 end,
        address targetAddress
    ) external payable;

    /**
     * @dev Contributes to an active crowdloan.
     * @param crowdloanId The id of the crowdloan to contribute to.
     * @param amount The amount of funds to contribute.
     */
    function contribute(uint32 crowdloanId, uint64 amount) external payable;

    /**
     * @dev Withdraws a contribution from an active crowdloan.
     * @param crowdloanId The id of the crowdloan to withdraw from.
     */
    function withdraw(uint32 crowdloanId) external payable;

    /**
     * @dev Finalizes a successful crowdloan.
     * @param crowdloanId The id of the crowdloan to finalize.
     */
    function finalize(uint32 crowdloanId) external payable;

    /**
     * @dev Refunds a failed crowdloan (may need to be called multiple times to refund all contributors).
     * @param crowdloanId The id of the crowdloan to refund.
     */
    function refund(uint32 crowdloanId) external payable;

    /**
     * @dev Dissolves a failed crowdloan.
     * @param crowdloanId The id of the crowdloan to dissolve.
     */
    function dissolve(uint32 crowdloanId) external payable;

    /**
     * @dev Updates the minimum contribution for an active crowdloan.
     * @param crowdloanId The id of the crowdloan to update.
     * @param newMinContribution The new minimum contribution.
     */
    function updateMinContribution(
        uint32 crowdloanId,
        uint64 newMinContribution
    ) external payable;

    /**
     * @dev Updates the end block for an active crowdloan.
     * @param crowdloanId The id of the crowdloan to update.
     * @param newEnd The new end block.
     */
    function updateEnd(uint32 crowdloanId, uint32 newEnd) external payable;

    /**
     * @dev Updates the cap for an active crowdloan.
     * @param crowdloanId The id of the crowdloan to update.
     * @param newCap The new cap.
     */
    function updateCap(uint32 crowdloanId, uint64 newCap) external payable;
    function setMaxContribution(
        uint32 crowdloanId,
        bool hasMaxContribution,
        uint64 maxContribution
    ) external;
}

struct CrowdloanInfo {
    bytes32 creator;
    uint64 deposit;
    uint64 min_contribution;
    uint32 end;
    uint64 cap;
    bytes32 funds_account;
    uint64 raised;
    bool has_target_address;
    bytes32 target_address;
    bool finalized;
    uint32 contributors_count;
}
