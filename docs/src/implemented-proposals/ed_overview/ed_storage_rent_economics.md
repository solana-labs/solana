## Storage Rent Economics

Each transaction that is submitted to the Solana ledger imposes costs. Transaction fees paid by the submitter, and collected by a validator, in theory, account for the acute, transactional, costs of validating and adding that data to the ledger. At the same time, our compensation design for archivers (see [Replication-client Economics](ed_replication_client_economics.md)), in theory, accounts for the long term storage of the historical ledger. Unaccounted in this process is the mid-term storage of active ledger state, necessarily maintained by the rotating validator set. This type of storage imposes costs not only to validators but also to the broader network as active state grows so does data transmission and validation overhead. To account for these costs, we describe here our preliminary design and implementation of storage rent.

Storage rent can be paid via one of two methods:

Method 1: Set it and forget it

With this approach, accounts with two-years worth of rent deposits secured are exempt from network rent charges. By maintaining this minimum-balance, the broader network benefits from reduced liquidity and the account holder can trust that their `Account::data` will be retained for continual access/usage.

Method 2: Pay per byte

If an account has less than two-years worth of deposited rent the network charges rent on a per-epoch basis, in credit for the next epoch (but in arrears when necessary). This rent is deducted at a rate specified in genesis, in lamports per kilobyte-year.

For information on the technical implementation details of this design, see the [Rent](rent.md) section. 



