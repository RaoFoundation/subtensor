import { expect } from "chai";
import { ethers } from "hardhat";
import { HardhatEthersProvider } from "@nomicfoundation/hardhat-ethers/internal/hardhat-ethers-provider";
import { WTAO } from "../typechain-types/contracts/WTAO";
import BigNumber from 'bignumber.js';
BigNumber.config({ DECIMAL_PLACES: 0, ROUNDING_MODE: BigNumber.ROUND_DOWN, EXPONENTIAL_AT: 255 });

describe("WTAO contract", function () {
  let sutContract: WTAO;
  let gasPrice: BigNumber;
  let provider: HardhatEthersProvider;
  let caller: any;
  let gasParameters: {
    gasLimit: ethers.BigNumber,
    gasPrice: ethers.BigNumber,
  } = {
    gasLimit: 2000000,
    gasPrice: ethers.parseUnits("20", "gwei"),
  };

  before(async () => {
    provider = ethers.provider;
    [caller] = await ethers.getSigners();
    sutContract = await ethers.deployContract("WTAO", { ...gasParameters});
    await sutContract.waitForDeployment();
    console.log(`Contract deployed at address: ${await sutContract.getAddress()}`);
  });

  it("Deposit TAO", async () => {
    const balanceBefore = await provider.getBalance(caller.address);
    const tx = await sutContract.deposit({ ...gasParameters, value: ethers.parseEther("0.1") });

    // Wait for the transaction to be mined
    await tx.wait();

    // Check user WTAO balance
    const wtaoBalance = await sutContract.balanceOf(caller);
    expect(wtaoBalance).to.be.eq(ethers.parseEther("0.1"));

    // Check that TAO is stored in the contract (less 500 rao existential deposit 
    // that we don't see on the EVM side)
    const tvlBalance = await sutContract.getContractETHBalance();
    expect(tvlBalance).to.be.eq(ethers.parseEther("0.1") - 500_000_000_000n);

    // Check that user balance decreased
    const balanceAfter = await provider.getBalance(caller.address);
    expect(balanceBefore - balanceAfter).to.be.gte(ethers.parseEther("0.1"));
  });

  it("Withdraw TAO", async () => {
    const balanceBefore = await provider.getBalance(caller.address);
    const tx = await sutContract.withdraw(ethers.parseEther("0.1"), { ...gasParameters});

    // Wait for the transaction to be mined
    await tx.wait();

    // Check user WTAO balance
    const wtaoBalance = await sutContract.balanceOf(caller);
    expect(wtaoBalance).to.be.eq(0);

    // Check that TAO is stored in the contract
    const tvlBalance = await sutContract.getContractETHBalance();
    expect(tvlBalance).to.be.eq(0);

    // Check that user balance increased
    const balanceAfter = await provider.getBalance(caller.address);
    expect(balanceAfter - balanceBefore).to.be.gte(0);
  });
});
