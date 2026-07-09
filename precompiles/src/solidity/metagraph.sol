pragma solidity ^0.8.0;

address constant IMetagraph_ADDRESS = 0x0000000000000000000000000000000000000802;

struct AxonInfo {
  uint64 block;
  uint32 version;
  uint128 ip;
  uint16 port;
  uint8 ip_type;
  uint8 protocol;
}

interface IMetagraph {

  /**
   * @dev Returns the count of unique identifiers (UIDs) associated with a given network identifier (netuid).
   * @param netuid The network identifier for which to retrieve the UID count.
   * @return The count of UIDs associated with the specified netuid.
   */
  function getUidCount(uint16 netuid) external view returns (uint16);

  /**
   * @dev Retrieves the stake amount associated with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the stake.
   * @param uid The unique identifier for which to retrieve the stake.
   * @return The stake amount associated with the specified netuid and uid.
   */
  function getStake(uint16 netuid, uint16 uid) external view returns (uint64);

  /**
   * @dev Retrieves the rank of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the rank.
   * @param uid The unique identifier for which to retrieve the rank.
   * @return The rank of the node with the specified netuid and uid.
   */
  function getRank(uint16 netuid, uint16 uid) external view returns (uint16);

  /**
   * @dev Retrieves the trust value of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the trust value.
   * @param uid The unique identifier for which to retrieve the trust value.
   * @return The trust value of the node with the specified netuid and uid.
   */
  function getTrust(uint16 netuid, uint16 uid) external view returns (uint16);

  /**
   * @dev Retrieves the consensus value of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the consensus value.
   * @param uid The unique identifier for which to retrieve the consensus value.
   * @return The consensus value of the node with the specified netuid and uid.
   */
  function getConsensus(uint16 netuid, uint16 uid) external view returns (uint16);

  /**
   * @dev Retrieves the incentive value of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the incentive value.
   * @param uid The unique identifier for which to retrieve the incentive value.
   * @return The incentive value of the node with the specified netuid and uid.
   */
  function getIncentive(uint16 netuid, uint16 uid) external view returns (uint16);

  /**
   * @dev Retrieves the dividend value of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the dividend value.
   * @param uid The unique identifier for which to retrieve the dividend value.
   * @return The dividend value of the node with the specified netuid and uid.
   */
  function getDividends(uint16 netuid, uint16 uid) external view returns (uint16);

  /**
   * @dev Retrieves the emission value of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the emission value.
   * @param uid The unique identifier for which to retrieve the emission value.
   * @return The emission value of the node with the specified netuid and uid.
   */
  function getEmission(uint16 netuid, uint16 uid) external view returns (uint64);

  /**
   * @dev Retrieves the v-trust value of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the v-trust value.
   * @param uid The unique identifier for which to retrieve the v-trust value.
   * @return The v-trust value of the node with the specified netuid and uid.
   */
  function getVtrust(uint16 netuid, uint16 uid) external view returns (uint16);

  /**
   * @dev Checks the validator status of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to check the validator status.
   * @param uid The unique identifier for which to check the validator status.
   * @return Returns true if the node is a validator, false otherwise.
   */
  function getValidatorStatus(uint16 netuid, uint16 uid) external view returns (bool);

  /**
   * @dev Retrieves the last update timestamp of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the last update timestamp.
   * @param uid The unique identifier for which to retrieve the last update timestamp.
   * @return The last update timestamp of the node with the specified netuid and uid.
   */
  function getLastUpdate(uint16 netuid, uint16 uid) external view returns (uint64);

  /**
   * @dev Checks if a node with a given network identifier (netuid) and unique identifier (uid) is active.
   * @param netuid The network identifier for which to check the node's activity.
   * @param uid The unique identifier for which to check the node's activity.
   * @return Returns true if the node is active, false otherwise.
   */
  function getIsActive(uint16 netuid, uint16 uid) external view returns (bool);

  /**
   * @dev Retrieves the axon information of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the axon information.
   * @param uid The unique identifier for which to retrieve the axon information.
   * @return The axon information of the node with the specified netuid and uid.
   */
  function getAxon(uint16 netuid, uint16 uid) external view returns (AxonInfo memory);

  /**
   * @dev Retrieves the hotkey of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the hotkey.
   * @param uid The unique identifier for which to retrieve the hotkey.
   * @return The hotkey of the node with the specified netuid and uid.
   */
  function getHotkey(uint16 netuid, uint16 uid) external view returns (bytes32);

  /**
   * @dev Retrieves the coldkey of a node with a given network identifier (netuid) and unique identifier (uid).
   * @param netuid The network identifier for which to retrieve the coldkey.
   * @param uid The unique identifier for which to retrieve the coldkey.
   * @return The coldkey of the node with the specified netuid and uid.
   */
  function getColdkey(uint16 netuid, uint16 uid) external view returns (bytes32);
}
