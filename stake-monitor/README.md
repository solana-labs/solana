## Overview
`solana-stake-monitor` is a utility that scans all transactions to ensure that stake accounts remain in compliance with the following rules:

1. The stake account must be created after genesis
1. The "compliant balance" of a stake account is set upon stake account initialization, system transfers of additional funds into a compliant stake account are excluded from the "compliant balance"
1. The stake account cannot have a lockup or custodian
1. Withdrawing funds from the stake account trigger non-compliance
1. Stake accounts split from a compliant stake account remain compliant, and the "compliant balance" is adjusted accordingly for the original stake account

In terms of `solana` command-line subcommands:
* `create-stake-account`: Creates a compliant stake account provided the `--lockup-date`, `--lockup-epoch`, or `--custodian` options are not specified
* `delegate-stake` / `deactivate-stake` / `stake-authorize` / `split-stake`: These commands do not affect compliance
* `withdraw-stake` / `stake-set-lockup`: These commands will cause non-compliance
* `transfer`:  Any additional funds transferred after `create-stake-account` are excluded from the "compliant balance"
