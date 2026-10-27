# Pinned copy of paritytech/srtool's Dockerfile at the commit in
# build-srtool-image.sh. The local copy lets CI select a bounded, nearby Ubuntu
# mirror without changing the deterministic srtool source context.
FROM docker.io/library/ubuntu:22.04@sha256:0e0a0fc6d18feda9db1590da249ac93e8d5abfea8f4c3c0c849ce512b5ef8982

LABEL maintainer="chevdor@gmail.com"
LABEL description="This image contains tools for Substrate blockchains runtimes."

ARG RUSTC_VERSION="1.84.0"
ENV RUSTC_VERSION=$RUSTC_VERSION
ENV DOCKER_IMAGE="paritytech/srtool"
ENV PROFILE=release
ENV PACKAGE=polkadot-runtime
ENV BUILDER=builder
ARG UID=1001
ARG GID=1001

ENV SRTOOL_TEMPLATES=/srtool/templates

RUN groupadd -g $GID $BUILDER && \
    useradd --no-log-init -m -u $UID -s /bin/bash -d /home/$BUILDER -r -g $BUILDER $BUILDER
RUN mkdir -p ${SRTOOL_TEMPLATES} && \
    mkdir /build && chown -R $BUILDER /build && \
    mkdir /out && chown -R $BUILDER /out

WORKDIR /tmp
ENV DEBIAN_FRONTEND=noninteractive

# Tooling
ARG SUBWASM_VERSION=0.21.0
ARG TERA_CLI_VERSION=0.2.4
ARG TOML_CLI_VERSION=0.2.4
ARG UBUNTU_MIRROR="http://ubuntu.mirrors.ovh.net/ubuntu"

COPY ./templates ${SRTOOL_TEMPLATES}/
RUN sed -i "s|http://archive.ubuntu.com/ubuntu|${UBUNTU_MIRROR}|g" /etc/apt/sources.list && \
    printf '%s\n' \
        'Acquire::Retries "5";' \
        'Acquire::http::Timeout "30";' \
        'Acquire::https::Timeout "30";' \
        'APT::Update::Error-Mode "any";' \
        > /etc/apt/apt.conf.d/80-srtool-network && \
    timeout --foreground --kill-after=15s 300s sh -c 'apt-get update && \
        apt-get upgrade -y && \
        apt-get install --no-install-recommends -y \
            cmake pkg-config libssl-dev make protobuf-compiler \
            git clang bsdmainutils ca-certificates curl' && \
    curl -L https://github.com/stedolan/jq/releases/download/jq-1.6/jq-linux64 --output /usr/bin/jq && chmod a+x /usr/bin/jq && \
    rm -rf /var/lib/apt/lists/* /tmp/* && apt-get clean

RUN curl -L https://github.com/chevdor/subwasm/releases/download/v${SUBWASM_VERSION}/subwasm_linux_amd64_v${SUBWASM_VERSION}.deb --output subwasm.deb && dpkg -i subwasm.deb && subwasm --version && \
    curl -L https://github.com/chevdor/tera-cli/releases/download/v${TERA_CLI_VERSION}/tera-cli_linux_amd64.deb --output tera_cli.deb && dpkg -i tera_cli.deb && tera --version && \
    curl -L https://github.com/chevdor/toml-cli/releases/download/v${TOML_CLI_VERSION}/toml_linux_amd64_v${TOML_CLI_VERSION}.deb --output toml.deb && dpkg -i toml.deb && toml --version && \
    rm -rf /tmp/*

COPY ./scripts/* /srtool/
COPY VERSION /srtool/
COPY RUSTC_VERSION /srtool/
RUN printf '%s\n' "$RUSTC_VERSION" > /srtool/RUSTC_VERSION

USER $BUILDER
ENV RUSTUP_HOME="/home/${BUILDER}/rustup"
ENV CARGO_HOME="/home/${BUILDER}/cargo"
ENV PATH="/srtool:$PATH"

RUN echo $SHELL && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none && \
    . $CARGO_HOME/env && \
    rustup toolchain add --profile minimal ${RUSTC_VERSION} && \
    rustup target add wasm32-unknown-unknown --toolchain $RUSTC_VERSION && \
    rustup target add wasm32v1-none --toolchain $RUSTC_VERSION && \
    rustup component add rust-src --toolchain $RUSTC_VERSION && \
    rustup default $RUSTC_VERSION && \
    chmod -R a+w $RUSTUP_HOME $CARGO_HOME && \
    rustup show && rustc -V

RUN git config --global --add safe.directory /build && \
    /srtool/version && \
    echo 'PATH=".:$HOME/cargo/bin:$PATH"' >> $HOME/.bashrc

VOLUME [ "/build", "$CARGO_HOME", "/out" ]
WORKDIR /srtool

CMD ["/srtool/build"]
