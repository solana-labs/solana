---
title: Safecoin Test Validator
---
During early stage development, it is often convenient to target a cluster with
fewer restrictions and more configuration options than the public offerings
provide. This is easily achieved with the `safecoin-test-validator` binary, which
starts a full-featured, single-node cluster on the developer's workstation.

## Advantages
- No RPC rate-limits
- No airdrop limits
- Direct [on-chain program](on-chain-programs/overview) deployment
(`--bpf-program ...`)
- Clone accounts from a public cluster, including programs (`--clone ...`)
- Configurable transaction history retention (`--limit-ledger-size ...`)
- Configurable epoch length (`--slots-per-epoch ...`)
- Jump to an arbitrary slot (`--warp-slot ...`)

## Installation
The `safecoin-test-validator` binary ships with the Safecoin CLI Tool Suite.
[Install](/cli/install-solana-cli-tools) before continuing.

## Running
First take a look at the configuration options
```
safecoin-test-validator --help
```

Next start the test validator
```
safecoin-test-validator
```

By default, basic status information is printed while the process is running.
See [Appendix I](#appendix-i-status-output) for details
```
Ledger location: test-ledger
Log: test-ledger/validator.log
Identity: EPhgPANa5Rh2wa4V2jxt7YbtWa3Uyw4sTeZ13cQjDDB8
Genesis Hash: 4754oPEMhAKy14CZc8GzQUP93CB4ouELyaTs4P8ittYn
Version: 1.6.7
Shred Version: 13286
Gossip Address: 127.0.0.1:1024
TPU Address: 127.0.0.1:1027
JSON RPC URL: http://127.0.0.1:8328
⠈ 00:36:02 | Processed Slot: 5142 | Confirmed Slot: 5142 | Finalized Slot: 5110 | Snapshot Slot: 5100 | Transactions: 5142 | ◎499.974295000
```

Leave `safecoin-test-validator` running in its own terminal. When it is no longer
needed, it can be stopped with ctrl-c.

## Interacting
Open a new terminal to interact with a [running](#running) `safecoin-test-validator`
instance using other binaries from the Safecoin CLI Tool Suite or your own client
software.

#### Configure the CLI Tool Suite to target a local cluster by default
```
safecoin config set --url http://127.0.0.1:8328
```

#### Verify the CLI Tool Suite configuration
```
safecoin genesis-hash
```
* **NOTE:** The result should match the `Genesis Hash:` field in the
`safecoin-test-validator` status output

#### Check the wallet balance
```
safecoin balance
```
* **NOTE:** `Error: No such file or directory (os error 2)` means that the default
wallet does not yet exist. Create it with `safecoin-keygen new`.
* **NOTE:** If the wallet has a zero SAFE balance, airdrop some localnet SAFE with
`safecoin airdrop 10`

#### Perform a basic transfer transaction
```
safecoin transfer EPhgPANa5Rh2wa4V2jxt7YbtWa3Uyw4sTeZ13cQjDDB8 1
```

#### Monitor `msg!()` output from on-chain programs
```
safecoin logs
```
* **NOTE:** This command needs to be running when the target transaction is
executed. Run it in its own terminal

## Appendix I: Status Output
```
Ledger location: test-ledger
```
* File path of the ledger storage directory. This directory can get large. Store
less transaction history with `--limit-ledger-size ...` or relocate it with
`--ledger ...`

```
Log: test-ledger/validator.log
```
* File path of the validator text log file. The log can also be streamed by
passing `--log`. Status output is suppressed in this case.

```
Identity: EPhgPANa5Rh2wa4V2jxt7YbtWa3Uyw4sTeZ13cQjDDB8
```
* The validator's identity in the [gossip network](/validator/gossip#gossip-overview)

```
Version: 1.6.7
```
* The software version

```
Gossip Address: 127.0.0.1:1024
TPU Address: 127.0.0.1:1027
JSON RPC URL: http://127.0.0.1:8328
```
* The network address of the [Gossip](/validator/gossip#gossip-overview),
[Transaction Processing Unit](/validator/tpu) and [JSON RPC](clients/jsonrpc-api#json-rpc-api-reference)
service, respectively

```
⠈ 00:36:02 | Processed Slot: 5142 | Confirmed Slot: 5142 | Finalized Slot: 5110 | Snapshot Slot: 5100 | Transactions: 5142 | ◎499.974295000
```
* Session running time, current slot of the the three block
[commitment levels](clients/jsonrpc-api#configuring-state-commitment),
slot height of the last snapshot, transaction count,
[voting authority](/running-validator/vote-accounts#vote-authority) balance
