# Develop Programs

## Account Properties

#### Balance

All active accounts must have a non-zero balance of lamports, the smallest division of a SOL. Accounts with a balance of **0 lamports** are considered to be inactive and will be deleted. Inactive accounts cannot be loaded by an instruction or accessed in an RPC API call. However, accounts can be initialized with an empty balance inside the scope of a transaction and used in a subsequent instruction but they will not be saved if they have a zero balance after the transaction is finished processing.

#### Ownership

All accounts specify an owner account which has privileged access to the account. Only accounts owned by the system program account may be assigned to a new owner.

* For _non-executable_ accounts, this declares which **program** is allowed to modify its data and deduct from its balance.
* For _executable_ \(program\) accounts, this declares which **loader program** is allowed to execute it and deduct from its balance.

Note that all active accounts can be read and have their balance credited by _any_ program.

#### Data

Accounts can store state in a byte array data field. The size of an account's data is specified at account creation. Data size is specified in number of bytes and can be as low as **0 bytes** and high as **10MB**. Currently, only unallocated \(zero byte\) accounts can be re-allocated after creation \(accounts cannot be dynamically resized\). Newly allocated data can always be assumed to be fully zeroed out.

#### Executable

Accounts can be set as executable to be used as programs but this action is irreversible. Executable accounts are called "programs" when used to process instructions and are called "loaders" when used to execute programs. All executable accounts can process instructions and optionally run sub-programs. One such case is the BPF Loader program which is used to create BPF programs on-chain \(the most common type of program on Solana\).

#### Rent

All non-system, non-executable accounts are subject to rent fees that are calculated as a function of account data size and time measured in epochs. Accounts can be made rent-exempt if their balance reaches the rent-exempt threshold \(~2 years of rent fees\). Created accounts that do not meet the rent-exempt threshold, will immediately be charged rent for the current epoch. Rent will be charged on an epoch-to-epoch basis but fees are only collected when the account is used in an instruction. Rent fees are thus calculated by tracking the last epoch an account paid rent. The fees can vary but are can be deterministically computed for a given transaction nonce or blockhash. More details can be found in the rent proposal.
