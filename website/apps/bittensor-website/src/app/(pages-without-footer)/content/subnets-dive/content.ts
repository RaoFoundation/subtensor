export const content = `
# Subnets Dive

## Understanding Key Terms

### What are subnets?

Subnetworks are self-contained incentive systems. These systems run on the Bitensor Blockchains and incentivize value creation between validators and miners. Examples of value creation can be generating digital commodities, data scraping, or intelligence. Miners and validators are rewarded in TAO.


The subnet owner determines how a subnet is run through their created code repository. Within this code is the protocol that determines 1) How miners respond to queries from the validators and 2) How validators evaluate the miner's responses to create weights.


Validators submit these weights to the chain. These are then processed by [Yuma Consensus](https://bittensor.com/docs/internals/consensus) to determine the proportion of emission that miners and validators should receive per block step.


All the common functionality currently running on Bittensor through subnet 1 will remain the same. Miners must register to subnetworks via a recycle or POW operation. The key change is that miners now follow the protocol set by the subnet owner.



#### Elements of a Subnet

Subnets are defined by distinct artifacts.

**Subnet id:** Subnets hold chain space under a unique identifier called a \`netuid\`. All operations on this subnet via the cli require the user to specify this \`netuid\`.

**Owners:** Owners can be individuals or groups. They are responsible for writing the incentive mechanisms for the subnet.  These mechanisms are into which validators and miners join to mine TAO. Owners are specified by their owner key. This key is a standard Bittensor wallet key that has performed the subnet registration and TAO lock. Owners have the right to define the mining mechanism and to set unique hyperparameters for the subnet's operations.

**Weights:** A unique weight matrix on the blockchain links each subnet, with validators on the subnet setting the rows' weights for miners in an nxn matrix.

**Miners:** Miners are endpoints that use the API defined by a subnet owner to answer queries from validators. Miners must use the templates defined by the owners to implement themselves.


**Validators:** Validators, like miners, run the code specified by subnet owners. Validators send queries to miners and evaluate those responses. Evaluations determine mechanism weights that sink to the unique weight matrix associated with the subnet. Validators earn dividends based on their network consensus, which measures their agreement with other validators.



### Questions

#### **How does one register a subnet?**

Subnets can be registered by anyone willing to lock up TAO for the duration of their subnet's existence.

To create a subnet using the cli, run the following command.
\`\`\`
btcli subnets create --wallet your_owner_wallet
\`\`\`

Running this command will prompt you to lock the current lock cost of TAO. The lock amount will be drained from your wallet and attached to your subnet. These funds will be returned to your wallet if your subnet is ever deregistered.

The calling \`owner_wallet\` owns the subnet. This wallet will receive an 18% percent cut of emissions through your mechanism which are directly sunk to your key.

Owner keys are also fully permissioned to call sudo operation on your subnet. This can be used for setting hyperparameters such as the network \`min_weights\`.

#### What is the root network?
The 'root' network is a meta subnet with id 0. This network determines the proportion of the network's block emission to be distributed to each subnet network. This is currently set to  1 TAO for every block mined.

Like other subnetworks, the root network consists of a set of validators that set weights (W). These weights are then processed by Yuma Consensus to determine an emission vector (E). The difference is that the vector E has a length equal to the number of active subnetworks currently running on the chain and each e_i in E is the emission proportion that subnet i receives every block.

The root network also doubles as the network senate. This senate is the top 12 keys on this network which have been granted veto power on proposals submitted by the triumvirate.

#### How can I become a root network member?

Like other subnetworks on Bittensor, a key must register by calling an extrinsic through the bittensor cli. However, unlike other subnetworks registering on the root network does not require a TAO burn or POW. Instead, entrance is based on the stake quantity.

To become a member of the root network your hotkey must have a stake in the top 64 accounts. When a key registers to the root network the chain either blocks the registration based on your stake amount or kicks another key if your stake exceeds another.

To register onto the chain's root network using the cli call the following btcli command.
\`\`\`
btcli root register
\`\`\`


#### What is the process of setting root weights?

The 'root' network is a meta subnet that determines the proportion of the network's block emission (currently 1 tao every block, 12 seconds).

Like other subnets, the root weights are a list of tuples (subnet_id, weight value) where the subnet_id is the identifier for the subnet and the weight value is a floating point 'weight', specifying the proportion of emission your key.

However, unlike other subnets where weights are set by the validator code you are running, root weights are set manually using the cli by members currently registered on the root subnet.

To set weights on the root network call the following from the btcli passing your weights.

\`\`\`
btcli root weights
Enter wallet name (default): <your coldkey>
Enter hotkey name (default): <your hotkey>
Enter netuids (e.g. 0, 3 ...):
Enter weights (e.g. 0.50, 0.50 ...):
\`\`\`

#### What happens when setting root weights on the root network itself?

The root network itself does not distribute emissions to its members. When setting weights on the root network a member may choose to specify a weight (0, w_0), the weight for the root network itself.

This value is used in the root network consensus mechanism to determine the proportion of emission that is recycled on Bittensor. This is not distributed during the block step.

For instance, if your root key specifies a value of 1 for subnet 0, you effectively reduce global block emission by a proportion equivalent to the amount of stake your key holds on the root network. There are some exceptions based on the root network's consensus mechanism.

#### How is consensus determined on the root network to determine subnet emission?**

The weights set on the root network compile themselves into a weight matrix which has dimension \`n x k\`, where:
- \`n\` is the number of root validator
- \`k\` is the number of subnets
- \`w_ij\` is the weight set by the ith root member and the jth network.

The total stake of each member compiles into a vector S of size n, such that the value s_i is the stake held by the ith root member.

Every 1000 blocks the root network runs Yuma Consensus version 1(YCv1). YCv1 is the same mechanism outlined in Bittensor's original white paper. The process can be written mathematically as follows

\`\`\`
R = W^T * S
T = W(!=0) * S
C = sigmoid( rho * ( T - kappa ) )
E = C * E
\`\`\`

The result of the simplistic consensus mechanism is that subnets that attract a majority of non-zero weights from validators attain a much larger proportion of the network emission.

YCv1 is likely to be updated at a later date to borrow performance from YCv2 which currently runs on other subnets.

#### What are the changes to the btcli?

The updated bittensor cli contains all the same functionality as the previous cli plus some new additions. This includes a variety of other commands for interacting with the root network, creating subnets, and modulating hyperparameters. The cli has asl now been modularized such that functionality is grouped based on use.

The major modules are the following
\`\`\`
btcli stake --help (adding removing stake)
btcli root --help (interactions with the root network)
btcli wallet --help (performing wallet specific operations, creation, etc)
btcli subnets --help (creating viewing and listing subnets)
btcli sudo --help (setting hyperparameters over subnetworks as a subnet owner)
\`\`\`

The btcli organization is subject to future changes. If in doubt use \`\`\`btcli --help\`\`\`.

#### What determines the amount of TAO locked when creating a subnetwork?

Like classic miner registration, the amount of TAO required to create a subnet is determined by supply and demand. Specifically, the lock cost is modulated based on the rate of registration for new subnets according to the following mechanism:

When a subnet is registered the lock cost is doubled. As blocks progress, the lock cost falls slowly, block by block, similar to a Dutch Auction and at a constant rate.

The rate of lock cost reduction is set so that the lock cost returns to its previous value after 2 weeks, with an initial value of 1000 TAO.


#### Is the TAO staked to create a subnet locked up or burned?

Locked.

The TAO locked by an owner remains unmoved until the subnet is deregistered by a newly entering subnet. The funds are then returned to the owner key which created the subnet.


#### Which subnets are deregistered when a new subnet is registered?

The subnet with the least emission is replaced by the incoming subnet. When a subnet is deregistered, all miners are removed and the network state is reset.

#### How many subnets will there be at launch?
8 subnets will be available at launch.

#### **What are the changes to subnet 11 and why are these made?**

Subnet 11 will exist in its current form during the launch of revolution, however may be later deregistered based on competition with other subnets.

#### Where do subnets get their emission from and how it is distrubuted?

![Emissions](/images/content/emissions.png)

Of the 100% emissions the subnet receives, it is divided by:
 - 41% to the validators
 - 41% to the miner
 - 18% to the subnet owner

The 41% emissions to the validator is also divide by:
 - 18% to the validator delegate
 - 82% distributed proportionally to all nominators

#### For new subnets, is there anything planned in the btcli that will provide information on what code to run the validators and miners?

We will introduce a subnet cli command where people can advertise their subnets

#### Will the foundation validator support all subnets available ?

The foundation validator will support all subnets doing useful work. If a subnet simply exists without doing any real contribution to the network (for example, supporting a new modality) then the foundation validator will not support it. This logic should follow for all validators. We only support what advances Bittensor.


#### Is Bittensor revolution a different substrate version that needs to be updated?
No, it is the same blockchain


#### Who is the subnet owner of S1? The Rao Foundation?
Yes, the Rao Foundation owns subnet 1 and subnet 11.
`;
