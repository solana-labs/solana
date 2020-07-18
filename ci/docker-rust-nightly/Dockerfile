<<<<<<< HEAD
FROM solanalabs/rust:1.43.0
=======
FROM solanalabs/rust:1.45.0
>>>>>>> 83cc92f3f... Upgade to Rust 1.45.0
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
