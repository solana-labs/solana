FROM rust:latest

ARG VAR_A=1
ENV NDEBUG=$VAR_A

ARG VAR_B=1001
ENV USER_ID=$VAR_B

ARG VAR_C=1001
ENV GROUP_ID=$VAR_C

# switch user
USER root

# some dependencies
RUN apt-get update && apt-get install -y sudo apt-utils libudev-dev clang gcc make lolcat toilet toilet-fonts tree

# add group
RUN addgroup --gid $GROUP_ID solana

# add user
RUN adduser --disabled-password --gecos 'solana' --uid $USER_ID --gid $GROUP_ID solana && \
  usermod -aG sudo solana

RUN mkdir -p /tmp/solana/itt && \
  chmod -R +w /tmp/solana

WORKDIR /home/solana

# clone solana code
RUN git clone https://github.com/solana-labs/solana.git

# checkout latest release, run cargo build
RUN cd solana && \
  TAG=$(git describe --tags $(git rev-list --tags --max-count=1)) && \
  git checkout $TAG && \
  cargo build --release

# run setup
RUN cd solana && \
  NDEBUG=$NDEBUG ./multinode-demo/setup.sh

# copy wrapper script
COPY --chown=$USER_ID:$GROUP_ID solana_itt_script.sh solana/solana_itt_script.sh

# add +x
RUN chmod +x solana/solana_itt_script.sh

# setting new context
WORKDIR solana

# run wrapper script
CMD ./solana_itt_script.sh