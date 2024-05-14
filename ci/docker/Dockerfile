FROM ubuntu:20.04

ARG \
  RUST_VERSION= \
  RUST_NIGHTLY_VERSION= \
  GOLANG_VERSION=1.21.3 \
  NODE_MAJOR=18 \
  SCCACHE_VERSION=v0.5.4 \
  GRCOV_VERSION=v0.8.18

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

ENV \
  DEBIAN_FRONTEND=noninteractive \
  TZ=UTC

# golang
ENV PATH="/usr/local/go/bin:$PATH"

# rust
ENV \
  RUSTUP_HOME=/usr/local/rustup \
  CARGO_HOME=/usr/local/cargo \
  PATH="$PATH:/usr/local/cargo/bin"

RUN \
  if [ -z "$RUST_VERSION" ]; then echo "ERROR: The RUST_VERSION argument is required!" && exit 1; fi && \
  if [ -z "$RUST_NIGHTLY_VERSION" ]; then echo "ERROR: The RUST_NIGHTLY_VERSION argument is required!" && exit 1; fi && \
  apt-get update && \
  apt-get install --no-install-recommends -y \
  # basic
  tzdata \
  apt-transport-https \
  sudo \
  build-essential \
  git \
  vim \
  jq \
  ca-certificates \
  curl \
  gnupg \
  lld \
  cmake \
  # docs
  mscgen \
  # solana compiling
  libssl-dev \
  libudev-dev \
  pkg-config \
  zlib1g-dev \
  llvm \
  clang \
  cmake \
  make \
  libprotobuf-dev \
  protobuf-compiler \
  && \
  # buildkite
  curl -fsSL https://keys.openpgp.org/vks/v1/by-fingerprint/32A37959C2FA5C3C99EFBC32A79206696452D198 | gpg --dearmor -o /usr/share/keyrings/buildkite-agent-archive-keyring.gpg && \
  echo "deb [signed-by=/usr/share/keyrings/buildkite-agent-archive-keyring.gpg] https://apt.buildkite.com/buildkite-agent stable main" | tee /etc/apt/sources.list.d/buildkite-agent.list && \
  apt-get update && \
  apt-get install -y buildkite-agent && \
  # gh
  curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg && \
  sudo chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | tee /etc/apt/sources.list.d/github-cli.list > /dev/null && \
  apt-get update && \
  apt-get install -y gh && \
  # rust
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs/ | sh -s -- --no-modify-path --profile minimal --default-toolchain $RUST_VERSION -y && \
  rustup component add rustfmt && \
  rustup component add clippy && \
  rustup install $RUST_NIGHTLY_VERSION && \
  rustup component add clippy --toolchain=$RUST_NIGHTLY_VERSION && \
  rustup component add rustfmt --toolchain=$RUST_NIGHTLY_VERSION && \
  rustup target add wasm32-unknown-unknown && \
  cargo install cargo-audit && \
  cargo install cargo-hack && \
  cargo install cargo-sort && \
  cargo install mdbook && \
  cargo install mdbook-linkcheck && \
  cargo install svgbob_cli && \
  cargo install wasm-pack && \
  cargo install rustfilt && \
  rustup show && \
  rustc --version && \
  cargo --version && \
  chmod -R a+w $CARGO_HOME $RUSTUP_HOME && \
  rm -rf $CARGO_HOME/registry && \
  # sccache
  curl -LOsS "https://github.com/mozilla/sccache/releases/download/$SCCACHE_VERSION/sccache-$SCCACHE_VERSION-x86_64-unknown-linux-musl.tar.gz" && \
  tar -xzf "sccache-$SCCACHE_VERSION-x86_64-unknown-linux-musl.tar.gz" && \
  mv "sccache-$SCCACHE_VERSION-x86_64-unknown-linux-musl"/sccache "$CARGO_HOME/bin/" && \
  rm "sccache-$SCCACHE_VERSION-x86_64-unknown-linux-musl.tar.gz" && \
  rm -rf "sccache-$SCCACHE_VERSION-x86_64-unknown-linux-musl" && \
  # nextest
  curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C "$CARGO_HOME/bin" && \
  # golang
  curl -LOsS "https://golang.org/dl/go$GOLANG_VERSION.linux-amd64.tar.gz" && \
  tar -C /usr/local -xzf "go$GOLANG_VERSION.linux-amd64.tar.gz" && \
  rm "go$GOLANG_VERSION.linux-amd64.tar.gz" && \
  # nodejs
  sudo mkdir -p /etc/apt/keyrings && \
  curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key | sudo gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg && \
  echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_$NODE_MAJOR.x nodistro main" | sudo tee /etc/apt/sources.list.d/nodesource.list && \
  sudo apt-get update && \
  sudo apt-get install -y nodejs && \
  # setup path
  mkdir /.cache && \
  chmod -R a+w /.cache && \
  mkdir /.config && \
  chmod -R a+w /.config && \
  mkdir /.npm && \
  chmod -R a+w /.npm && \
  # grcov
  curl -LOsS "https://github.com/mozilla/grcov/releases/download/$GRCOV_VERSION/grcov-x86_64-unknown-linux-musl.tar.bz2" && \
  tar -xf grcov-x86_64-unknown-linux-musl.tar.bz2 && \
  mv ./grcov $CARGO_HOME/bin && \
  rm grcov-x86_64-unknown-linux-musl.tar.bz2 && \
  # codecov
  curl -Os https://uploader.codecov.io/latest/linux/codecov && \
  chmod +x codecov && \
  mv codecov /usr/bin && \
  # clean lists
  rm -rf /var/lib/apt/lists/*
