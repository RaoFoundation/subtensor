#!/bin/bash
if [ -z "$CARGO_HOME" ]; then
    echo "CARGO_HOME is not set"
    exit 1
fi

cd runtime # check for symlink
if [ ! -L node-subtensor ]; then
    ln -s . node-subtensor
fi
cd ..

docker run --rm --user root --platform=linux/amd64 \
    -e PACKAGE=node-subtensor-runtime \
    -e BUILD_OPTS="--features=metadata-hash" \
    -e PROFILE=production \
    -e CONST_RANDOM_SEED=subtensor-runtime \
    -v $HOME/.cargo:/cargo-home \
    -v $(pwd):/build \
    -it srtool bash -c "git config --global --add safe.directory /build && \
             /srtool/build --app > /build/runtime/node-subtensor/srtool-output.log; \
             BUILD_EXIT_CODE=\$?; \
             if [ \"\$BUILD_EXIT_CODE\" -ne 0 ]; then \
                    cat /build/runtime/node-subtensor/srtool-output.log; \
                    exit \$BUILD_EXIT_CODE; \
             fi && \
             tail -n 1 /build/runtime/node-subtensor/srtool-output.log > /build/runtime/node-subtensor/subtensor-digest.json"