# Note: when the rust version is changed also modify
# ci/buildkite.yml to pick up the new image tag
FROM rust:1.34.0

RUN set -x \
 && apt update \
 && apt-get install apt-transport-https \
 && echo deb https://apt.buildkite.com/buildkite-agent stable main > /etc/apt/sources.list.d/buildkite-agent.list \
 && echo deb http://apt.llvm.org/stretch/ llvm-toolchain-stretch-7 main > /etc/apt/sources.list.d/llvm.list \
 && apt-key adv --no-tty --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys 32A37959C2FA5C3C99EFBC32A79206696452D198 \
 && wget -O - https://apt.llvm.org/llvm-snapshot.gpg.key | apt-key add - \
 && apt update \
 && apt install -y \
      buildkite-agent \
      clang-7 \
      cmake \
      lcov \
      libclang-common-7-dev \
      llvm-7 \
      mscgen \
      rsync \
      sudo \
      \
 && rustup component add rustfmt \
 && rustup component add clippy \
 && rm -rf /var/lib/apt/lists/* \
 && rustc --version \
 && cargo --version

