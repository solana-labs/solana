FROM rustlang/rust:nightly

RUN rustup component add clippy-preview --toolchain=nightly && \
    echo deb http://ftp.debian.org/debian stretch-backports main >> /etc/apt/sources.list && \
    apt update && \
    apt install -y \
      llvm-6.0 \
      && \
    rm -rf /var/lib/apt/lists/*
