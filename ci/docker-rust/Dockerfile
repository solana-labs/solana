FROM rust:1.28

RUN set -x && \
    apt update && \
    apt-get install apt-transport-https && \
    echo deb https://apt.buildkite.com/buildkite-agent stable main > /etc/apt/sources.list.d/buildkite-agent.list && \
    apt-key adv --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys 32A37959C2FA5C3C99EFBC32A79206696452D198 && \
    apt update && \
    apt install -y \
      buildkite-agent \
      cmake \
      llvm-6.0 \
      rsync \
      sudo \
      && \
    rustup component add rustfmt-preview && \
    rm -rf /var/lib/apt/lists/* && \
    rustc --version && \
    cargo --version
