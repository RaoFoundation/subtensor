# Drand Bridge Pallet

This is a [FRAME](https://docs.substrate.io/reference/frame-pallets/) pallet that allows Substrate-based chains to bridge to drand. It only supports bridging to drand's [Quicknet](https://drand.love/blog/quicknet-is-live-on-the-league-of-entropy-mainnet), which provides fresh randomness every 3 seconds. Adding this pallet to a runtime allows it to acquire verifiable on-chain randomness which can be used in runtime modules or ink! smart contracts.

Read [here](https://github.com/ideal-lab5/pallet-drand/blob/main/docs/how_it_works.md) for a deep-dive into the pallet.

## Usage

Use this pallet in a Substrate runtime to acquire verifiable randomness from drand's quicknet.

### Node Requirements

Usage of this pallet requires that the node support:
- arkworks host functions
- offchain workers
- (optional - in case of smart contracts) Contracts pallet and drand  chain extension enabled

We have included a node in this repo, [substrate-node-template](https://github.com/ideal-lab5/pallet-drand/tree/main/substrate-node-template), that meets these requirements that you can use to get started.

See [here](https://github.com/ideal-lab5/pallet-drand/blob/main/docs/integration.md) for a detailed guide on integrating this pallet into a runtime.

### For Pallets
This pallet implements the [Randomness](https://paritytech.github.io/polkadot-sdk/master/frame_support/traits/trait.Randomness.html) trait. FRAME pallets can use it by configuring their runtimes

``` rust
impl pallet_with_randomness for Runtime {
    type Randomness = Drand;
}
```

Subsequently in your pallet, fetch the latest round randomness with:

``` rust
let latest_randomness = T::Randomness::random(b"ctx");
```

For example, the [lottery pallet](https://github.com/paritytech/polkadot-sdk/blob/d3d1542c1d387408c141f9a1a8168e32435a4be9/substrate/frame/lottery/src/lib.rs#L518)

### For Smart Contracts

Add a [chain extension](https://use.ink/macros-attributes/chain-extension/) to your runtime to expose the drand pallet's randomness. An example can be found in the template [here](https://github.com/ideal-lab5/pallet-drand/blob/f00598d961a484fc3c47d1d7f3fa74e5a9f4d38a/substrate-node-template/runtime/src/lib.rs#L854). and then follow the guide [here](https://github.com/ideal-lab5/contracts). The [template contract](https://github.com/ideal-lab5/contracts/tree/main/template) provides a minimal working example.

## Building

``` shell
cargo build
```

## Testing

We maintain a minimum of 85% coverage on all new code. You can check coverage with tarpauling by running

``` shell
cargo tarpaulin --rustflags="-C opt-level=0"
```

### Unit Tests

``` shell
cargo test
```

### Benchmarks

The pallet can be benchmarked with a substrate node that adds the pallet to it's runtime, such as the substrate-node-template example included in this repo.

``` shell
cd substrate-node-template
# build the node with benchmarks enables
cargo build --release --features runtime-benchmarks
# run the pallet benchmarks
./target/release/node-template benchmark pallet \
    --chain dev \
    --wasm-execution=compiled \
    --pallet pallet_drand \
    --extrinsic "*" \
    --steps 50 \
    --repeat 20 \
    --output ../src/new_weight.rs \
    --allow-missing-host-functions
```

License: MIT-0
