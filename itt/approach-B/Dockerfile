FROM rust:latest

WORKDIR /usr/src/solana

# some dependencies
RUN apt-get update && apt-get install -y libudev-dev apt-utils clang gcc make supervisor

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

# copy configuration file
COPY supervisord.conf /etc/supervisor/conf.d/supervisord.conf

# setting new context and exposing ports
WORKDIR solana
EXPOSE 8000-9000

# execute supervisord
CMD ["/usr/bin/supervisord"]
