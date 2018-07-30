FROM rustlang/rust:nightly

RUN cargo install --force clippy cargo-cov && \
    echo deb http://ftp.debian.org/debian stretch-backports main >> /etc/apt/sources.list && \
    apt update && \
    apt install -y \
      llvm-6.0
      && \
    rm -rf /var/lib/apt/lists/*
