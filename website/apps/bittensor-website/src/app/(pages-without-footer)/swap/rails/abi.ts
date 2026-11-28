/** The parts of the rails' contracts that the page calls, as ethers reads them. */

const DESTINATION = 'tuple(uint8 kind, uint16 netuid, bytes32 recipient, address beneficiary)';

export const ERC20_ABI = [
  'function balanceOf(address) view returns (uint256)',
  'function allowance(address owner, address spender) view returns (uint256)',
  'function approve(address spender, uint256 amount) returns (bool)',
  'function name() view returns (string)',
  'function symbol() view returns (string)',
];

/** The local test stablecoin lets anyone mint. */
export const FAUCET_ABI = ['function mint(address to, uint256 amount)'];

export const VAULT_ABI = [
  `function deposit(uint256 amount, ${DESTINATION} destination, uint64 minOut, uint64 deadline) payable returns (uint64)`,
];

export const TOKEN_ABI = [
  ...ERC20_ABI,
  `function sell(uint64 amount, ${DESTINATION} destination, uint64 minOut, uint64 deadline) payable returns (uint64)`,
];

export const ALPHA_TOKEN_ABI = [
  ...TOKEN_ABI,
  'function principalOf(address) view returns (uint256)',
  'function rate() view returns (uint256)',
];

export const PORTAL_ABI = [
  'function tao() view returns (address)',
  'function alphaTokens(uint16 netuid) view returns (address)',
  'function orderFee() view returns (uint256)',
  'function orderSlots() view returns (uint64)',
  'function paused() view returns (bool)',
  'function owed(address) view returns (uint256)',
  'function claim(address to) returns (uint256)',
  `event OrderSent(uint64 indexed nonce, address indexed payer, uint8 source, uint16 sourceNetuid, uint64 amount, ${DESTINATION} destination, uint64 minOut, uint64 deadline, bytes32 messageId)`,
  'event Credited(uint64 indexed nonce, address indexed to, uint8 asset, uint16 netuid, uint64 amount, bool answersOrder, uint64 indexed order, bool refund)',
];

/** The hub's precompile on Bittensor's EVM. */
export const RAILS_ADDRESS = '0x0000000000000000000000000000000000000815';

export const RAILS_ABI = [
  'function quote(uint32 bridge, bytes order) view returns (bool ok, uint64 amountOut, string reason)',
  'function receipt(uint32 bridge, uint64 nonce) view returns (uint8 status, uint64 amountOut, uint32 blockNumber)',
  'function alphaRate(uint16 netuid) view returns (uint256)',
  'function creditsOf(uint16 netuid, bytes32 service, address payer) view returns (uint64)',
];
