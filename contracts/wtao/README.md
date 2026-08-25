# WTAO Contract

## Description

The WTAO contract is an ERC-20 token implementation that wraps the native TAO token, deployed on the Subtensor mainnet. It allows users to deposit TAO into the contract and mint an equivalent amount of wrapped TAO tokens (WTAO), which can be used as a tradable asset on the blockchain. The contract enables users to withdraw TAO by burning the corresponding amount of WTAO tokens, with the transaction being recorded via Deposit and Withdrawal events. The contract tracks its TAO balance, which accumulates from user deposits, and allows external users to check this balance through the getContractETHBalance function. The contract initializes with the name "Wrapped TAO" and the symbol "WTAO", and it follows the ERC-20 token standard, providing a seamless mechanism for wrapping and unwrapping TAO tokens. Notably, it does not collect any additional fees and is neither owned nor controlled by any party, ensuring a decentralized and trustless operation.

## Deployment info

### Deployed contract address

```
0x312bD9165550C964a37ca33d6d55779d953eC566
```

### Deployer address

H160:
0x6ce97D4154A0FD893ED6FB6aB5A7B20F98340DE2

ss58 mirror:
5GQ9R3hCR2DDZSk2ytYppNhkjfaFtexzt6SmxPG7wrfSGENN

## Running tests locally

```
yarn install
yarn run test
```
