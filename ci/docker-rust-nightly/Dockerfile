FROM solanalabs/rust
ARG date

RUN set -x \
 && rustup install nightly-$date \
 && rustup show \
 && rustup show \
 && rustc --version \
 && cargo --version \
 && rustc +nightly-$date --version \
 && cargo +nightly-$date --version

