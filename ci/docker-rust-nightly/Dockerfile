FROM solanalabs/rust

RUN set -x && \
    rustup install nightly && \
    rustup default nightly && \
    rustup component add clippy-preview --toolchain=nightly && \
    rustc --version && \
    cargo --version
