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
    function getProxyDeposit(bytes32 account) external view returns (uint256);
    struct AnnouncementInfo {
        bytes32 real;
        bytes32 callHash;
        uint64 height;
    }
    function getAnnouncements(
        bytes32 account
    ) external view returns (AnnouncementInfo[] memory, uint256 deposit);
    function getLastCallResult(
        bytes32 account
    )
        external
        view
        returns (
            bool exists,
            bool succeeded,
            uint8 errorKind,
            uint8 palletIndex,
            bytes32 errorData
        );
    function isRealPaysFee(
        bytes32 real,
        bytes32 delegate
    ) external view returns (bool);
}
