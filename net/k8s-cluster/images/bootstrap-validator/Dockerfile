# Use a base image
# FROM ubuntu:20.04
FROM ubuntu:latest
# RUN echo "heyasdasdasd"
# Install necessary software
RUN apt update 
RUN apt install -y iputils-ping curl vim bzip2 
#psmisc iproute2 software-properties-common apt-transport-https ca-certificates openssh-client openssh-server

# # Install Solana and set PATH
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.16.5/install)"
ENV PATH="${PATH}:/root/.local/share/solana/install/active_release/bin"

# # Copy the local folder into the image
# COPY ./workspace /home/solana/workspace
COPY ./workspace /workspace

# RUN useradd -ms /bin/bash solana
# RUN adduser solana sudo
# USER solana
# RUN echo "hey"

# COPY ./fetch-perf-libs.sh ./fetch-spl.sh ./scripts ./net ./multinode-demo /home/solana/


# Set the working directory
# WORKDIR /home/solana
WORKDIR /workspace 


# Run your application or set other instructions
# CMD ["/bin/sleep", "3650d"]
