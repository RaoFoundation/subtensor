pragma solidity ^0.8.0;

address constant INeuron_ADDRESS = 0x0000000000000000000000000000000000000804;

interface INeuron {
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
}
