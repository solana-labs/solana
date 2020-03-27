# Note: when the rust version is changed also modify
# ci/rust-version.sh to pick up the new image tag
FROM rust:1.42.0

# Add Google Protocol Buffers for Libra's metrics library.
ENV PROTOC_VERSION 3.8.0
ENV PROTOC_ZIP protoc-$PROTOC_VERSION-linux-x86_64.zip

RUN set -x \
 && apt update \
 && apt-get install apt-transport-https \
 && echo deb https://apt.buildkite.com/buildkite-agent stable main > /etc/apt/sources.list.d/buildkite-agent.list \
 && apt-key adv --no-tty --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys 32A37959C2FA5C3C99EFBC32A79206696452D198 \
 && apt update \
 && apt install -y \
      buildkite-agent \
      clang-7 \
      cmake \
      lcov \
      libudev-dev \
      libclang-common-7-dev \
      mscgen \
      net-tools \
      rsync \
      sudo \
      golang \
      unzip \
      \
 && rm -rf /var/lib/apt/lists/* \
 && rustup component add rustfmt \
 && rustup component add clippy \
 && cargo install cargo-audit \
 && cargo install svgbob_cli \
 && cargo install mdbook \
 && cargo install mdbook-linkcheck \
 && rustc --version \
 && cargo --version \
 && curl -OL https://github.com/google/protobuf/releases/download/v$PROTOC_VERSION/$PROTOC_ZIP \
 && unzip -o $PROTOC_ZIP -d /usr/local bin/protoc \
 && unzip -o $PROTOC_ZIP -d /usr/local include/* \
 && rm -f $PROTOC_ZIP
