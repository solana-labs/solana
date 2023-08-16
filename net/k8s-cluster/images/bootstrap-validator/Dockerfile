# Use a base image
FROM ubuntu:20.04

# Install necessary software
RUN apt update && apt install -y iputils-ping curl vim bzip2 psmisc iproute2 software-properties-common apt-transport-https ca-certificates openssh-client openssh-server

# # Install Solana and set PATH
# RUN sh -c "$(curl -sSfL https://release.solana.com/v1.16.5/install)"
# ENV PATH="${PATH}:/root/.local/share/solana/install/active_release/bin"

# Copy the local folder into the image
COPY ./workspace /workspace

COPY ./fetch-perf-libs ./fetch-spl.sh ./scripts ./net ./multinode-demo /home/sol/

# Set the working directory
WORKDIR /home/sol

# Run your application or set other instructions
CMD ["/bin/sleep", "3650d"]
