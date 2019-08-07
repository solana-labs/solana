# Slashing

Proof of Stake consensus requires that Byzantine behavior be punished by loss of stake and rewards, also known as slashing. 
Solana implements slashing in 2 regimes: the validator regime and the replicator regime, corresponding to the two rewards regimes.
During slashing, the network destroys some "penalty percentage" of lamports staked/earned in the rewards-generating accounts.

## Validator regime

Validators cast their votes to a Vote account to which stake has been [passively delegated](stake-delegation-and-rewards.md).
The Vote program keeps track of rewards for all of the stakes, which allows it to be a central point for slashing.
The basic mechanism of slashing is:

1. a proof of Byzantine behavior is submitted to the Vote program
2. the Vote program marks the Vote account as "slashed", which means:
    * no further votes may be submitted to this account
    * the slashing penalty percentage of the lamports in this account are forfeit
3. any further access of any delegated Stake accounts will see they are also "slashed"

Vote accounts thus slashed cannot be recovered, 

Since any entity on the network can submit a rewards redemption instruction to a Stake account, stakes can be 
slashed at the network's leisure.  The lamports in a Stake or Vote account cannot be reclaimed from those 
accounts without some access, so the lamports are effectively destroyed if no slashing transactions are 
ever sent to the Stake program.

### Byzantine behavior (i.e. slashing proofs)

#### Vote transaction as proof

Some proofs of a conflicting vote can be as simple as a Vote transaction, because the Vote account keeps track
of some number of previously submitted votes:
1. two votes at the same slot with differing bank hashes
2. a lockout violation 
3. two votes with conflicting ancestors

#### Other proofs

Some proofs of Byzantine behavior require more data.
1. conflicting transmission: 2 blobs or 2 shreds that are signed by a validator as leader that have the same slot and index, 
but differing signatures.  The proof comprises `S(slot, index, hash(data)) + slot, index, hash(data)` for each of the conflicting blobs.
2. ..

### Slashing's implications on network

#### Consensus

The network removes slashed stakes from the consensus calculations.  A voting strategy of waiting for 2/3rds stake at a lockout 
threshold does not consider stakes that are slashed at the current block height.

#### Gossip, Data plane, Rent

Slashed stakes are effectively zero for the purposes of stake-weighted gossip, turbine, and rent distribution.

## Replicator regime
### Byzantine validators
### Byzantine replicators

