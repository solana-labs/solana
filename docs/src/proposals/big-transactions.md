# Big Transactions

## Problem

Messages transmitted to Solana validators must not exceed the IPv6 MTU size to
ensure fast and reliable network transmission of cluster info over UDP.
Solana's networking stack uses a conservative MTU size of 1280 bytes which,
after accounting for headers, leaves 1232 bytes for packet data like serialized
transactions.

Developers building applications on Solana must design their on-chain program
interfaces within the above transaction size limit constraint. One common
work-around is to store state temporarily on-chain and consume that state in
later transactions. This is the approach used by the BPF loader program for
deploying Solana programs.

However, this workaround doesn't work well when developers compose many on-chain
programs in a single atomic transaction. With more composition comes more
account inputs, each of which takes up 32 bytes. There is currently no available
workaround for increasing the number of accounts used in a single transaction
since each transaction must list all accounts that it needs to properly lock
accounts for parallel execution. Therefore the current cap is about 35 accounts
after accounting for signatures and other transaction metadata.

## Proposed Solution

Introduce a new on-chain account indexing program which stores account address
mappings and add a new transaction format which supports concise account
references through the new on-chain account indexes.

### Account Indexing Program

Here we describe a contract-based solution to the problem, whereby a protocol
developer or end-user can create collections of related accounts on-chain for
concise use in a transaction's account inputs. This approach is similar to page
tables used in operating systems to succinctly map virtual addresses to physical
memory.

After addresses are stored on-chain in an index account, they may be succinctly
referenced from a transaction using an index rather than a full 32 byte address.
This will require a new transaction format to make use of these succinct indexes
as well as runtime handling for looking up and loading accounts from the
on-chain indexes.

#### State

Once created, index accounts may not be deleted. Stored addresses should be
append only so that once an address is stored in an index account, it may not be
removed later.

Since transactions use a u16 offset to look up addresses, index accounts can
store up to 2^16 addresses each. Anyone may create an index account of any size
as long as its big enough to store the necessary metadata. In addition to
stored addresses, index accounts must also track the latest count of stored
addresses and an authority which must be a present signer for all index
modifications.

#### Program controlled indexes

If the authority of an index account is controlled by a program, more
sophisticated indexes could be built with governance features or price curves
for new index addresses.

### Versioned Transactions

In order to allow accounts to be referenced more succinctly, the structure of
serialized transactions must be modified. This means that the new transaction
format must be distinguished from the current transaction format.

Current transactions can fit at most 19 signatures (64-bytes each) but the
message format encodes the number of required signers as a `u8`.  Since the
upper bit of the `u8` will never be set for a valid transaction, we can enable
it to denote whether a transaction should be decoded with the versioned format
or not.

#### New Transaction Format

```rust
pub struct VersionedMessage {
    /// Version of encoded message.
    /// The max encoded version is 2^7 - 1 due to the ignored upper disambiguation bit
    pub version: u8,
    pub header: MessageHeader,
    /// Number of read-only account inputs specified thru indexes 
    pub num_readonly_indexed_accounts: u8,
    #[serde(with = "short_vec")]
    pub account_keys: Vec<Pubkey>,
    /// All the account indexes used by this transaction
    #[serde(with = "short_vec")]
    pub account_indexes: Vec<AccountIndex>,
    pub recent_blockhash: Hash,
    /// Compiled instructions stay the same, account indexes continue to be stored
    /// as a u8 which means the max number of account_indexes + account_keys is 256.
    #[serde(with = "short_vec")]
    pub instructions: Vec<CompiledInstruction>,
}

pub struct AccountIndex {
    pub account_key_offset: u8,
    // 1-3 bytes used to lookup address in index account
    pub index_account_offset: CompactU16,
}
```

#### Size changes

- Extra byte for version field
- Extra byte for number of total account index inputs
- Extra byte for number of readonly account index inputs
- Most indexes will be compact and use 2 bytes + index address
- Cost of each additional index account is ~2 bytes

### Limitations

- Max of 256 accounts may be specified in a transaction because u8 is used by compiled
instructions to index into transaction message account keys.
Indexes can hold up to 2^16 keys. Smaller indexes is ok. Each index is then u16
- Transaction signers may not be specified using an on-chain account index, the
full address of each signer must be serialized in the transaction. This ensures
that the performance of transaction signature checks is not affected.

## Security Concerns

### Resource consumption

Enabling more account inputs in a transaction allows for more program
invocations, write-locks, and data reads / writes. Before indexes are live, we
need transaction-wide compute limits and increased costs for write locks and
data reads.

### Front running

If the addresses listed within an index account are modifiable, front running
attacks could modify which index accounts are accessed from a later transaction.
For this reason, we propose that any stored address is immutable and that index
accounts themselves may not be removed.

### Denial of service

Index accounts will be read very frequently and will therefore be a more high
profile target for denial of service attacks through write locks similar to
sysvar accounts.

Since stored accounts inside index accounts are immutable, reads and writes
to index accounts could be parallelized as long as all referenced addresses
are for indexes less than the current number of addresses stored.

## Other Proposals

1) Account prefixes

Needing to pre-register accounts in an on-chain index is cumbersome because it
adds an extra step for transaction processing. Instead, Solana transactions
could use variable length address prefixes to specify accounts. These prefix
shortcuts can save on data usage without needing to setup on-chain state.

However, this model requires nodes to keep a mapping of prefixes to active account
addresses. Attackers can create accounts with the same prefix as a popular account
to disrupt transactions.

2) Transaction builder program

Solana can provide a new on-chain program which allows "Big" transactions to be
constructed on-chain by normal transactions. Once the transaction is
constructed, a final "Execute" transaction can trigger a node to process the big
transaction as a normal transaction without needing to fit it into an MTU sized
packet.

The UX of this approach is tricky. A user could in theory sign a big transaction
but it wouldn't be great if they had to use their wallet to sign multiple
transactions to build that transaction that they already signed and approved. This
could be a use-case for transaction relay services, though. A user could pay a
relayer to construct the large pre-signed transaction on-chain for them.

In order to prevent the large transaction from being reconstructed and replayed,
a nonce counter system would be necessary as well.

3) Epoch account indexes

Similarly to leader schedule calculation, validators could create a global index
of the most accessed accounts in the previous epoch and make that index
available to transactions in the following epoch.

This approach has a downside of only updating the index at epoch boundaries
which means there would be a few day delay before popular new accounts could be
referenced.
