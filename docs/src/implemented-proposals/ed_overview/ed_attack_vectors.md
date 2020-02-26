# Attack Vectors

**Subject to change.**

## Colluding validation and replication clients

A colluding validation-client, may take the strategy to mark PoReps from non-colluding archiver nodes as invalid as an attempt to maximize the rewards for the colluding archiver nodes. In this case, it isn’t feasible for the offended-against archiver nodes to petition the network for resolution as this would result in a network-wide vote on each offending PoRep and create too much overhead for the network to progress adequately. Also, this mitigation attempt would still be vulnerable to a &gt;= 51% staked colluder.

Alternatively, transaction fees from submitted PoReps are pooled and distributed across validation-clients in proportion to the number of valid PoReps discounted by the number of invalid PoReps as voted by each validator-client. Thus invalid votes are directly dis-incentivized through this reward channel. Invalid votes that are revealed by archiver nodes as fishing PoReps, will not be discounted from the payout PoRep count.

Another collusion attack involves a validator-client who may take the strategy to ignore invalid PoReps from colluding archiver and vote them as valid. In this case, colluding archiver-clients would not have to store the data while still receiving rewards for validated PoReps. Additionally, colluding validator nodes would also receive rewards for validating these PoReps. To mitigate this attack, validators must randomly sample PoReps corresponding to the ledger block they are validating and because of this, there will be multiple validators that will receive the colluding archiver’s invalid submissions. These non-colluding validators will be incentivized to mark these PoReps as invalid as they have no way to determine whether the proposed invalid PoRep is actually a fishing PoRep, for which a confirmation vote would result in the validator’s stake being slashed.

In this case, the proportion of time a colluding pair will be successful has an upper limit determined by the % of stake of the network claimed by the colluding validator. This also sets bounds to the value of such an attack. For example, if a colluding validator controls 10% of the total validator stake, transaction fees will be lost \(likely sent to mining pool\) by the colluding archiver 90% of the time and so the attack vector is only profitable if the per-PoRep reward at least 90% higher than the average PoRep transaction fee. While, probabilistically, some colluding archiver-client PoReps will find their way to colluding validation-clients, the network can also monitor rates of paired \(validator + archiver\) discrepancies in voting patterns and censor identified colluders in these cases.

