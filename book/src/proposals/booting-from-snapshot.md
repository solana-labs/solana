# Booting From a Snapshot

## Problem

A validator booting from a snapshot should confirm that they are caught up to the correct cluster before voting. There are two major risks if they do not:

### Wrong Cluster

The validator may be fed a snapshot and gossip info for a different (though not necessarily malicious) cluster. If that cluster is at slot 100,000 while the cluster the validator intended to join is at 10,000, if the validator votes once on the wrong cluster, it is locked out of the correct cluster for 90,000 slots.

### Voting Before Catching Up

Any vote that a validator signs must have its lockout observed. If a validator has to reboot, the most secure way for it to check if its lockouts apply to the new set of forks is to see the votes on the ledger. If it can do this, it knows that those votes do not lock it out of voting on the current fork, so it just needs to wait for the lockouts of the votes not on the ledger to expire. However, if the validator votes before catching up, the votes will not go onto the ledger, so if the validator reboots, it will have to assume that the votes lock it out from voting again.

## Solution

A booting validator needs to have some set of trusted validators whose votes it will rely upon to ensure that it has caught up to the cluster and has a valid snapshot. With that set, the validator can

1. Get a snapshot to boot from. This can come from any source, as it will be verified before being vote upon.

2. Periodically send canary transactions with its local recent blockhash.

3. While waiting to see one of the canary transactions, set a new root every time some threshold percent of the trusted validator stake roots a bank.

4. Wait to observe a canary transaction in a bank that some threshold of trusted validator stake has voted on.

5. Confirm that the `trusted validator root` > `snapshot root`

6. Figure out what banks the validator is not locked out from. This is not currently done, and should be addressed separately, but is not necessary for this design.

7. Start voting.
