export const ISTAKING_ADDRESS = "0x0000000000000000000000000000000000000801";
export const ISTAKING_V2_ADDRESS = "0x0000000000000000000000000000000000000805";
export const IPROXY_ADDRESS = "0x000000000000000000000000000000000000080b";

export const IStakingABI = [
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "delegate",
                type: "bytes32",
            },
        ],
        name: "addProxy",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "addStake",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "delegate",
                type: "bytes32",
            },
        ],
        name: "removeProxy",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "bytes32",
                name: "coldkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "getStake",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "removeStake",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
];

export const IStakingV2ABI = [
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "delegate",
                type: "bytes32",
            },
        ],
        name: "addProxy",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "addStake",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "getAlphaStakedValidators",
        outputs: [
            {
                internalType: "uint256[]",
                name: "",
                type: "uint256[]",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "bytes32",
                name: "coldkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "getStake",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "getTotalAlphaStaked",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "coldkey",
                type: "bytes32",
            },
        ],
        name: "getTotalColdkeyStake",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "coldkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "getTotalColdkeyStakeOnSubnet",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
        ],
        name: "getTotalHotkeyStake",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "getNominatorMinRequiredStake",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "delegate",
                type: "bytes32",
            },
        ],
        name: "removeProxy",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "removeStake",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "limit_price",
                type: "uint256",
            },
            {
                internalType: "bool",
                name: "allow_partial",
                type: "bool",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "addStakeLimit",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "limit_price",
                type: "uint256",
            },
            {
                internalType: "bool",
                name: "allow_partial",
                type: "bool",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "removeStakeLimit",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "removeStakeFull",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "limit_price",
                type: "uint256",
            },
        ],
        name: "removeStakeFullLimit",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "burnAlpha",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "spenderAddress",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "absoluteAmount",
                type: "uint256",
            },
        ],
        name: "approve",
        outputs: [],
        stateMutability: "",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "sourceAddress",
                type: "address",
            },
            {
                internalType: "address",
                name: "spenderAddress",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "allowance",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "spenderAddress",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "increaseAmount",
                type: "uint256",
            },
        ],
        name: "increaseAllowance",
        outputs: [],
        stateMutability: "",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "spenderAddress",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "decreaseAmount",
                type: "uint256",
            },
        ],
        name: "decreaseAllowance",
        outputs: [],
        stateMutability: "",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "sourceAddress",
                type: "address",
            },
            {
                internalType: "address",
                name: "destinationAddress",
                type: "address",
            },
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "originNetuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "destinationNetuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
        ],
        name: "transferStakeFrom",
        outputs: [],
        stateMutability: "",
        type: "function",
    },
];

export const IProxyABI = [
    {
        inputs: [
            {
                internalType: "uint8",
                name: "proxy_type",
                type: "uint8",
            },
            {
                internalType: "uint32",
                name: "delay",
                type: "uint32",
            },
            {
                internalType: "uint16",
                name: "index",
                type: "uint16",
            },
        ],
        name: "createPureProxy",
        outputs: [
            {
                internalType: "bytes32",
                name: "proxy",
                type: "bytes32",
            },
        ],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "spawner",
                type: "bytes32",
            },
            {
                internalType: "uint8",
                name: "proxy_type",
                type: "uint8",
            },
            {
                internalType: "uint16",
                name: "index",
                type: "uint16",
            },
            {
                internalType: "uint32",
                name: "height",
                type: "uint32",
            },
            {
                internalType: "uint32",
                name: "ext_index",
                type: "uint32",
            },
        ],
        name: "killPureProxy",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "real",
                type: "bytes32",
            },
            {
                internalType: "uint8[]",
                name: "force_proxy_type",
                type: "uint8[]",
            },
            {
                internalType: "uint8[]",
                name: "call",
                type: "uint8[]",
            },
        ],
        name: "proxyCall",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [],
        name: "removeProxies",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [],
        name: "pokeDeposit",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "delegate",
                type: "bytes32",
            },
            {
                internalType: "uint8",
                name: "proxy_type",
                type: "uint8",
            },
            {
                internalType: "uint32",
                name: "delay",
                type: "uint32",
            },
        ],
        name: "removeProxy",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "delegate",
                type: "bytes32",
            },
            {
                internalType: "uint8",
                name: "proxy_type",
                type: "uint8",
            },
            {
                internalType: "uint32",
                name: "delay",
                type: "uint32",
            },
        ],
        name: "addProxy",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "account",
                type: "bytes32",
            },
        ],
        name: "getProxies",
        outputs: [
            {
                components: [
                    {
                        internalType: "bytes32",
                        name: "delegate",
                        type: "bytes32",
                    },
                    {
                        internalType: "uint256",
                        name: "proxy_type",
                        type: "uint256",
                    },
                    {
                        internalType: "uint256",
                        name: "delay",
                        type: "uint256",
                    },
                ],
                internalType: "struct IProxy.ProxyInfo[]",
                name: "",
                type: "tuple[]",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
];

export const IBALANCETRANSFER_ADDRESS = "0x0000000000000000000000000000000000000800";

export const IBalanceTransferABI = [
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "data",
                type: "bytes32",
            },
        ],
        name: "transfer",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
] as const;

export const WITHDRAW_CONTRACT_ABI = [
    {
        inputs: [],
        stateMutability: "nonpayable",
        type: "constructor",
    },
    {
        inputs: [
            {
                internalType: "uint256",
                name: "value",
                type: "uint256",
            },
        ],
        name: "withdraw",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        stateMutability: "payable",
        type: "receive",
    },
] as const;

export const WITHDRAW_CONTRACT_BYTECODE =
    "6080604052348015600e575f80fd5b506101148061001c5f395ff3fe608060405260043610601e575f3560e01c80632e1a7d4d146028576024565b36602457005b5f80fd5b603e6004803603810190603a919060b8565b6040565b005b3373ffffffffffffffffffffffffffffffffffffffff166108fc8290811502906040515f60405180830381858888f193505050501580156082573d5f803e3d5ffd5b5050565b5f80fd5b5f819050919050565b609a81608a565b811460a3575f80fd5b50565b5f8135905060b2816093565b92915050565b5f6020828403121560ca5760c96086565b5b5f60d58482850160a6565b9150509291505056fea2646970667358221220f43400858bfe4fcc0bf3c1e2e06d3a9e6ced86454a00bd7e4866b3d4d64e46bb64736f6c634300081a0033";
export const ALPHA_POOL_CONTRACT_ABI = [
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "_contract_hotkey",
                type: "bytes32",
            },
        ],
        stateMutability: "nonpayable",
        type: "constructor",
    },
    {
        inputs: [],
        name: "ISTAKING_V2_ADDRESS",
        outputs: [
            {
                internalType: "address",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        name: "alphaBalance",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "contract_coldkey",
        outputs: [
            {
                internalType: "bytes32",
                name: "",
                type: "bytes32",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "contract_hotkey",
        outputs: [
            {
                internalType: "bytes32",
                name: "",
                type: "bytes32",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint256",
                name: "_netuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "_alphaAmount",
                type: "uint256",
            },
            {
                internalType: "bytes32",
                name: "_hotkey",
                type: "bytes32",
            },
        ],
        name: "depositAlpha",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "getContractStake",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "_contract_coldkey",
                type: "bytes32",
            },
        ],
        name: "setContractColdkey",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint256",
                name: "_netuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "_alphaAmount",
                type: "uint256",
            },
            {
                internalType: "bytes32",
                name: "_user_coldkey",
                type: "bytes32",
            },
        ],
        name: "withdrawAlpha",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
];

export const ALPHA_POOL_CONTRACT_BYTECODE =
    "6080604052348015600e575f5ffd5b506040516110d83803806110d88339818101604052810190602e9190606c565b80600181905550506092565b5f5ffd5b5f819050919050565b604e81603e565b81146057575f5ffd5b50565b5f815190506066816047565b92915050565b5f60208284031215607e57607d603a565b5b5f608984828501605a565b91505092915050565b6110398061009f5f395ff3fe608060405234801561000f575f5ffd5b5060043610610086575f3560e01c8063cdcde3e911610059578063cdcde3e914610110578063d67c076114610140578063f0d6bb891461015e578063fdbcdce91461017a57610086565b80632849912d1461008a5780633af975ff146100a657806359948a67146100c4578063bee0bca1146100f4575b5f5ffd5b6100a4600480360381019061009f919061090d565b610198565b005b6100ae610518565b6040516100bb919061096c565b60405180910390f35b6100de60048036038101906100d991906109df565b61051d565b6040516100eb9190610a2c565b60405180910390f35b61010e6004803603810190610109919061090d565b61053d565b005b61012a60048036038101906101259190610a45565b610805565b6040516101379190610a2c565b60405180910390f35b61014861088e565b604051610155919061096c565b60405180910390f35b61017860048036038101906101739190610a70565b610894565b005b61018261089d565b60405161018f9190610aaa565b60405180910390f35b5f5f1b5f54036101dd576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016101d490610b1d565b60405180910390fd5b5f6101e784610805565b90505f6317ce5f6260e01b5f548487888860405160240161020c959493929190610b3b565b604051602081830303815290604052907bffffffffffffffffffffffffffffffffffffffffffffffffffffffff19166020820180517bffffffffffffffffffffffffffffffffffffffffffffffffffffffff838183161783525050505090505f61080573ffffffffffffffffffffffffffffffffffffffff165a836040516102949190610bde565b5f604051808303818686f4925050503d805f81146102cd576040519150601f19603f3d011682016040523d82523d5f602084013e6102d2565b606091505b5050905080610316576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161030d90610c3e565b60405180910390fd5b5f61032087610805565b9050838111610364576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161035b90610ccc565b60405180910390fd5b5f84826103719190610d17565b905060015486146104ac57631149f65960e01b866001548a8b8560405160240161039f959493929190610b3b565b604051602081830303815290604052907bffffffffffffffffffffffffffffffffffffffffffffffffffffffff19166020820180517bffffffffffffffffffffffffffffffffffffffffffffffffffffffff8381831617835250505050935061080573ffffffffffffffffffffffffffffffffffffffff165a856040516104269190610bde565b5f604051808303815f8787f1925050503d805f8114610460576040519150601f19603f3d011682016040523d82523d5f602084013e610465565b606091505b505080935050826104ab576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016104a290610dba565b60405180910390fd5b5b8060025f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8a81526020019081526020015f205f8282546105079190610dd8565b925050819055505050505050505050565b5f5481565b6002602052815f5260405f20602052805f5260405f205f91509150505481565b5f5f1b5f5403610582576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161057990610b1d565b60405180910390fd5b8160025f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8581526020019081526020015f20541015610611576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161060890610e7b565b60405180910390fd5b5f61061b84610805565b90508260025f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8681526020019081526020015f205f8282546106789190610d17565b925050819055505f6317ce5f6260e01b836001548788886040516024016106a3959493929190610b3b565b604051602081830303815290604052907bffffffffffffffffffffffffffffffffffffffffffffffffffffffff19166020820180517bffffffffffffffffffffffffffffffffffffffffffffffffffffffff838183161783525050505090505f61080573ffffffffffffffffffffffffffffffffffffffff165a8360405161072b9190610bde565b5f604051808303815f8787f1925050503d805f8114610765576040519150601f19603f3d011682016040523d82523d5f602084013e61076a565b606091505b505090505f61077887610805565b90508381106107bc576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016107b390610f09565b60405180910390fd5b816107fc576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016107f390610f71565b60405180910390fd5b50505050505050565b5f61080573ffffffffffffffffffffffffffffffffffffffff1663e3b598fa6001545f54856040518463ffffffff1660e01b815260040161084893929190610f8f565b602060405180830381865afa158015610863573d5f5f3e3d5ffd5b505050506040513d601f19601f820116820180604052508101906108879190610fd8565b9050919050565b60015481565b805f8190555050565b61080581565b5f5ffd5b5f819050919050565b6108b9816108a7565b81146108c3575f5ffd5b50565b5f813590506108d4816108b0565b92915050565b5f819050919050565b6108ec816108da565b81146108f6575f5ffd5b50565b5f81359050610907816108e3565b92915050565b5f5f5f60608486031215610924576109236108a3565b5b5f610931868287016108c6565b9350506020610942868287016108c6565b9250506040610953868287016108f9565b9150509250925092565b610966816108da565b82525050565b5f60208201905061097f5f83018461095d565b92915050565b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f6109ae82610985565b9050919050565b6109be816109a4565b81146109c8575f5ffd5b50565b5f813590506109d9816109b5565b92915050565b5f5f604083850312156109f5576109f46108a3565b5b5f610a02858286016109cb565b9250506020610a13858286016108c6565b9150509250929050565b610a26816108a7565b82525050565b5f602082019050610a3f5f830184610a1d565b92915050565b5f60208284031215610a5a57610a596108a3565b5b5f610a67848285016108c6565b91505092915050565b5f60208284031215610a8557610a846108a3565b5b5f610a92848285016108f9565b91505092915050565b610aa4816109a4565b82525050565b5f602082019050610abd5f830184610a9b565b92915050565b5f82825260208201905092915050565b7f636f6e747261637420636f6c646b6579206e6f742073657400000000000000005f82015250565b5f610b07601883610ac3565b9150610b1282610ad3565b602082019050919050565b5f6020820190508181035f830152610b3481610afb565b9050919050565b5f60a082019050610b4e5f83018861095d565b610b5b602083018761095d565b610b686040830186610a1d565b610b756060830185610a1d565b610b826080830184610a1d565b9695505050505050565b5f81519050919050565b5f81905092915050565b8281835e5f83830152505050565b5f610bb882610b8c565b610bc28185610b96565b9350610bd2818560208601610ba0565b80840191505092915050565b5f610be98284610bae565b915081905092915050565b7f75736572206465706f73697420616c7068612063616c6c206661696c656400005f82015250565b5f610c28601e83610ac3565b9150610c3382610bf4565b602082019050919050565b5f6020820190508181035f830152610c5581610c1c565b9050919050565b7f636f6e7472616374207374616b652064656372656173656420616674657220645f8201527f65706f7369740000000000000000000000000000000000000000000000000000602082015250565b5f610cb6602683610ac3565b9150610cc182610c5c565b604082019050919050565b5f6020820190508181035f830152610ce381610caa565b9050919050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f610d21826108a7565b9150610d2c836108a7565b9250828203905081811115610d4457610d43610cea565b5b92915050565b7f75736572206465706f7369742c206d6f7665207374616b652063616c6c2066615f8201527f696c656400000000000000000000000000000000000000000000000000000000602082015250565b5f610da4602483610ac3565b9150610daf82610d4a565b604082019050919050565b5f6020820190508181035f830152610dd181610d98565b9050919050565b5f610de2826108a7565b9150610ded836108a7565b9250828201905080821115610e0557610e04610cea565b5b92915050565b7f757365722077697468647261772c20696e73756666696369656e7420616c70685f8201527f612062616c616e63650000000000000000000000000000000000000000000000602082015250565b5f610e65602983610ac3565b9150610e7082610e0b565b604082019050919050565b5f6020820190508181035f830152610e9281610e59565b9050919050565b7f636f6e7472616374207374616b6520696e6372656173656420616674657220775f8201527f6974686472617700000000000000000000000000000000000000000000000000602082015250565b5f610ef3602783610ac3565b9150610efe82610e99565b604082019050919050565b5f6020820190508181035f830152610f2081610ee7565b9050919050565b7f7573657220776974686472617720616c7068612063616c6c206661696c6564005f82015250565b5f610f5b601f83610ac3565b9150610f6682610f27565b602082019050919050565b5f6020820190508181035f830152610f8881610f4f565b9050919050565b5f606082019050610fa25f83018661095d565b610faf602083018561095d565b610fbc6040830184610a1d565b949350505050565b5f81519050610fd2816108b0565b92915050565b5f60208284031215610fed57610fec6108a3565b5b5f610ffa84828501610fc4565b9150509291505056fea2646970667358221220a0d4b2eb5f0c7f74a27f987e803ae1c8465e0da35f09c240ddb6bac757ce422164736f6c634300081e0033";

export const BRIDGE_TOKEN_CONTRACT_ABI = [
    {
        inputs: [
            {
                internalType: "string",
                name: "name_",
                type: "string",
            },
            {
                internalType: "string",
                name: "symbol_",
                type: "string",
            },
            {
                internalType: "address",
                name: "admin",
                type: "address",
            },
        ],
        stateMutability: "nonpayable",
        type: "constructor",
    },
    {
        inputs: [],
        name: "AccessControlBadConfirmation",
        type: "error",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "account",
                type: "address",
            },
            {
                internalType: "bytes32",
                name: "neededRole",
                type: "bytes32",
            },
        ],
        name: "AccessControlUnauthorizedAccount",
        type: "error",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "spender",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "allowance",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "needed",
                type: "uint256",
            },
        ],
        name: "ERC20InsufficientAllowance",
        type: "error",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "sender",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "balance",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "needed",
                type: "uint256",
            },
        ],
        name: "ERC20InsufficientBalance",
        type: "error",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "approver",
                type: "address",
            },
        ],
        name: "ERC20InvalidApprover",
        type: "error",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "receiver",
                type: "address",
            },
        ],
        name: "ERC20InvalidReceiver",
        type: "error",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "sender",
                type: "address",
            },
        ],
        name: "ERC20InvalidSender",
        type: "error",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "spender",
                type: "address",
            },
        ],
        name: "ERC20InvalidSpender",
        type: "error",
    },
    {
        inputs: [],
        name: "UnauthorizedHandler",
        type: "error",
    },
    {
        anonymous: false,
        inputs: [
            {
                indexed: true,
                internalType: "address",
                name: "owner",
                type: "address",
            },
            {
                indexed: true,
                internalType: "address",
                name: "spender",
                type: "address",
            },
            {
                indexed: false,
                internalType: "uint256",
                name: "value",
                type: "uint256",
            },
        ],
        name: "Approval",
        type: "event",
    },
    {
        anonymous: false,
        inputs: [
            {
                indexed: true,
                internalType: "bytes32",
                name: "role",
                type: "bytes32",
            },
            {
                indexed: true,
                internalType: "bytes32",
                name: "previousAdminRole",
                type: "bytes32",
            },
            {
                indexed: true,
                internalType: "bytes32",
                name: "newAdminRole",
                type: "bytes32",
            },
        ],
        name: "RoleAdminChanged",
        type: "event",
    },
    {
        anonymous: false,
        inputs: [
            {
                indexed: true,
                internalType: "bytes32",
                name: "role",
                type: "bytes32",
            },
            {
                indexed: true,
                internalType: "address",
                name: "account",
                type: "address",
            },
            {
                indexed: true,
                internalType: "address",
                name: "sender",
                type: "address",
            },
        ],
        name: "RoleGranted",
        type: "event",
    },
    {
        anonymous: false,
        inputs: [
            {
                indexed: true,
                internalType: "bytes32",
                name: "role",
                type: "bytes32",
            },
            {
                indexed: true,
                internalType: "address",
                name: "account",
                type: "address",
            },
            {
                indexed: true,
                internalType: "address",
                name: "sender",
                type: "address",
            },
        ],
        name: "RoleRevoked",
        type: "event",
    },
    {
        anonymous: false,
        inputs: [
            {
                indexed: true,
                internalType: "address",
                name: "from",
                type: "address",
            },
            {
                indexed: true,
                internalType: "address",
                name: "to",
                type: "address",
            },
            {
                indexed: false,
                internalType: "uint256",
                name: "value",
                type: "uint256",
            },
        ],
        name: "Transfer",
        type: "event",
    },
    {
        inputs: [],
        name: "DEFAULT_ADMIN_ROLE",
        outputs: [
            {
                internalType: "bytes32",
                name: "",
                type: "bytes32",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "owner",
                type: "address",
            },
            {
                internalType: "address",
                name: "spender",
                type: "address",
            },
        ],
        name: "allowance",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "spender",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "value",
                type: "uint256",
            },
        ],
        name: "approve",
        outputs: [
            {
                internalType: "bool",
                name: "",
                type: "bool",
            },
        ],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "account",
                type: "address",
            },
        ],
        name: "balanceOf",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint256",
                name: "value",
                type: "uint256",
            },
        ],
        name: "burn",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "from",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
        ],
        name: "burnFrom",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [],
        name: "decimals",
        outputs: [
            {
                internalType: "uint8",
                name: "",
                type: "uint8",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "role",
                type: "bytes32",
            },
        ],
        name: "getRoleAdmin",
        outputs: [
            {
                internalType: "bytes32",
                name: "",
                type: "bytes32",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "role",
                type: "bytes32",
            },
            {
                internalType: "address",
                name: "account",
                type: "address",
            },
        ],
        name: "grantRole",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "role",
                type: "bytes32",
            },
            {
                internalType: "address",
                name: "account",
                type: "address",
            },
        ],
        name: "hasRole",
        outputs: [
            {
                internalType: "bool",
                name: "",
                type: "bool",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "account",
                type: "address",
            },
        ],
        name: "isAdmin",
        outputs: [
            {
                internalType: "bool",
                name: "",
                type: "bool",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "to",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
        ],
        name: "mint",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [],
        name: "name",
        outputs: [
            {
                internalType: "string",
                name: "",
                type: "string",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "role",
                type: "bytes32",
            },
            {
                internalType: "address",
                name: "callerConfirmation",
                type: "address",
            },
        ],
        name: "renounceRole",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "role",
                type: "bytes32",
            },
            {
                internalType: "address",
                name: "account",
                type: "address",
            },
        ],
        name: "revokeRole",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes4",
                name: "interfaceId",
                type: "bytes4",
            },
        ],
        name: "supportsInterface",
        outputs: [
            {
                internalType: "bool",
                name: "",
                type: "bool",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "symbol",
        outputs: [
            {
                internalType: "string",
                name: "",
                type: "string",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "totalSupply",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "to",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "value",
                type: "uint256",
            },
        ],
        name: "transfer",
        outputs: [
            {
                internalType: "bool",
                name: "",
                type: "bool",
            },
        ],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "from",
                type: "address",
            },
            {
                internalType: "address",
                name: "to",
                type: "address",
            },
            {
                internalType: "uint256",
                name: "value",
                type: "uint256",
            },
        ],
        name: "transferFrom",
        outputs: [
            {
                internalType: "bool",
                name: "",
                type: "bool",
            },
        ],
        stateMutability: "nonpayable",
        type: "function",
    },
];

export const BRIDGE_TOKEN_CONTRACT_BYTECODE =
    "0x60806040523480156200001157600080fd5b5060405162000fac38038062000fac8339810160408190526200003491620001ea565b8282600362000044838262000308565b50600462000053828262000308565b5062000065915060009050826200006f565b50505050620003d4565b60008281526005602090815260408083206001600160a01b038516845290915281205460ff16620001185760008381526005602090815260408083206001600160a01b03861684529091529020805460ff19166001179055620000cf3390565b6001600160a01b0316826001600160a01b0316847f2f8788117e7eff1d82e926ec794901d17c78024a50270940304540a733656f0d60405160405180910390a45060016200011c565b5060005b92915050565b634e487b7160e01b600052604160045260246000fd5b600082601f8301126200014a57600080fd5b81516001600160401b038082111562000167576200016762000122565b604051601f8301601f19908116603f0116810190828211818310171562000192576200019262000122565b8160405283815260209250866020858801011115620001b057600080fd5b600091505b83821015620001d45785820183015181830184015290820190620001b5565b6000602085830101528094505050505092915050565b6000806000606084860312156200020057600080fd5b83516001600160401b03808211156200021857600080fd5b620002268783880162000138565b945060208601519150808211156200023d57600080fd5b506200024c8682870162000138565b604086015190935090506001600160a01b03811681146200026c57600080fd5b809150509250925092565b600181811c908216806200028c57607f821691505b602082108103620002ad57634e487b7160e01b600052602260045260246000fd5b50919050565b601f82111562000303576000816000526020600020601f850160051c81016020861015620002de5750805b601f850160051c820191505b81811015620002ff57828155600101620002ea565b5050505b505050565b81516001600160401b0381111562000324576200032462000122565b6200033c8162000335845462000277565b84620002b3565b602080601f8311600181146200037457600084156200035b5750858301515b600019600386901b1c1916600185901b178555620002ff565b600085815260208120601f198616915b82811015620003a55788860151825594840194600190910190840162000384565b5085821015620003c45787850151600019600388901b60f8161c191681555b5050505050600190811b01905550565b610bc880620003e46000396000f3fe608060405234801561001057600080fd5b506004361061012c5760003560e01c806340c10f19116100ad57806395d89b411161007157806395d89b4114610288578063a217fddf14610290578063a9059cbb14610298578063d547741f146102ab578063dd62ed3e146102be57600080fd5b806340c10f191461021357806342966c681461022657806370a082311461023957806379cc67901461026257806391d148541461027557600080fd5b8063248a9ca3116100f4578063248a9ca3146101a657806324d7806c146101c95780632f2ff15d146101dc578063313ce567146101f157806336568abe1461020057600080fd5b806301ffc9a71461013157806306fdde0314610159578063095ea7b31461016e57806318160ddd1461018157806323b872dd14610193575b600080fd5b61014461013f3660046109ab565b6102f7565b60405190151581526020015b60405180910390f35b61016161032e565b60405161015091906109dc565b61014461017c366004610a47565b6103c0565b6002545b604051908152602001610150565b6101446101a1366004610a71565b6103d8565b6101856101b4366004610aad565b60009081526005602052604090206001015490565b6101446101d7366004610ac6565b6103fc565b6101ef6101ea366004610ae1565b610408565b005b60405160128152602001610150565b6101ef61020e366004610ae1565b610433565b6101ef610221366004610a47565b61046b565b6101ef610234366004610aad565b610480565b610185610247366004610ac6565b6001600160a01b031660009081526020819052604090205490565b6101ef610270366004610a47565b61048d565b610144610283366004610ae1565b6104a2565b6101616104cd565b610185600081565b6101446102a6366004610a47565b6104dc565b6101ef6102b9366004610ae1565b6104ea565b6101856102cc366004610b0d565b6001600160a01b03918216600090815260016020908152604080832093909416825291909152205490565b60006001600160e01b03198216637965db0b60e01b148061032857506301ffc9a760e01b6001600160e01b03198316145b92915050565b60606003805461033d90610b37565b80601f016020809104026020016040519081016040528092919081815260200182805461036990610b37565b80156103b65780601f1061038b576101008083540402835291602001916103b6565b820191906000526020600020905b81548152906001019060200180831161039957829003601f168201915b5050505050905090565b6000336103ce81858561050f565b5060019392505050565b6000336103e685828561051c565b6103f1858585610599565b506001949350505050565b600061032881836104a2565b600082815260056020526040902060010154610423816105f8565b61042d8383610602565b50505050565b6001600160a01b038116331461045c5760405163334bd91960e11b815260040160405180910390fd5b6104668282610696565b505050565b6000610476816105f8565b6104668383610703565b61048a338261073d565b50565b6000610498816105f8565b610466838361073d565b60009182526005602090815260408084206001600160a01b0393909316845291905290205460ff1690565b60606004805461033d90610b37565b6000336103ce818585610599565b600082815260056020526040902060010154610505816105f8565b61042d8383610696565b6104668383836001610773565b6001600160a01b03838116600090815260016020908152604080832093861683529290522054600019811461042d578181101561058a57604051637dc7a0d960e11b81526001600160a01b038416600482015260248101829052604481018390526064015b60405180910390fd5b61042d84848484036000610773565b6001600160a01b0383166105c357604051634b637e8f60e11b815260006004820152602401610581565b6001600160a01b0382166105ed5760405163ec442f0560e01b815260006004820152602401610581565b610466838383610848565b61048a8133610972565b600061060e83836104a2565b61068e5760008381526005602090815260408083206001600160a01b03861684529091529020805460ff191660011790556106463390565b6001600160a01b0316826001600160a01b0316847f2f8788117e7eff1d82e926ec794901d17c78024a50270940304540a733656f0d60405160405180910390a4506001610328565b506000610328565b60006106a283836104a2565b1561068e5760008381526005602090815260408083206001600160a01b0386168085529252808320805460ff1916905551339286917ff6391f5c32d9c69d2a47ea670b442974b53935d1edc7fd64eb21e047a839171b9190a4506001610328565b6001600160a01b03821661072d5760405163ec442f0560e01b815260006004820152602401610581565b61073960008383610848565b5050565b6001600160a01b03821661076757604051634b637e8f60e11b815260006004820152602401610581565b61073982600083610848565b6001600160a01b03841661079d5760405163e602df0560e01b815260006004820152602401610581565b6001600160a01b0383166107c757604051634a1406b160e11b815260006004820152602401610581565b6001600160a01b038085166000908152600160209081526040808320938716835292905220829055801561042d57826001600160a01b0316846001600160a01b03167f8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b9258460405161083a91815260200190565b60405180910390a350505050565b6001600160a01b0383166108735780600260008282546108689190610b71565b909155506108e59050565b6001600160a01b038316600090815260208190526040902054818110156108c65760405163391434e360e21b81526001600160a01b03851660048201526024810182905260448101839052606401610581565b6001600160a01b03841660009081526020819052604090209082900390555b6001600160a01b03821661090157600280548290039055610920565b6001600160a01b03821660009081526020819052604090208054820190555b816001600160a01b0316836001600160a01b03167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8360405161096591815260200190565b60405180910390a3505050565b61097c82826104a2565b6107395760405163e2517d3f60e01b81526001600160a01b038216600482015260248101839052604401610581565b6000602082840312156109bd57600080fd5b81356001600160e01b0319811681146109d557600080fd5b9392505050565b60006020808352835180602085015260005b81811015610a0a578581018301518582016040015282016109ee565b506000604082860101526040601f19601f8301168501019250505092915050565b80356001600160a01b0381168114610a4257600080fd5b919050565b60008060408385031215610a5a57600080fd5b610a6383610a2b565b946020939093013593505050565b600080600060608486031215610a8657600080fd5b610a8f84610a2b565b9250610a9d60208501610a2b565b9150604084013590509250925092565b600060208284031215610abf57600080fd5b5035919050565b600060208284031215610ad857600080fd5b6109d582610a2b565b60008060408385031215610af457600080fd5b82359150610b0460208401610a2b565b90509250929050565b60008060408385031215610b2057600080fd5b610b2983610a2b565b9150610b0460208401610a2b565b600181811c90821680610b4b57607f821691505b602082108103610b6b57634e487b7160e01b600052602260045260246000fd5b50919050565b8082018082111561032857634e487b7160e01b600052601160045260246000fdfea2646970667358221220e179fc58c926e64cb6e87416f8ca64c117044e3195b184afe45038857606c15364736f6c63430008160033";

export const PRECOMPILE_WRAPPER_ABI = [
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "delegate",
                type: "bytes32",
            },
            {
                internalType: "uint8",
                name: "proxy_type",
                type: "uint8",
            },
            {
                internalType: "uint32",
                name: "delay",
                type: "uint32",
            },
        ],
        name: "addProxy",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "addStake",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "address",
                name: "target_address",
                type: "address",
            },
        ],
        name: "addressMapping",
        outputs: [
            {
                internalType: "bytes32",
                name: "",
                type: "bytes32",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "addressMappingPrecompile",
        outputs: [
            {
                internalType: "contract IAddressMapping",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "alpha",
        outputs: [
            {
                internalType: "contract IAlpha",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "balanceTransfer",
        outputs: [
            {
                internalType: "contract ISubtensorBalanceTransfer",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
        ],
        name: "burnedRegister",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint64",
                name: "deposit",
                type: "uint64",
            },
            {
                internalType: "uint64",
                name: "minContribution",
                type: "uint64",
            },
            {
                internalType: "uint64",
                name: "cap",
                type: "uint64",
            },
            {
                internalType: "uint32",
                name: "end",
                type: "uint32",
            },
            {
                internalType: "address",
                name: "targetAddress",
                type: "address",
            },
        ],
        name: "createCrowdloan",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint64",
                name: "crowdloanDeposit",
                type: "uint64",
            },
            {
                internalType: "uint64",
                name: "crowdloanMinContribution",
                type: "uint64",
            },
            {
                internalType: "uint64",
                name: "crowdloanCap",
                type: "uint64",
            },
            {
                internalType: "uint32",
                name: "crowdloanEnd",
                type: "uint32",
            },
            {
                internalType: "uint8",
                name: "leasingEmissionsShare",
                type: "uint8",
            },
            {
                internalType: "bool",
                name: "hasLeasingEndBlock",
                type: "bool",
            },
            {
                internalType: "uint32",
                name: "leasingEndBlock",
                type: "uint32",
            },
        ],
        name: "createLeaseCrowdloan",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [],
        name: "crowdloan",
        outputs: [
            {
                internalType: "contract ICrowdloan",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
        ],
        name: "getAlphaPrice",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint32",
                name: "crowdloanId",
                type: "uint32",
            },
            {
                internalType: "bytes32",
                name: "coldkey",
                type: "bytes32",
            },
        ],
        name: "getContribution",
        outputs: [
            {
                internalType: "uint64",
                name: "",
                type: "uint64",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint32",
                name: "leaseId",
                type: "uint32",
            },
            {
                internalType: "bytes32",
                name: "contributor",
                type: "bytes32",
            },
        ],
        name: "getContributorShare",
        outputs: [
            {
                internalType: "uint128",
                name: "",
                type: "uint128",
            },
            {
                internalType: "uint128",
                name: "",
                type: "uint128",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint32",
                name: "crowdloanId",
                type: "uint32",
            },
        ],
        name: "getCrowdloan",
        outputs: [
            {
                components: [
                    {
                        internalType: "bytes32",
                        name: "creator",
                        type: "bytes32",
                    },
                    {
                        internalType: "uint64",
                        name: "deposit",
                        type: "uint64",
                    },
                    {
                        internalType: "uint64",
                        name: "min_contribution",
                        type: "uint64",
                    },
                    {
                        internalType: "uint32",
                        name: "end",
                        type: "uint32",
                    },
                    {
                        internalType: "uint64",
                        name: "cap",
                        type: "uint64",
                    },
                    {
                        internalType: "bytes32",
                        name: "funds_account",
                        type: "bytes32",
                    },
                    {
                        internalType: "uint64",
                        name: "raised",
                        type: "uint64",
                    },
                    {
                        internalType: "bool",
                        name: "has_target_address",
                        type: "bool",
                    },
                    {
                        internalType: "bytes32",
                        name: "target_address",
                        type: "bytes32",
                    },
                    {
                        internalType: "bool",
                        name: "finalized",
                        type: "bool",
                    },
                    {
                        internalType: "uint32",
                        name: "contributors_count",
                        type: "uint32",
                    },
                ],
                internalType: "struct CrowdloanInfo",
                name: "",
                type: "tuple",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "account",
                type: "bytes32",
            },
        ],
        name: "getProxies",
        outputs: [
            {
                components: [
                    {
                        internalType: "bytes32",
                        name: "delegate",
                        type: "bytes32",
                    },
                    {
                        internalType: "uint256",
                        name: "proxy_type",
                        type: "uint256",
                    },
                    {
                        internalType: "uint256",
                        name: "delay",
                        type: "uint256",
                    },
                ],
                internalType: "struct IProxy.ProxyInfo[]",
                name: "",
                type: "tuple[]",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
        ],
        name: "getServingRateLimit",
        outputs: [
            {
                internalType: "uint64",
                name: "",
                type: "uint64",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
        ],
        name: "getNetworkRegistrationBlock",
        outputs: [
            {
                internalType: "uint64",
                name: "",
                type: "uint64",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "coldkey",
                type: "bytes32",
            },
        ],
        name: "getTotalColdkeyStake",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
        ],
        name: "getTotalHotkeyStake",
        outputs: [
            {
                internalType: "uint256",
                name: "",
                type: "uint256",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
        ],
        name: "getUidCount",
        outputs: [
            {
                internalType: "uint16",
                name: "",
                type: "uint16",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "leasing",
        outputs: [
            {
                internalType: "contract ILeasing",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "metagraph",
        outputs: [
            {
                internalType: "contract IMetagraph",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "neuron",
        outputs: [
            {
                internalType: "contract INeuron",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "proxy",
        outputs: [
            {
                internalType: "contract IProxy",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "real",
                type: "bytes32",
            },
            {
                internalType: "uint8[]",
                name: "force_proxy_type",
                type: "uint8[]",
            },
            {
                internalType: "uint8[]",
                name: "call",
                type: "uint8[]",
            },
        ],
        name: "proxyCall",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "string",
                name: "subnetName",
                type: "string",
            },
            {
                internalType: "string",
                name: "githubRepo",
                type: "string",
            },
            {
                internalType: "string",
                name: "subnetContact",
                type: "string",
            },
            {
                internalType: "string",
                name: "subnetUrl",
                type: "string",
            },
            {
                internalType: "string",
                name: "discord",
                type: "string",
            },
            {
                internalType: "string",
                name: "description",
                type: "string",
            },
            {
                internalType: "string",
                name: "additional",
                type: "string",
            },
        ],
        name: "registerNetworkWithDetails",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
        ],
        name: "removeStake",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [],
        name: "staking",
        outputs: [
            {
                internalType: "contract IStaking",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "subnet",
        outputs: [
            {
                internalType: "contract ISubnet",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "data",
                type: "bytes32",
            },
        ],
        name: "transfer",
        outputs: [],
        stateMutability: "payable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint16",
                name: "netuid",
                type: "uint16",
            },
            {
                internalType: "address",
                name: "evm_address",
                type: "address",
            },
            {
                internalType: "uint16",
                name: "limit",
                type: "uint16",
            },
        ],
        name: "uidLookup",
        outputs: [
            {
                components: [
                    {
                        internalType: "uint16",
                        name: "uid",
                        type: "uint16",
                    },
                    {
                        internalType: "uint64",
                        name: "block_associated",
                        type: "uint64",
                    },
                ],
                internalType: "struct LookupItem[]",
                name: "",
                type: "tuple[]",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [],
        name: "uidLookupPrecompile",
        outputs: [
            {
                internalType: "contract IUidLookup",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
];

export const PRECOMPILE_WRAPPER_BYTECODE =
    "6080604052348015600e575f5ffd5b506119368061001c5f395ff3fe6080604052600436106101da575f3560e01c80638bba466c116100fd578063b1f789ef11610092578063d75e3e0d11610062578063d75e3e0d14610547578063db1d0fd51461055c578063ec55688914610571578063fc6679fb14610586575f5ffd5b8063b1f789ef146104de578063bfe252a21461050a578063caf2ebf21461051f578063cd6f4eb114610534575f5ffd5b8063a2176276116100cd578063a217627614610482578063ac3166bf14610497578063afed65f9146104ac578063b0c751b0146104bf575f5ffd5b80638bba466c146103ec57806394e3ac6f14610418578063998538c4146104445780639f246f6f14610463575f5ffd5b80634cf088d91161017357806369e38bc31161014357806369e38bc31461038857806371214e27146103a75780637444dadc146103ba5780637d691e30146103d9575f5ffd5b80634cf088d9146103145780635b53ddde146103295780635b7210c51461033e5780635e25f3f814610375575f5ffd5b80631fc9b141116101ae5780631fc9b141146102825780633175bd98146102955780634054ecca146102d45780634c378a96146102e7575f5ffd5b80620ae759146101de5780630494cd9a146101ff5780630cadeda5146102315780631f19357214610250575b5f5ffd5b3480156101e9575f5ffd5b506101fd6101f8366004610e85565b61059b565b005b34801561020a575f5ffd5b5061021e610219366004610f06565b6105f4565b6040519081526020015b60405180910390f35b34801561023c575f5ffd5b506101fd61024b366004610f33565b610665565b34801561025b575f5ffd5b5061026f61026a366004610f7f565b6106a0565b60405161ffff9091168152602001610228565b6101fd610290366004610f9a565b610705565b3480156102a0575f5ffd5b506102b46102af366004610fc3565b610739565b604080516001600160801b03938416815292909116602083015201610228565b6101fd6102e2366004610fed565b6107b3565b3480156102f2575f5ffd5b506102fc61080481565b6040516001600160a01b039091168152602001610228565b34801561031f575f5ffd5b506102fc61080581565b348015610334575f5ffd5b506102fc61080a81565b348015610349575f5ffd5b5061035d610358366004610fc3565b6107f7565b6040516001600160401b039091168152602001610228565b6101fd610383366004611074565b61086c565b348015610393575f5ffd5b5061021e6103a2366004610f7f565b6108d6565b6101fd6103b53660046111c2565b610901565b3480156103c5575f5ffd5b5061035d6103d4366004610f7f565b610989565b6101fd6103e7366004610f9a565b6109ef565b3480156103f7575f5ffd5b5061040b61040636600461122b565b610a23565b6040516102289190611246565b348015610423575f5ffd5b50610437610432366004611334565b610add565b604051610228919061134b565b34801561044f575f5ffd5b5061021e61045e366004611334565b610b42565b34801561046e575f5ffd5b5061021e61047d366004611334565b610b6a565b34801561048d575f5ffd5b506102fc61080681565b3480156104a2575f5ffd5b506102fc61080c81565b6101fd6104ba3660046113b6565b610b92565b3480156104ca575f5ffd5b5061035d6104d9366004610f7f565b610c26565b3480156104e9575f5ffd5b506104fd6104f8366004611445565b610c51565b6040516102289190611480565b348015610515575f5ffd5b506102fc61080981565b34801561052a575f5ffd5b506102fc61080381565b6101fd610542366004611334565b610cd8565b348015610552575f5ffd5b506102fc61080081565b348015610567575f5ffd5b506102fc61080881565b34801561057c575f5ffd5b506102fc61080b81565b348015610591575f5ffd5b506102fc61080281565b604051620ae75960e01b815261080b90620ae759906105c29086908690869060040161150d565b5f604051808303815f87803b1580156105d9575f5ffd5b505af11580156105eb573d5f5f3e3d5ffd5b50505050505050565b60405163024a66cd60e11b81526001600160a01b03821660048201525f9061080c90630494cd9a906024015b602060405180830381865afa15801561063b573d5f5f3e3d5ffd5b505050506040513d601f19601f8201168201806040525081019061065f9190611541565b92915050565b604051630cadeda560e01b81526004810184905260ff8316602482015263ffffffff8216604482015261080b90630cadeda5906064016105c2565b604051630f8c9ab960e11b815261ffff821660048201525f9061080290631f19357290602401602060405180830381865afa1580156106e1573d5f5f3e3d5ffd5b505050506040513d601f19601f8201168201806040525081019061065f9190611558565b604051631fc9b14160e01b815260048101849052602481018390526044810182905261080590631fc9b141906064016105c2565b60405163062eb7b360e31b815263ffffffff83166004820152602481018290525f90819061080a90633175bd98906044016040805180830381865afa158015610784573d5f5f3e3d5ffd5b505050506040513d601f19601f820116820180604052508101906107a89190611589565b915091509250929050565b60405163202a766560e11b815261ffff831660048201526024810182905261080490634054ecca9034906044015f604051808303818588803b1580156105d9575f5ffd5b604051635b7210c560e01b815263ffffffff83166004820152602481018290525f9061080990635b7210c590604401602060405180830381865afa158015610841573d5f5f3e3d5ffd5b505050506040513d601f19601f8201168201806040525081019061086591906115c5565b9392505050565b604051631cf98c6b60e01b815261080390631cf98c6b9061089f908b908b908b908b908b908b908b908b9060040161160e565b5f604051808303815f87803b1580156108b6575f5ffd5b505af11580156108c8573d5f5f3e3d5ffd5b505050505050505050505050565b6040516369e38bc360e01b815261ffff821660048201525f90610808906369e38bc390602401610620565b60405163127e1adb60e01b81526001600160401b03808716600483015280861660248301528416604482015263ffffffff831660648201526001600160a01b03821660848201526108099063127e1adb9060a4015f604051808303815f87803b15801561096c575f5ffd5b505af115801561097e573d5f5f3e3d5ffd5b505050505050505050565b604051631d1136b760e21b815261ffff821660048201525f9061080390637444dadc906024015b602060405180830381865afa1580156109cb573d5f5f3e3d5ffd5b505050506040513d601f19601f8201168201806040525081019061065f91906115c5565b6040516307d691e360e41b815260048101849052602481018390526044810182905261080590637d691e30906064016105c2565b60408051610160810182525f80825260208201819052818301819052606082018190526080820181905260a0820181905260c0820181905260e082018190526101008201819052610120820181905261014082015290516322ee919b60e21b815263ffffffff8316600482015261080990638bba466c9060240161016060405180830381865afa158015610ab9573d5f5f3e3d5ffd5b505050506040513d601f19601f8201168201806040525081019061065f91906116c3565b6040516394e3ac6f60e01b81526004810182905260609061080b906394e3ac6f906024015f60405180830381865afa158015610b1b573d5f5f3e3d5ffd5b505050506040513d5f823e601f3d908101601f1916820160405261065f919081019061178a565b6040516326614e3160e21b8152600481018290525f906108059063998538c490602401610620565b604051639f246f6f60e01b8152600481018290525f9061080590639f246f6f90602401610620565b60405163afed65f960e01b81526001600160401b03808916600483015280881660248301528616604482015263ffffffff808616606483015260ff8516608483015283151560a4830152821660c482015261080a9063afed65f99060e4015f604051808303815f87803b158015610c07575f5ffd5b505af1158015610c19573d5f5f3e3d5ffd5b5050505050505050505050565b604051630b0c751b60e41b815261ffff821660048201525f906108039063b0c751b0906024016109b0565b60405163b1f789ef60e01b815261ffff80851660048301526001600160a01b0384166024830152821660448201526060906108069063b1f789ef906064015f60405180830381865afa158015610ca9573d5f5f3e3d5ffd5b505050506040513d5f823e601f3d908101601f19168201604052610cd0919081019061183f565b949350505050565b60405163cd6f4eb160e01b8152600481018290526108009063cd6f4eb19034906024015f604051808303818588803b158015610d12575f5ffd5b505af1158015610d24573d5f5f3e3d5ffd5b505050505050565b634e487b7160e01b5f52604160045260245ffd5b60405161016081016001600160401b0381118282101715610d6357610d63610d2c565b60405290565b604051606081016001600160401b0381118282101715610d6357610d63610d2c565b604080519081016001600160401b0381118282101715610d6357610d63610d2c565b604051601f8201601f191681016001600160401b0381118282101715610dd557610dd5610d2c565b604052919050565b5f6001600160401b03821115610df557610df5610d2c565b5060051b60200190565b803560ff81168114610e0f575f5ffd5b919050565b5f82601f830112610e23575f5ffd5b8135610e36610e3182610ddd565b610dad565b8082825260208201915060208360051b860101925085831115610e57575f5ffd5b602085015b83811015610e7b57610e6d81610dff565b835260209283019201610e5c565b5095945050505050565b5f5f5f60608486031215610e97575f5ffd5b8335925060208401356001600160401b03811115610eb3575f5ffd5b610ebf86828701610e14565b92505060408401356001600160401b03811115610eda575f5ffd5b610ee686828701610e14565b9150509250925092565b80356001600160a01b0381168114610e0f575f5ffd5b5f60208284031215610f16575f5ffd5b61086582610ef0565b63ffffffff81168114610f30575f5ffd5b50565b5f5f5f60608486031215610f45575f5ffd5b83359250610f5560208501610dff565b91506040840135610f6581610f1f565b809150509250925092565b61ffff81168114610f30575f5ffd5b5f60208284031215610f8f575f5ffd5b813561086581610f70565b5f5f5f60608486031215610fac575f5ffd5b505081359360208301359350604090920135919050565b5f5f60408385031215610fd4575f5ffd5b8235610fdf81610f1f565b946020939093013593505050565b5f5f60408385031215610ffe575f5ffd5b8235610fdf81610f70565b5f82601f830112611018575f5ffd5b81356001600160401b0381111561103157611031610d2c565b611044601f8201601f1916602001610dad565b818152846020838601011115611058575f5ffd5b816020850160208301375f918101602001919091529392505050565b5f5f5f5f5f5f5f5f610100898b03121561108c575f5ffd5b8835975060208901356001600160401b038111156110a8575f5ffd5b6110b48b828c01611009565b97505060408901356001600160401b038111156110cf575f5ffd5b6110db8b828c01611009565b96505060608901356001600160401b038111156110f6575f5ffd5b6111028b828c01611009565b95505060808901356001600160401b0381111561111d575f5ffd5b6111298b828c01611009565b94505060a08901356001600160401b03811115611144575f5ffd5b6111508b828c01611009565b93505060c08901356001600160401b0381111561116b575f5ffd5b6111778b828c01611009565b92505060e08901356001600160401b03811115611192575f5ffd5b61119e8b828c01611009565b9150509295985092959890939650565b6001600160401b0381168114610f30575f5ffd5b5f5f5f5f5f60a086880312156111d6575f5ffd5b85356111e1816111ae565b945060208601356111f1816111ae565b93506040860135611201816111ae565b9250606086013561121181610f1f565b915061121f60808701610ef0565b90509295509295909350565b5f6020828403121561123b575f5ffd5b813561086581610f1f565b8151815260208083015161016083019161126a908401826001600160401b03169052565b50604083015161128560408401826001600160401b03169052565b50606083015161129d606084018263ffffffff169052565b5060808301516112b860808401826001600160401b03169052565b5060a083015160a083015260c08301516112dd60c08401826001600160401b03169052565b5060e08301516112f160e084018215159052565b5061010083015161010083015261012083015161131361012084018215159052565b5061014083015161132d61014084018263ffffffff169052565b5092915050565b5f60208284031215611344575f5ffd5b5035919050565b602080825282518282018190525f918401906040840190835b8181101561139e57835180518452602081015160208501526040810151604085015250606083019250602084019350600181019050611364565b509095945050505050565b8015158114610f30575f5ffd5b5f5f5f5f5f5f5f60e0888a0312156113cc575f5ffd5b87356113d7816111ae565b965060208801356113e7816111ae565b955060408801356113f7816111ae565b9450606088013561140781610f1f565b935061141560808901610dff565b925060a0880135611425816113a9565b915060c088013561143581610f1f565b8091505092959891949750929550565b5f5f5f60608486031215611457575f5ffd5b833561146281610f70565b925061147060208501610ef0565b91506040840135610f6581610f70565b602080825282518282018190525f918401906040840190835b8181101561139e578351805161ffff1684526020908101516001600160401b03168185015290930192604090920191600101611499565b5f8151808452602084019350602083015f5b8281101561150357815160ff168652602095860195909101906001016114e2565b5093949350505050565b838152606060208201525f61152560608301856114d0565b828103604084015261153781856114d0565b9695505050505050565b5f60208284031215611551575f5ffd5b5051919050565b5f60208284031215611568575f5ffd5b815161086581610f70565b80516001600160801b0381168114610e0f575f5ffd5b5f5f6040838503121561159a575f5ffd5b6115a383611573565b91506115b160208401611573565b90509250929050565b8051610e0f816111ae565b5f602082840312156115d5575f5ffd5b8151610865816111ae565b5f81518084528060208401602086015e5f602082860101526020601f19601f83011685010191505092915050565b88815261010060208201525f61162861010083018a6115e0565b828103604084015261163a818a6115e0565b9050828103606084015261164e81896115e0565b9050828103608084015261166281886115e0565b905082810360a084015261167681876115e0565b905082810360c084015261168a81866115e0565b905082810360e084015261169e81856115e0565b9b9a5050505050505050505050565b8051610e0f81610f1f565b8051610e0f816113a9565b5f6101608284031280156116d5575f5ffd5b506116de610d40565b825181526116ee602084016115ba565b60208201526116ff604084016115ba565b6040820152611710606084016116ad565b6060820152611721608084016115ba565b608082015260a0838101519082015261173c60c084016115ba565b60c082015261174d60e084016116b8565b60e0820152610100838101519082015261176a61012084016116b8565b61012082015261177d61014084016116ad565b6101408201529392505050565b5f6020828403121561179a575f5ffd5b81516001600160401b038111156117af575f5ffd5b8201601f810184136117bf575f5ffd5b80516117cd610e3182610ddd565b808282526020820191506020606084028501019250868311156117ee575f5ffd5b6020840193505b82841015611537576060848803121561180c575f5ffd5b611814610d69565b84518152602080860151818301526040808701519083015290835260609094019391909101906117f5565b5f6020828403121561184f575f5ffd5b81516001600160401b03811115611864575f5ffd5b8201601f81018413611874575f5ffd5b8051611882610e3182610ddd565b8082825260208201915060208360061b8501019250868311156118a3575f5ffd5b6020840193505b8284101561153757604084880312156118c1575f5ffd5b6118c9610d8b565b84516118d481610f70565b815260208501516118e4816111ae565b80602083015250808352506020820191506040840193506118aa56fea264697066735822122026460b0cf8f5e17c58e4083c1b1155431c8d2cb9962cd9d5f6105ce473df73ee64736f6c63430008230033";

export const STAKE_WRAP_ABI = [
    {
        inputs: [],
        stateMutability: "nonpayable",
        type: "constructor",
    },
    {
        inputs: [],
        name: "owner",
        outputs: [
            {
                internalType: "address",
                name: "",
                type: "address",
            },
        ],
        stateMutability: "view",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
        ],
        name: "removeStake",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
        ],
        name: "stake",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "bytes32",
                name: "hotkey",
                type: "bytes32",
            },
            {
                internalType: "uint256",
                name: "netuid",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "limitPrice",
                type: "uint256",
            },
            {
                internalType: "uint256",
                name: "amount",
                type: "uint256",
            },
            {
                internalType: "bool",
                name: "allowPartial",
                type: "bool",
            },
        ],
        name: "stakeLimit",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        stateMutability: "payable",
        type: "receive",
    },
];

export const STAKE_WRAP_BYTECODE =
    "6080604052348015600e575f5ffd5b50335f5f6101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff160217905550610ad08061005b5f395ff3fe608060405260043610610042575f3560e01c80632daedd521461004d5780637d691e30146100755780638da5cb5b1461009d57806390b9d534146100c757610049565b3661004957005b5f5ffd5b348015610058575f5ffd5b50610073600480360381019061006e91906106bd565b6100ef565b005b348015610080575f5ffd5b5061009b600480360381019061009691906106bd565b6102ad565b005b3480156100a8575f5ffd5b506100b161046b565b6040516100be919061074c565b60405180910390f35b3480156100d2575f5ffd5b506100ed60048036038101906100e8919061079a565b61048f565b005b5f5f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff161461017d576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161017490610891565b60405180910390fd5b5f631fc9b14160e01b84838560405160240161019b939291906108cd565b604051602081830303815290604052907bffffffffffffffffffffffffffffffffffffffffffffffffffffffff19166020820180517bffffffffffffffffffffffffffffffffffffffffffffffffffffffff838183161783525050505090505f61080573ffffffffffffffffffffffffffffffffffffffff165a836040516102239190610954565b5f604051808303815f8787f1925050503d805f811461025d576040519150601f19603f3d011682016040523d82523d5f602084013e610262565b606091505b50509050806102a6576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161029d906109b4565b60405180910390fd5b5050505050565b5f5f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff161461033b576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161033290610891565b60405180910390fd5b5f637d691e3060e01b848385604051602401610359939291906108cd565b604051602081830303815290604052907bffffffffffffffffffffffffffffffffffffffffffffffffffffffff19166020820180517bffffffffffffffffffffffffffffffffffffffffffffffffffffffff838183161783525050505090505f61080573ffffffffffffffffffffffffffffffffffffffff165a836040516103e19190610954565b5f604051808303815f8787f1925050503d805f811461041b576040519150601f19603f3d011682016040523d82523d5f602084013e610420565b606091505b5050905080610464576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161045b906109b4565b60405180910390fd5b5050505050565b5f5f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1681565b5f5f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff161461051d576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161051490610891565b60405180910390fd5b5f635beb6b7460e01b868486858960405160240161053f9594939291906109e1565b604051602081830303815290604052907bffffffffffffffffffffffffffffffffffffffffffffffffffffffff19166020820180517bffffffffffffffffffffffffffffffffffffffffffffffffffffffff838183161783525050505090505f61080573ffffffffffffffffffffffffffffffffffffffff165a836040516105c79190610954565b5f604051808303815f8787f1925050503d805f8114610601576040519150601f19603f3d011682016040523d82523d5f602084013e610606565b606091505b505090508061064a576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161064190610a7c565b60405180910390fd5b50505050505050565b5f5ffd5b5f819050919050565b61066981610657565b8114610673575f5ffd5b50565b5f8135905061068481610660565b92915050565b5f819050919050565b61069c8161068a565b81146106a6575f5ffd5b50565b5f813590506106b781610693565b92915050565b5f5f5f606084860312156106d4576106d3610653565b5b5f6106e186828701610676565b93505060206106f2868287016106a9565b9250506040610703868287016106a9565b9150509250925092565b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f6107368261070d565b9050919050565b6107468161072c565b82525050565b5f60208201905061075f5f83018461073d565b92915050565b5f8115159050919050565b61077981610765565b8114610783575f5ffd5b50565b5f8135905061079481610770565b92915050565b5f5f5f5f5f60a086880312156107b3576107b2610653565b5b5f6107c088828901610676565b95505060206107d1888289016106a9565b94505060406107e2888289016106a9565b93505060606107f3888289016106a9565b925050608061080488828901610786565b9150509295509295909350565b5f82825260208201905092915050565b7f4f6e6c79206f776e65722063616e2063616c6c20746869732066756e6374696f5f8201527f6e00000000000000000000000000000000000000000000000000000000000000602082015250565b5f61087b602183610811565b915061088682610821565b604082019050919050565b5f6020820190508181035f8301526108a88161086f565b9050919050565b6108b881610657565b82525050565b6108c78161068a565b82525050565b5f6060820190506108e05f8301866108af565b6108ed60208301856108be565b6108fa60408301846108be565b949350505050565b5f81519050919050565b5f81905092915050565b8281835e5f83830152505050565b5f61092e82610902565b610938818561090c565b9350610948818560208601610916565b80840191505092915050565b5f61095f8284610924565b915081905092915050565b7f6164645374616b652063616c6c206661696c65640000000000000000000000005f82015250565b5f61099e601483610811565b91506109a98261096a565b602082019050919050565b5f6020820190508181035f8301526109cb81610992565b9050919050565b6109db81610765565b82525050565b5f60a0820190506109f45f8301886108af565b610a0160208301876108be565b610a0e60408301866108be565b610a1b60608301856109d2565b610a2860808301846108be565b9695505050505050565b7f6164645374616b654c696d69742063616c6c206661696c6564000000000000005f82015250565b5f610a66601983610811565b9150610a7182610a32565b602082019050919050565b5f6020820190508181035f830152610a9381610a5a565b905091905056fea2646970667358221220f8ad692d7919fb10f08e5311c64a0aa705a4e665689967633c2ddade5398076664736f6c634300081e0033";

export const PRECOMPILE_GAS_CONTRACT_ABI = [
    {
        anonymous: false,
        inputs: [
            {
                indexed: false,
                internalType: "string",
                name: "message",
                type: "string",
            },
        ],
        name: "Log",
        type: "event",
    },
    {
        inputs: [
            {
                internalType: "uint64",
                name: "iterations",
                type: "uint64",
            },
        ],
        name: "callED25519",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
    {
        inputs: [
            {
                internalType: "uint64",
                name: "iterations",
                type: "uint64",
            },
        ],
        name: "callSR25519",
        outputs: [],
        stateMutability: "nonpayable",
        type: "function",
    },
];

export const PRECOMPILE_GAS_CONTRACT_BYTECODE =
    "60806040527f1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef5f1b5f555f5f1b6001555f5f1b6002555f5f1b6003553480156045575f5ffd5b5061048b806100535f395ff3fe608060405234801561000f575f5ffd5b5060043610610034575f3560e01c806356554a5714610038578063bd9cac2b14610054575b5f5ffd5b610052600480360381019061004d919061028f565b610070565b005b61006e6004803603810190610069919061028f565b61015f565b005b5f5f90505b8167ffffffffffffffff168167ffffffffffffffff1610156101265761040373ffffffffffffffffffffffffffffffffffffffff1663869adcb95f546001546002546003546040518563ffffffff1660e01b81526004016100d994939291906102d2565b602060405180830381865afa1580156100f4573d5f5f3e3d5ffd5b505050506040513d601f19601f82011682018060405250810190610118919061034a565b508080600101915050610075565b507fcf34ef537ac33ee1ac626ca1587a0a7e8e51561e5514f8cb36afa1c5102b3bab604051610154906103cf565b60405180910390a150565b5f5f90505b8167ffffffffffffffff168167ffffffffffffffff1610156102155761040273ffffffffffffffffffffffffffffffffffffffff1663869adcb95f546001546002546003546040518563ffffffff1660e01b81526004016101c894939291906102d2565b602060405180830381865afa1580156101e3573d5f5f3e3d5ffd5b505050506040513d601f19601f82011682018060405250810190610207919061034a565b508080600101915050610164565b507fcf34ef537ac33ee1ac626ca1587a0a7e8e51561e5514f8cb36afa1c5102b3bab60405161024390610437565b60405180910390a150565b5f5ffd5b5f67ffffffffffffffff82169050919050565b61026e81610252565b8114610278575f5ffd5b50565b5f8135905061028981610265565b92915050565b5f602082840312156102a4576102a361024e565b5b5f6102b18482850161027b565b91505092915050565b5f819050919050565b6102cc816102ba565b82525050565b5f6080820190506102e55f8301876102c3565b6102f260208301866102c3565b6102ff60408301856102c3565b61030c60608301846102c3565b95945050505050565b5f8115159050919050565b61032981610315565b8114610333575f5ffd5b50565b5f8151905061034481610320565b92915050565b5f6020828403121561035f5761035e61024e565b5b5f61036c84828501610336565b91505092915050565b5f82825260208201905092915050565b7f63616c6c535232353531390000000000000000000000000000000000000000005f82015250565b5f6103b9600b83610375565b91506103c482610385565b602082019050919050565b5f6020820190508181035f8301526103e6816103ad565b9050919050565b7f63616c6c454432353531390000000000000000000000000000000000000000005f82015250565b5f610421600b83610375565b915061042c826103ed565b602082019050919050565b5f6020820190508181035f83015261044e81610415565b905091905056fea26469706673582212202addcdae9c59ee78157cddedc3148678edf455132bdfc62347f85e7c660b4d2164736f6c634300081e0033";
