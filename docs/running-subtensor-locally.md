# Running subtensor node locally

For General information on running Subtensors, see
[**Subtensor Nodes** section in Bittensor Developer Documentation](https://docs.learnbittensor.org/subtensor-nodes).

### Running a localnet subtensor node

Running a localnet in docker compose is the easiest way to quickly iterate on
chain state, like building on the evm.

1. install docker and docker compose, along with cloning this repository.

1. build the images from source on the desired branch using
   `docker compose -f docker-compose.localnet.yml build`. Note this will take
   quite a while.

1. Run the docker compose file via
   `docker compose -f docker-compose.localnet.yml up -d`

Now you should have a full local validator running. To test your connection, you
can use the following script to check `//Alice`'s balance. Alice is a sudo
account in localnet.

```py
# pip install substrate-interface
from substrateinterface import Keypair, SubstrateInterface

substrate = SubstrateInterface(url="ws://127.0.0.1:9945")
hotkey = Keypair.create_from_uri('//Alice')
result = substrate.query("System", "Account", [hotkey.ss58_address])
print(result.value)
```
