# Use a base image
FROM ubuntu:latest

# Install necessary software
RUN apt update && apt install -y curl vim bzip2

# Install Solana and set PATH
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.16.5/install)"
ENV PATH="${PATH}:/root/.local/share/solana/install/active_release/bin"

# Copy the local folder into the image
COPY ./workspace /workspace

# Set the working directory
WORKDIR /workspace

# Run your application or set other instructions
CMD ["./start-validator.sh"]

