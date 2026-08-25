import { HardhatUserConfig } from "hardhat/config";
import "@nomicfoundation/hardhat-toolbox";
import "@nomicfoundation/hardhat-verify";
import { config } from "./config";

const hardhatConfig: HardhatUserConfig = {
  solidity: "0.8.24",
  defaultNetwork: "local",
  networks: {
    mainnet: {
      url: "https://entrypoint-finney.opentensor.ai",
      accounts: [config.ethPrivateKey],
    },
    subevm: {
      url: "https://test.chain.opentensor.ai",
      accounts: [config.ethPrivateKey],
    },
    local: {
      url: "http://127.0.0.1:9944",
      accounts: [config.ethPrivateKey],
    },
  },
  mocha: {
    timeout: 300000,
  },
};

export default hardhatConfig;
