<p align="center">
  <a href="https://solana.com">
    <img alt="Solana" src="https://i.imgur.com/IKyzQ6T.png" width="250" />
  </a>
</p>   

Solana - High-Performance Blockchain

Solana is a fast, secure, and scalable blockchain network designed for decentralized applications and crypto transactions.


---

Building from Source

1. Install Rust Toolchain

curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env
rustup component add rustfmt

Ensure you are using the latest stable Rust version:

rustup update

If building a specific release branch, check ci/rust-version.sh and install the correct version:

rustup install VERSION

Linux Dependencies

On Ubuntu:

sudo apt-get update
sudo apt-get install libssl-dev libudev-dev pkg-config zlib1g-dev llvm clang cmake make libprotobuf-dev protobuf-compiler

On Fedora:

sudo dnf install openssl-devel systemd-devel pkg-config zlib-devel llvm clang cmake make protobuf-devel protobuf-compiler perl-core

2. Clone the Repository

git clone https://github.com/solana-labs/solana.git
cd solana

3. Build the Project

./cargo build --release


---

Testing & Benchmarking

Run the test suite:

./cargo test

Run benchmarks:

rustup install nightly
cargo +nightly bench

Start a Local Testnet

Instructions for running a local testnet are available in the documentation.


---

Deployment

1. Setting Up the Environment

Ensure you have Docker installed:

sudo apt install docker.io

Pull the latest Solana Docker image:

docker pull solanalabs/solana:latest

Run the container:

docker run -it solanalabs/solana:latest

2. Deploying a Smart Contract

Compile the smart contract:

solana program deploy path/to/your_program.so

Check deployment status:

solana program show YOUR_PROGRAM_ID

3. Running a Validator Node

Initialize the validator:

solana-validator --identity ~/.config/solana/id.json --ledger ~/solana-ledger

Monitor logs:

tail -f ~/solana-ledger/validator.log


---

Release Process

The release process for this project is described in RELEASE.md.

Code Coverage

Generate code coverage statistics:

scripts/coverage.sh
open target/cov/lcov-local/index.html


---

Disclaimer

All content, specifications, and performance measurements described in this project are based on Solana Labs' best efforts. Users should independently verify their accuracy and compliance with applicable regulations.


---
