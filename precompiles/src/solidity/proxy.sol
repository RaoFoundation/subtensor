// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant IPROXY_ADDRESS = 0x000000000000000000000000000000000000080b;

interface IProxy {
    function createPureProxy(
        uint8 proxy_type,
        uint32 delay,
        uint16 index
    ) external;

    function proxyCall(
        bytes32 real,
        uint8[] memory force_proxy_type,
        uint8[] memory call
    ) external;

    function killPureProxy(
        bytes32 spawner,
        uint8 proxy_type,
        uint16 index,
        uint32 height,
        uint32 ext_index
    ) external;

    function addProxy(
        bytes32 delegate,
        uint8 proxy_type,
        uint32 delay
    ) external;

    function removeProxy(
        bytes32 delegate,
        uint8 proxy_type,
        uint32 delay
    ) external;

    function removeProxies() external;

    function pokeDeposit() external;

    struct ProxyInfo {
        bytes32 delegate;
        uint256 proxy_type;
        uint256 delay;
    }

    function getProxies(
        bytes32 account
    ) external view returns (ProxyInfo[] memory);

    function announce(bytes32 real, bytes32 callHash) external;
    function removeAnnouncement(bytes32 real, bytes32 callHash) external;
    function rejectAnnouncement(bytes32 delegate, bytes32 callHash) external;
    function setRealPaysFee(bytes32 delegate, bool paysFee) external;
}
