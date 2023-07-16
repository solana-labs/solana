---
title: Validator Requirements
---

## Minimum SOL requirements

There is no strict minimum amount of SOL required to run a validator on Solana.

However in order to participate in consensus, a vote account is required which
has a rent-exempt reserve of 0.02685864 SOL. Voting also requires sending a vote
transaction for each block the validator agrees with, which can cost up to
1.1 SOL per day.

## Hardware Recommendations

The hardware recommendations below are provided as a guide.  Operators are encouraged to do their own performance testing.

- CPU
  - 12 cores / 24 threads, or more
  - 2.8GHz base clock speed, or faster
  - SHA extensions instruction support
    - AMD Gen 3 or newer
    - Intel Ice Lake or newer
  - AVX2 instruction support (to use official release binaries, self-compile
    otherwise)
  - Support for AVX512f is helpful
- RAM
  - 256GB or more
  - Error Correction Code (ECC) memory is suggested
  - Motherboard with 512GB capacity suggested
- Disk
  - PCIe Gen3 x4 NVME SSD, or better
  - Accounts: 500GB, or larger. High TBW (Total Bytes Written)
  - Ledger: 1TB or larger. High TBW suggested
  - OS: (Optional) 500GB, or larger. SATA OK
  - The OS may be installed on the ledger disk, though testing has shown better
    performance with the ledger on its own disk
  - Accounts and ledger _can_ be stored on the same disk, however due to high
    IOPS, this is not recommended
  - The Samsung 970 and 980 Pro series SSDs are popular with the validator community
- GPUs
  - Not necessary at this time
  - Operators in the validator community do no use GPUs currently

### RPC Node Recommendations

The [hardware recommendations](#hardware-recommendations) above should be considered
bare minimums if the validator is intended to be employed as an RPC node. To provide
full functionality and improved reliability, the following adjustments should be
made.

- CPU
  - 16 cores / 32 threads, or more
- RAM
  - 512 GB or more if `account-index` is used
- Disk
  - Consider a larger ledger disk if longer transaction history is required
  - Accounts and ledger should not be stored on the same disk

## Virtual machines on Cloud Platforms

While you can run a validator on a cloud computing platform, it may not
be cost-efficient over the long term.

However, it may be convenient to run non-voting api nodes on VM instances for
your own internal usage. This use case includes exchanges and services built on
Solana.

In fact, the mainnet-beta validators operated by the team are currently
(Mar. 2021) run on GCE `n2-standard-32` (32 vCPUs, 128 GB memory) instances with
2048 GB SSD for operational convenience.

For other cloud platforms, select instance types with similar specs.

Also note that egress internet traffic usage may turn out to be high,
especially for the case of running staked validators.

## Docker

Running validator for live clusters (including mainnet-beta) inside Docker is
not recommended and generally not supported. This is due to concerns of general
Docker's containerization overhead and resultant performance degradation unless
specially configured.

We use Docker only for development purposes. Docker Hub contains images for all
releases at [solanalabs/solana](https://hub.docker.com/r/solanalabs/solana).

## Software

- We build and run on Ubuntu 20.04.
- See [Installing Solana](../cli/install-solana-cli-tools.md) for the current Solana software release.

Prebuilt binaries are available for Linux x86_64 on CPUs supporting AVX2 \(Ubuntu 20.04 recommended\).
MacOS or WSL users may build from source.

## Networking
Internet service should be at least 1GBbit/s symmetric, commercial. 10GBit/s preferred.

### Port Forwarding
The following ports need to be open to the internet for both inbound and outbound

It is not recommended to run a validator behind a NAT. Operators who choose to
do so should be comfortable configuring their networking equipment and debugging
any traversal issues on their own.

#### Required
- 8000-10000 TCP/UDP - P2P protocols (gossip, turbine, repair, etc). This can
be limited to any free 13 port range with `--dynamic-port-range`

#### Optional
For security purposes, it is not suggested that the following ports be open to
the internet on staked, mainnet-beta validators.
- 8899 TCP - JSONRPC over HTTP. Change with `--rpc-port RPC_PORT``
- 8900 TCP - JSONRPC over Websockets. Derived. Uses `RPC_PORT + 1`