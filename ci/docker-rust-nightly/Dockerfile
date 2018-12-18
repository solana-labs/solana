FROM solanalabs/rust
ARG date

RUN set -x \
 && rustup install nightly-$date \
 && rustup show \
 && mv /usr/local/rustup/toolchains/nightly-$date-* \
       /usr/local/rustup/toolchains/nightly-x86_64-unknown-linux-gnu \
 && rustup show \
 && rustc --version \
 && cargo --version \
 && rustc +nightly --version \
 && cargo +nightly --version

