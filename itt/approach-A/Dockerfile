FROM rust:latest

WORKDIR /usr/src/solana

# some dependencies
RUN apt-get update && apt-get install -y libudev-dev apt-utils clang gcc make

# clone solana code
RUN git clone https://github.com/solana-labs/solana.git

# checkout latest release, run cargo build
RUN cd solana && \
  TAG=$(git describe --tags $(git rev-list --tags --max-count=1)) && \
  git checkout $TAG && \
  cargo build --release

# run setup
RUN cd solana && \
  NDEBUG=1 ./multinode-demo/setup.sh

# copy wrapper script
COPY solana_itt_script.sh solana/solana_itt_script.sh

# setting new context
WORKDIR solana

# run wrapper script
CMD ./solana_itt_script.sh

