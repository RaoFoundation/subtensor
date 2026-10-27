pragma solidity ^0.8.0;

address constant ISUBTENSOR_BALANCE_TRANSFER_ADDRESS = 0x0000000000000000000000000000000000000800;

interface ISubtensorBalanceTransfer {
    function transfer(bytes32 data) external payable;
    function transferKeepAlive(bytes32 destination, uint256 amount) external;
    function transferAll(bytes32 destination, bool keepAlive) external;
}
