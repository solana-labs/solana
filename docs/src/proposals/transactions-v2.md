# Transactions v2 - Compressed account inputs

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
references through on-chain account indexes.

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

Index accounts must be rent-exempt and may not be deleted. Stored addresses
should be append only so that once an address is stored in an index account, it
may not be removed later. Index accounts must be pre-allocated since Solana does
not support reallocation for accounts yet.

Since transactions use a u16 offset to look up addresses, index accounts can
store up to 2^16 addresses each. Anyone may create an index account of any size
as long as its big enough to store the necessary metadata. In addition to
stored addresses, index accounts must also track the latest count of stored
addresses and an authority which must be a present signer for all index
additions.

Index additions require one slot to activate and so the index data should track
how many additions are still pending activation in on-chain data.

```rust
struct IndexMeta {
  // authority must sign for each addition
  authority: Pubkey,
  // incremented on each addition
  len: u16,
  // always set to the current slot
  last_update_slot: Slot,
  // incremented if current slot is equal to `last_update_slot`
  last_update_slot_additions: u16,
}
```

#### Cost

Since index accounts require caching and special handling in the runtime, they should incur
higher costs for storage. Cost structure design will be added later.

#### Program controlled indexes

If the authority of an index account is controlled by a program, more
sophisticated indexes could be built with governance features or price curves
for new index addresses.

### Versioned Transactions

In order to allow accounts to be referenced more succinctly, the structure of
serialized transactions must be modified. The new transaction format should not
affect transaction processing in the Solana VM beyond the increased capacity for
accounts and instruction invocations. Invoked programs will be unaware of which
transaction format was used.

The new transaction format must be distinguished from the current transaction
format.  Current transactions can fit at most 19 signatures (64-bytes each) but
the message header encodes `num_required_signatures` as a `u8`. Since the upper
bit of the `u8` will never be set for a valid transaction, we can enable it to
denote whether a transaction should be decoded with the versioned format or not.

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

#### Cost changes

Accessing an index account in a transaction should incur an extra cost due to
the extra work validators need to do to load and cache index accounts.

#### Metadata changes

Each account accessed via an index should be stored in the transaction metadata
for quick reference. This will avoid the need for clients to make multiple RPC
round trips to fetch all accounts referenced in a page-indexed transaction. It
will also make it easier to use the ledger tool to analyze account access
patterns.

#### RPC changes

Fetched transaction responses will likely require a new version field to
indicate to clients which transaction structure to use for deserialization.
Clients using pre-existing RPC methods will receive error responses when
attempting to fetch a versioned transaction which will indicate that they
must upgrade.

The RPC API should also support an option for returning fully decompressed
transactions to abstract away the indexing details from downstream clients.

### Limitations

- Max of 256 accounts may be specified in a transaction because u8 is used by compiled
instructions to index into transaction message account keys.
Indexes can hold up to 2^16 keys. Smaller indexes is ok. Each index is then u16
- Transaction signers may not be specified using an on-chain account index, the
full address of each signer must be serialized in the transaction. This ensures
that the performance of transaction signature checks is not affected.
- Hardware wallets will probably not be able to display details about accounts
referenced with an index due to inability to verify on-chain data.
- Only single level indexes can be used. Recursive indexes will not be supported.

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

Additionally, a malicious actor could try to fork the chain immediately after a
new index account is added to a block. If successful, they could add a different
unexpected index account in the fork. In order to deter this attack, clients
should wait for indexes to be finalized before using them in a transaction.
Clients may also append integrity check instructions to the transaction which
verify that the correct accounts are used.

### Denial of service

Index accounts will be read very frequently and will therefore be a more high
profile target for denial of service attacks through write locks similar to
sysvar accounts.

Since stored accounts inside index accounts are immutable, reads and writes
to index accounts could be parallelized as long as all referenced addresses
are for indexes less than the current number of addresses stored.

### Duplicate accounts

If the same account is referenced in a transaction by address as well as through
an index, the transaction should be rejected to avoid conflicts when determining
if the account is a signer or writeable.

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
its message hash will need to be added to the status cache when executed.

3) Epoch account indexes

Similarly to leader schedule calculation, validators could create a global index
of the most accessed accounts in the previous epoch and make that index
available to transactions in the following epoch.

This approach has a downside of only updating the index at epoch boundaries
which means there would be a few day delay before popular new accounts could be
referenced. It also needs to be consistently generated by all validators by
using some criteria like adding accounts in order by access count.

4) Address lists

Extend the transaction structure to support addresses that, when loaded, expand
to a list of addresses. After expansion, all account inputs are concatenated to
form a single list of account keys which can be indexed into by instructions.
Address lists would likely need to be immutable to prevent attacks. They would
also need to be limited in length to limit resource consumption.

This proposal can be thought of a special case of the proposed index account
approach. Since the full account list would be expanded, there's no need to add
additional offsets that use up the limited space in a serialized
transaction. However, the expected size of an address list may need to be
encoded into the transaction to aid the sanitization of account indexes.
Additionally, special attention must be given to watch out for accounts that
exist in multiple account lists.
