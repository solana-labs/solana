---
title: Solana Validator Requirements
sidebar_position: 3
sidebar_label: Requirements
pagination_label: Requirements to Operate a Validator
---

## Minimum SOL requirements

There is no strict minimum amount of SOL required to run a validator on Solana.

However in order to participate in consensus, a vote account is required which
has a rent-exempt reserve of 0.02685864 SOL. Voting also requires sending a vote
transaction for each block the validator agrees with, which can cost up to
1.1 SOL per day.

## Hardware Recommendations

The hardware recommendations below are provided as a guide.  Operators are encouraged to do their own performance testing.

| Component | Validator Requirements | Additional RPC Node Requirements |
|-----------|------------------------|----------------------------------|
| **CPU**   | - 2.8GHz base clock speed, or faster<br />- SHA extensions instruction support<br />- AMD Gen 3 or newer<br />- Intel Ice Lake or newer<br />- Higher clock speed is preferable over more cores<br />- AVX2 instruction support (to use official release binaries, self-compile otherwise)<br />- Support for AVX512f is helpful<br />||
| | 12 cores / 24 threads, or more  | 16 cores / 32 threads, or more |
| **RAM**   | Error Correction Code (ECC) memory is suggested<br />Motherboard with 512GB capacity suggested ||
| | 256GB or more| 512 GB or more for **all [account indexes](https://docs.solanalabs.com/operations/setup-an-rpc-node#account-indexing)** |
| **Disk**  | PCIe Gen3 x4 NVME SSD, or better, on each of: <br />- **Accounts**: 500GB, or larger. High TBW (Total Bytes Written)<br />- **Ledger**: 1TB or larger. High TBW suggested<br />- **Snapshots**: 250GB or larger. High TBW suggested<br />- **OS**: (Optional) 500GB, or larger. SATA OK<br /><br />The OS may be installed on the ledger disk, though testing has shown better performance with the ledger on its own disk<br /><br />Accounts and ledger *can* be stored on the same disk, however due to high IOPS, this is not recommended<br /><br />The Samsung 970 and 980 Pro series SSDs are popular with the validator community | Consider a larger ledger disk if longer transaction history is required<br /><br />Accounts and ledger **should not** be stored on the same disk |
| **GPUs**  | Not necessary at this time<br />Operators in the validator community do not use GPUs currently | |


## Virtual machines on Cloud Platforms

Running a solana node in the cloud requires significantly greater
operational expertise to achieve stability and performance. Do not
expect to find sympathetic voices should you chose this route and
find yourself in need of support.

## Docker

Running validator for live clusters (including mainnet-beta) inside Docker is
not recommended and generally not supported. This is due to concerns of general
Docker's containerization overhead and resultant performance degradation unless
specially configured.

We use Docker only for development purposes. Docker Hub contains images for all
releases at [solanalabs/solana](https://hub.docker.com/r/solanalabs/solana).

## Software

- We build and run on Ubuntu 20.04.
- See [Installing Solana CLI](../cli/install.md) for the current Solana software release.

Prebuilt binaries are available for Linux x86_64 on CPUs supporting AVX2 \(Ubuntu 20.04 recommended\).
MacOS or WSL users may build from source.

## Networking
Internet service should be at least 1GBbit/s symmetric, commercial. 10GBit/s preferred (especially for mainnet-beta).

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