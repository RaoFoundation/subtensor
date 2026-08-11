pragma solidity ^0.8.0;

address constant INeuron_ADDRESS = 0x0000000000000000000000000000000000000804;

interface INeuron {
    struct WeightPair {
        uint16 uid;
        uint16 value;
    }

    /**
     * @dev Registers a neuron by calling `do_burned_registration` internally with the origin set to the ss58 mirror of the H160 address.
     * This allows the H160 to further call neuron-related methods and receive emissions.
     *
     * @param netuid The subnet to register the neuron to (uint16).
     * @param hotkey The hotkey public key (32 bytes).
     */
    function burnedRegister(uint16 netuid, bytes32 hotkey) external payable;

    /**
     * @dev Registers a neuron like `burnedRegister`, but only if the current
     * burn cost does not exceed `limitPrice` (in rao).
     *
     * @param netuid The subnet to register the neuron to (uint16).
     * @param hotkey The hotkey public key (32 bytes).
     * @param limitPrice The maximum acceptable burn cost in rao (uint64).
     */
    function registerLimit(
        uint16 netuid,
        bytes32 hotkey,
        uint64 limitPrice
    ) external payable;

    /**
     * @dev Registers axon information for a neuron.
     * This function is used to serve axon information, including the subnet to register to, version, IP address, port, IP type, protocol, and placeholders for future use.
     *
     * @param netuid The subnet to register the axon to (uint16).
     * @param version The version of the axon (uint32).
     * @param ip The IP address of the axon (uint128).
     * @param port The port number of the axon (uint16).
     * @param ipType The type of IP address (uint8).
     * @param protocol The protocol used by the axon (uint8).
     * @param placeholder1 Placeholder for future use (uint8).
     * @param placeholder2 Placeholder for future use (uint8).
     */
    function serveAxon(
        uint16 netuid,
        uint32 version,
        uint128 ip,
        uint16 port,
        uint8 ipType,
        uint8 protocol,
        uint8 placeholder1,
        uint8 placeholder2
    ) external payable;

    /**
     * @dev Serves axon information for a neuron over TLS.
     * This function is used to serve axon information, including the subnet to register to, version, IP address, port, IP type, protocol, and placeholders for future use.
     *
     * @param netuid The subnet to register the axon to (uint16).
     * @param version The version of the axon (uint32).
     * @param ip The IP address of the axon (uint128).
     * @param port The port number of the axon (uint16).
     * @param ipType The type of IP address (uint8).
     * @param protocol The protocol used by the axon (uint8).
     * @param placeholder1 Placeholder for future use (uint8).
     * @param placeholder2 Placeholder for future use (uint8).
     * @param certificate The TLS certificate for the axon (bytes).
     */
    function serveAxonTls(
        uint16 netuid,
        uint32 version,
        uint128 ip,
        uint16 port,
        uint8 ipType,
        uint8 protocol,
        uint8 placeholder1,
        uint8 placeholder2,
        bytes memory certificate
    ) external payable;

    /**
     * @dev Serves Prometheus information for a neuron.
     * This function is used to serve Prometheus information, including the subnet to register to, version, IP address, port, and IP type.
     *
     * @param netuid The subnet to register the Prometheus information to (uint16).
     * @param version The version of the Prometheus information (uint32).
     * @param ip The IP address of the Prometheus information (uint128).
     * @param port The port number of the Prometheus information (uint16).
     * @param ipType The type of IP address (uint8).
     */
    function servePrometheus(
        uint16 netuid,
        uint32 version,
        uint128 ip,
        uint16 port,
        uint8 ipType
    ) external payable;

    /**
     * @dev Sets the weights for a neuron.
     *
     * @param netuid The subnet to set the weights for (uint16).
     * @param dests The destinations of the weights (uint16[]).
     * @param weights The weights to set (uint16[]).
     * @param versionKey The version key for the weights (uint64).
     */
    function setWeights(
        uint16 netuid,
        uint16[] memory dests,
        uint16[] memory weights,
        uint64 versionKey
    ) external payable;

    /**
     * @dev Commits the weights for a neuron.
     *
     * @param netuid The subnet to commit the weights for (uint16).
     * @param commitHash The commit hash for the weights (bytes32).
     */
    function commitWeights(uint16 netuid, bytes32 commitHash) external payable;

    /**
     * @dev Reveals the weights for a neuron.
     *
     * @param netuid The subnet to reveal the weights for (uint16).
     * @param uids The unique identifiers for the weights (uint16[]).
     * @param values The values of the weights (uint16[]).
     * @param salt The salt values for the weights (uint16[]).
     * @param versionKey The version key for the weights (uint64).
     */
    function revealWeights(
        uint16 netuid,
        uint16[] memory uids,
        uint16[] memory values,
        uint16[] memory salt,
        uint64 versionKey
    ) external payable;

    function setMechanismWeights(
        uint16 netuid,
        uint8 mecid,
        uint16[] calldata dests,
        uint16[] calldata weights,
        uint64 versionKey
    ) external;
    function batchSetWeights(
        uint16[] calldata netuids,
        uint16[][] calldata dests,
        uint16[][] calldata values,
        uint64[] calldata versionKeys
    ) external;
    function commitMechanismWeights(
        uint16 netuid,
        uint8 mecid,
        bytes32 commitHash
    ) external;
    function batchCommitWeights(
        uint16[] calldata netuids,
        bytes32[] calldata commitHashes
    ) external;
    function revealMechanismWeights(
        uint16 netuid,
        uint8 mecid,
        uint16[] calldata uids,
        uint16[] calldata values,
        uint16[] calldata salt,
        uint64 versionKey
    ) external;
    function commitCrv3MechanismWeights(
        uint16 netuid,
        uint8 mecid,
        bytes calldata commit,
        uint64 revealRound
    ) external;
    function batchRevealWeights(
        uint16 netuid,
        uint16[][] calldata uids,
        uint16[][] calldata values,
        uint16[][] calldata salts,
        uint64[] calldata versionKeys
    ) external;
    function commitTimelockedWeights(
        uint16 netuid,
        bytes calldata commit,
        uint64 revealRound,
        uint16 commitRevealVersion
    ) external;
    function commitTimelockedMechanismWeights(
        uint16 netuid,
        uint8 mecid,
        bytes calldata commit,
        uint64 revealRound,
        uint16 commitRevealVersion
    ) external;
    function register(
        uint16 netuid,
        uint64 blockNumber,
        uint64 nonce,
        bytes calldata work,
        bytes32 hotkey,
        bytes32 coldkey
    ) external;
    function rootRegister(bytes32 hotkey) external;
    function swapHotkey(
        bytes32 hotkey,
        bytes32 newHotkey,
        bool hasNetuid,
        uint16 netuid
    ) external;
    function swapHotkeyV2(
        bytes32 hotkey,
        bytes32 newHotkey,
        bool hasNetuid,
        uint16 netuid,
        bool keepStake
    ) external;
    function setChildren(
        bytes32 hotkey,
        uint16 netuid,
        uint64[] calldata proportions,
        bytes32[] calldata children
    ) external;
    function setIdentity(
        string calldata name,
        string calldata url,
        string calldata githubRepo,
        string calldata image,
        string calldata discord,
        string calldata description,
        string calldata additional
    ) external;
    function tryAssociateHotkey(bytes32 hotkey) external;
    function associateEvmKey(
        uint16 netuid,
        address evmKey,
        uint64 blockNumber,
        bytes calldata signature
    ) external;
    function announceColdkeySwap(bytes32 newColdkeyHash) external;
    function executeAnnouncedColdkeySwap(bytes32 newColdkey) external;
    function disputeColdkeySwap() external;
    function clearColdkeySwapAnnouncement() external;
    function getUid(
        uint16 netuid,
        bytes32 hotkey
    ) external view returns (bool exists, uint16 uid);
    function isNetworkMember(
        bytes32 hotkey,
        uint16 netuid
    ) external view returns (bool);
    function getWeights(
        uint16 netuid,
        uint16 uid
    ) external view returns (WeightPair[] memory);
    function getBonds(
        uint16 netuid,
        uint16 uid
    ) external view returns (WeightPair[] memory);
    function getBlockAtRegistration(
        uint16 netuid,
        uint16 uid
    ) external view returns (uint64);
    function getNeuronCertificate(
        uint16 netuid,
        bytes32 hotkey
    ) external view returns (bool exists, uint8 algorithm, bytes memory publicKey);
    function getPrometheus(
        uint16 netuid,
        bytes32 hotkey
    )
        external
        view
        returns (
            bool exists,
            uint64 blockNumber,
            uint32 version,
            uint128 ip,
            uint16 port,
            uint8 ipType
        );
    function getChainIdentity(
        bytes32 coldkey
    )
        external
        view
        returns (
            bool exists,
            bytes memory name,
            bytes memory url,
            bytes memory githubRepo,
            bytes memory image,
            bytes memory discord,
            bytes memory description,
            bytes memory additional
        );
    function getSubnetIdentity(
        uint16 netuid
    )
        external
        view
        returns (
            bool exists,
            bytes memory subnetName,
            bytes memory githubRepo,
            bytes memory subnetContact,
            bytes memory subnetUrl,
            bytes memory discord,
            bytes memory description,
            bytes memory logoUrl,
            bytes memory additional
        );
    struct LoadedEmission {
        bytes32 hotkey;
        uint64 serverEmission;
        uint64 validatorEmission;
    }
    function getLoadedEmission(
        uint16 netuid
    ) external view returns (bool exists, LoadedEmission[] memory);
    function getTransactionKeyLastBlock(
        bytes32 hotkey,
        uint16 netuid,
        uint16 transactionKey
    ) external view returns (uint64);
    function getLegacyTransactionRateBlocks(
        bytes32 hotkey
    )
        external
        view
        returns (
            uint64 lastTransactionBlock,
            uint64 lastChildkeyTakeBlock,
            uint64 lastDelegateTakeBlock
        );
    function getWeightCommit(
        uint16 netuid,
        bytes32 hotkey,
        uint32 index
    )
        external
        view
        returns (bool exists, bytes32 hash, uint64 epoch, uint64 blockNumber);
    function getWeightCommitCount(
        uint16 netuid,
        bytes32 hotkey
    ) external view returns (uint32);
    function getTimelockedWeightCommit(
        uint16 netuid,
        uint64 epoch,
        uint32 index
    )
        external
        view
        returns (
            bool exists,
            bytes32 hotkey,
            uint64 blockNumber,
            bytes32 ciphertextHash,
            uint32 ciphertextLength,
            uint64 revealRound
        );
    function getTimelockedWeightCommitCount(
        uint16 netuid,
        uint64 epoch
    ) external view returns (uint32);
    function getLegacyTimelockedWeightCommit(
        uint8 version,
        uint16 netuid,
        uint64 epoch,
        uint32 index
    )
        external
        view
        returns (
            bool exists,
            bytes32 hotkey,
            uint64 blockNumber,
            bytes32 ciphertextHash,
            uint32 ciphertextLength,
            uint64 revealRound
        );
    function getLegacyTimelockedWeightCommitCount(
        uint8 version,
        uint16 netuid,
        uint64 epoch
    ) external view returns (uint32);
}
