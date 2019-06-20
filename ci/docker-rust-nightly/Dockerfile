FROM solanalabs/rust
ARG date

RUN set -x \
 && rustup install nightly-$date \
 && rustup component add clippy --toolchain=nightly-$date \
 && rustup show \
 && rustc --version \
 && cargo --version \
 && cargo install grcov \
 && rustc +nightly-$date --version \
 && cargo +nightly-$date --version
