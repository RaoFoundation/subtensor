#!/bin/bash

set -e

cd $(dirname $0)/..

# Clean vitest cache, so the tests order are the same on CI and locally
rm -rf node_modules/.vite/vitest
mkdir -p specs

../target/release/node-subtensor build-spec --disable-default-bootnode --raw --chain local > specs/chain-spec.json