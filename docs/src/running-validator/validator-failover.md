---
title: Failover Setup
---

A simple two machine instance failover method is described here, which allows you to:
* upgrade your validator software with virtually no down time, and
* failover to the secondary instance when your monitoring detects a problem with
  the primary instance
without any safety issues that would otherwise be associated with running two
instances of your validator.

In order to begin you will need two machine instances, for your primary and
secondary validator. It's assumed that both these machine instances are already
configured and able to run a single validator

## Setup

## Primary Validator
The only additional `solana-validator` flag required is `--acquire-node-instance-then-vote`.

## Secondary Validator
Configure the secondary validator like the primary with the exception of the
following `solana-validator` command-line argument changes:
* Use a secondary validator identity: `--identity secondary-validator-keypair.json`
* Add `--no-check-vote-account`
* Add `--authorized-voter validator-keypair.json` (where
  `validator-keypair.json` is the identity keypair for your primary validator)

## Triggering a failover manually
When both validators are running normally and caught up to the cluster, a
failover from primary to secondary can be triggered by running the following
command on the secondary validator:
```bash
$ solana-validator wait-for-restart-window --identity validator-keypair.json \
  && solana-validator set-identity validator-keypair.json
```

The secondary validator will then perform on-chain coordination to ensure voting
and block production safely switches over from the primary validator.

The primary validator will terminate as soon as it detects the secondary
validator with its identity.

Note: When the primary validator restarts (which may be immediate if you have
configured your primary validator to do so) it will reclaim its identity
from the secondary validator. This will in turn cause the secondary validator to
exit. However if/when the secondary validator restarts, it will do so using the
secondary validator identity and thus the restart cycle is broken.

## Triggering a failover via monitoring
Monitoring of your choosing can invoke the `solana-validator set-identity
validator-keypair.json` command mentioned in the previous section.

It is not necessary to guarantee the primary validator has halted before failing
over to the secondary, as the failover process will prevent the primary
validator from voting and producing blocks even if it is in an unknown state.

## Validator Software Upgrades
To perform a software upgrade using this failover method:
1. Install the new software version on your primary validator system but do not
   restart it yet.
2. Trigger a manual failover to your secondary validator. This should cause your
   primary validator to terminate.
3. When your primary validator restarts it will now be using the new software version.
4. Once the primary validator catches up upgrade the secondary validator at
   your convenience.
