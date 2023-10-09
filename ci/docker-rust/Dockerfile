# Note: when the rust version is changed also modify
# ci/rust-version.sh to pick up the new image tag
FROM rust:1.73.0

ARG NODE_MAJOR=18

RUN set -x \
 && apt update \
 && apt-get install apt-transport-https \
 && echo deb https://apt.buildkite.com/buildkite-agent stable main > /etc/apt/sources.list.d/buildkite-agent.list \
 && apt-key adv --no-tty --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys 32A37959C2FA5C3C99EFBC32A79206696452D198 \
 && apt update \
 && apt install -y \
      buildkite-agent \
      clang \
      cmake \
      jq \
      lcov \
      libudev-dev \
      mscgen \
      nodejs \
      net-tools \
      rsync \
      sudo \
      golang \
      unzip \
      lld \
      protobuf-compiler \
      \
 && apt remove -y libcurl4-openssl-dev \
 # node
 && sudo apt-get update \
 && sudo apt-get install -y ca-certificates curl gnupg \
 && sudo mkdir -p /etc/apt/keyrings \
 && curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key | sudo gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg \
 && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_$NODE_MAJOR.x nodistro main" | sudo tee /etc/apt/sources.list.d/nodesource.list \
 && sudo apt-get update \
 && sudo apt-get install nodejs -y \
 && node --version \
 && npm --version \
 # rust
 && rustup component add rustfmt \
 && rustup component add clippy \
 && rustup target add wasm32-unknown-unknown \
 && cargo install cargo-audit \
 && cargo install cargo-hack \
 && cargo install cargo-sort \
 && cargo install mdbook \
 && cargo install mdbook-linkcheck \
 && cargo install svgbob_cli \
 && cargo install wasm-pack \
 && cargo install sccache \
 && rustc --version \
 && cargo --version \
 && rm -rf /var/lib/apt/lists/*
