import { ethers } from "hardhat";

async function main() {
  const [deployer] = await ethers.getSigners();
  const wtao = await ethers.deployContract("WTAO", { gasLimit: 2000000, from: deployer.address });
  await wtao.waitForDeployment();

  // Make an initial deposit to cover ED
  await wtao.deposit({ gasLimit: 2000000, from: deployer.address, value: ethers.parseEther("0.0001") });

  console.log(
    `WTAO contract deployed to ${wtao.target}`
  );
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
