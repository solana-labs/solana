FROM solanalabs/rust:1.74.0

ARG date
ARG GRCOV_VERSION=v0.8.18

RUN \
  rustup install nightly-$date && \
  rustup component add clippy --toolchain=nightly-$date && \
  rustup component add rustfmt --toolchain=nightly-$date && \
  rustup show && \
  rustc --version && \
  cargo --version && \
  # grcov
  curl -LOsS "https://github.com/mozilla/grcov/releases/download/$GRCOV_VERSION/grcov-x86_64-unknown-linux-musl.tar.bz2" && \
  tar -xf grcov-x86_64-unknown-linux-musl.tar.bz2 && \
  mv ./grcov $CARGO_HOME/bin && \
  rm grcov-x86_64-unknown-linux-musl.tar.bz2 && \
  # codecov
  curl -Os https://uploader.codecov.io/latest/linux/codecov && \
  chmod +x codecov && \
  mv codecov /usr/bin
