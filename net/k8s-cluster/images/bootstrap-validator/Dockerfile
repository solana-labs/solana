# Use a base image
FROM ubuntu:20.04
# FROM ubuntu:latest
# RUN echo "heyasdasdasd"
# Install necessary software
RUN apt update 
RUN apt install -y iputils-ping curl vim bzip2 psmisc \
    iproute2 software-properties-common apt-transport-https \
    ca-certificates openssh-client openssh-server

# runs source common.sh
#   creates solana/config locally
#   runs source scripts/configure-metrics.sh # requires '$SOLANA_METRICS_CONFIG' to be set. pass in as flags in rust
# 'prepareDeploy()'
#   download tar/build image locally
# 'deploy()'
#   'initLogDir'
#       create netLogDir (stores all of the logs. this is created locally)
#   'startBootstrapLeader()'
#       'startCommon()'
#           gets ~/home of remote node (we can just set this to /home/solana for all pods)
#           create a ~/.cargo/.bin folder
#           'syncScripts()'
#               COPY ./fetch-perf-libs.sh ./fetch-spl.sh ./scripts ./net ./multinode-demo /home/solana/ 
#       'deployBootstrapValidator()'
#           if method is 'tar':
#               copy SOLANA_ROOT/solana-release/bin/* into containers' /home/solana/.cargo/bin/ folder
#               copy SOLANA_ROOT/solana-release/version.yml into containers' /home/solana/ folder (aka $HOME)
#           if method is 'local':
#               copy SOLANA_ROOT/farf/bin/* into containers' /home/solana/.cargo/bin/ folder
#               copy SOLANA_ROOT/farf/version.yml into containers' /home/solana/ folder (aka $HOME)              
#           if method is 'skip', continue. if method is anything else internal error.
#       call remote-node.sh with hella flags
# remote-node.sh
#   if bootstrap-validator
#       create a home/solana/config directory
#       we'll initially create brand new keys. aka solana-keygen... see line 153 in remote-node.sh


# # # Install Solana and set PATH
# RUN sh -c "$(curl -sSfL https://release.solana.com/v1.16.5/install)"
# ENV PATH="${PATH}:/root/.local/share/solana/install/active_release/bin"

# # Copy the local folder into the image
COPY ./net/k8s-cluster/images/workspace /home/solana/workspace
# COPY ./workspace /workspace

RUN useradd -ms /bin/bash solana
RUN adduser solana sudo
USER solana
# RUN echo "hey"

# this is basically `syncScripts()''
COPY ./fetch-perf-libs.sh ./fetch-spl.sh ./scripts ./net ./multinode-demo /home/solana/
RUN mkdir -p /home/solana/.cargo/bin

ARG deploy_method
RUN if [ -z "$build_method" ]; then \
        echo "build_method not set! exiting..." \
        exit 1; \
    fi

RUN if [ "$deploy_method" = "tar" ]; then \
        echo "Copying tar release to dockerfile"; \
        # do the copying here \
    elif [ "$deploy_method" = "local" ]; then \
        echo "Copying local build here"; \
        # do copying here   \
    elif [ "$deploy_method" = "skip" ]; then \
        echo "Skipping copy..." \
    else \ 
        echo "ERROR!!!"
    fi



# Set the working directory
WORKDIR /home/solana
# WORKDIR /workspace 


# Run your application or set other instructions
CMD ["/bin/sleep", "3650d"]
