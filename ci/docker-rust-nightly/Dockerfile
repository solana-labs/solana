FROM solanalabs/rust
ARG nightly=nightly-2018-09-03

RUN set -x && \
    rustup install $nightly && \
    rustup default $nightly && \
    rustup component add clippy-preview --toolchain=$nightly && \
    rustc --version && \
    cargo --version && \
    cargo +$nightly install cargo-cov
