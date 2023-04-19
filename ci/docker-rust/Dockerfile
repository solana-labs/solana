# Note: when the rust version is changed also modify
# ci/rust-version.sh to pick up the new image tag
FROM rust:1.68.2

RUN set -x \
 && apt update \
 && apt-get install apt-transport-https \
 && echo deb https://apt.buildkite.com/buildkite-agent stable main > /etc/apt/sources.list.d/buildkite-agent.list \
 && apt-key adv --no-tty --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys 32A37959C2FA5C3C99EFBC32A79206696452D198 \
 && curl -fsSL https://deb.nodesource.com/setup_current.x | bash - \
 && apt update \
 && apt install -y \
      buildkite-agent \
      clang \
      cmake \
      lcov \
      libudev-dev \
      mscgen \
      nodejs \
      net-tools \
      rsync \
      sudo \
      golang \
      unzip \
      \
 && apt remove -y libcurl4-openssl-dev \
 && rm -rf /var/lib/apt/lists/* \
 && node --version \
 && npm --version \
 && rustup component add rustfmt \
 && rustup component add clippy \
 && rustup target add wasm32-unknown-unknown \
 && cargo install cargo-audit \
 && cargo install cargo-sort \
 && cargo install mdbook \
 && cargo install mdbook-linkcheck \
 && cargo install svgbob_cli \
 && cargo install --git https://github.com/rustwasm/wasm-pack --rev b4e619c8a13a8441b804895348afbfd4fb1a68a3 \
 && rustc --version \
 && cargo --version
