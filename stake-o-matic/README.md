## Effortlessly Manage Cluster Stakes
The testnet and mainnet-beta clusters currently have a large population of
validators that need to be staked by a central authority.

## Staking Criteria
1. All non-delinquent validators receive 5,000 SOL stake
1. Additionally, non-deliquent validators that have produced a block in 75% of
   their slots in the previous epoch receive bonus stake of 50,000 SOL

A validator that is delinquent for more than 24 hours will have all stake
removed.  However stake-o-matic has no memory, so if the same validator resolves
their delinquency then they will be re-staked again

## Validator Whitelist
To be eligible for staking, a validator's identity pubkey must be added to a
YAML whitelist file.

## Stake Account Management
Stake-o-matic will split the individual validator stake accounts from a master
stake account, and must be given the authorized staker keypair for the master
stake account.
