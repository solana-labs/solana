# Versioned Transactions - v0: Address Lookup Tables

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

1. Introduce a new program which manages on-chain address lookup tables
2. Add a new transaction format which can make use of on-chain
   address lookup tables to efficiently load more accounts in a single transaction.

### Address Lookup Table Program

Here we describe a program-based solution to the problem, whereby a protocol
developer or end-user can create collections of related addresses on-chain for
concise use in a transaction's account inputs.

After addresses are stored on-chain in an address lookup table account, they may be
succinctly referenced in a transaction using a 1-byte u8 index rather than a
full 32-byte address. This will require a new transaction format to make use of
these succinct references as well as runtime handling for looking up and loading
addresses from the on-chain lookup tables.

#### State

Address lookup tables must be rent-exempt when initialized and after
each time new addresses are appended. Lookup tables can either be extended
from an on-chain buffered list of addresses or directly by appending
addresses through instruction data. Newly appended addresses require
one slot to warmup before being available to transactions for lookups.

Since transactions use a `u8` index to look up addresses, address tables can
store up to 256 addresses each. In addition to stored addresses, address table
accounts also tracks various metadata explained below.

```rust
/// The maximum number of addresses that a lookup table can hold
pub const LOOKUP_TABLE_MAX_ADDRESSES: usize = 256;

/// The serialized size of lookup table metadata
pub const LOOKUP_TABLE_META_SIZE: usize = 56;

pub struct LookupTableMeta {
    /// Lookup tables cannot be closed until the deactivation slot is
    /// no longer "recent" (not accessible in the `SlotHashes` sysvar).
    pub deactivation_slot: Slot,
    /// The slot that the table was last extended. Address tables may
    /// only be used to lookup addresses that were extended before
    /// the current bank's slot.
    pub last_extended_slot: Slot,
    /// The start index where the table was last extended from during
    /// the `last_extended_slot`.
    pub last_extended_slot_start_index: u8,
    /// Authority address which must sign for each modification.
    pub authority: Option<Pubkey>,
    // Raw list of addresses follows this serialized structure in
    // the account's data, starting from `LOOKUP_TABLE_META_SIZE`.
}
```

#### Cleanup

Once an address lookup table is no longer needed, it can be deactivated and closed
to have its rent balance reclaimed. Address lookup tables may not be recreated
at the same address because each new lookup table must be initialized at an address
derived from a recent slot.

Address lookup tables can be deactivated at any time but can continue to be used
by transactions until the deactivation slot is no longer present in the slot hashes
sysvar. This cool-down period ensures that in-flight transactions cannot be
censored and that address lookup tables cannot be closed and recreated for the same
slot.

### Versioned Transactions

In order to support address table lookups, the structure of serialized
transactions must be modified. The new transaction format should not
affect transaction processing in the Solana program runtime beyond the
increased capacity for accounts and program invocations. Invoked
programs will be unaware of which transaction format was used.

The new transaction format must be distinguished from the current transaction
format. Current transactions can fit at most 19 signatures (64-bytes each) but
the message header encodes `num_required_signatures` as a `u8`. Since the upper
bit of the `u8` will never be set for a valid transaction, we can enable it to
denote whether a transaction should be decoded with the versioned format or not.

The original transaction format will be referred to as the legacy transaction
version and the first versioned transaction format will start at version 0.

#### New Transaction Format

```rust
#[derive(Serialize, Deserialize)]
pub struct VersionedTransaction {
    /// List of signatures
    #[serde(with = "short_vec")]
    pub signatures: Vec<Signature>,
    /// Message to sign.
    pub message: VersionedMessage,
}

// Uses custom serialization. If the first bit is set, the remaining bits
// in the first byte will encode a version number. If the first bit is not
// set, the first byte will be treated as the first byte of an encoded
// legacy message.
pub enum VersionedMessage {
    Legacy(LegacyMessage),
    V0(v0::Message),
}

// The structure of the new v0 Message
#[derive(Serialize, Deserialize)]
pub struct Message {
  // unchanged
  pub header: MessageHeader,

  // unchanged
  #[serde(with = "short_vec")]
  pub account_keys: Vec<Pubkey>,

  // unchanged
  pub recent_blockhash: Hash,

  // unchanged
  //
  // # Notes
  //
  // Account and program indexes will index into the list of addresses
  // constructed from the concatenation of three key lists:
  //   1) message `account_keys`
  //   2) ordered list of keys loaded from address table `writable_indexes`
  //   3) ordered list of keys loaded from address table `readonly_indexes`
  #[serde(with = "short_vec")]
  pub instructions: Vec<CompiledInstruction>,

  /// List of address table lookups used to load additional accounts
  /// for this transaction.
  #[serde(with = "short_vec")]
  pub address_table_lookups: Vec<MessageAddressTableLookup>,
}

/// Address table lookups describe an on-chain address lookup table to use
/// for loading more readonly and writable accounts in a single tx.
#[derive(Serialize, Deserialize)]
pub struct MessageAddressTableLookup {
  /// Address lookup table account key
  pub account_key: Pubkey,
  /// List of indexes used to load writable account addresses
  #[serde(with = "short_vec")]
  pub writable_indexes: Vec<u8>,
  /// List of indexes used to load readonly account addresses
  #[serde(with = "short_vec")]
  pub readonly_indexes: Vec<u8>,
}
```

#### Size changes

- 1 extra byte for the `version` field
- 1 extra byte for the `address_table_lookups` length
- 34 extra bytes for each address table lookup which includes 32 bytes for the
  address of the table and a byte each for the lengths of the `writable_indexes`
  and `readonly_indexes` fields
- 1 extra byte for each lookup table index

#### Cost

Address lookups require extra computational overhead during transaction
processing, but also reduce network bandwidth due to smaller transactions and
therefore smaller blocks.

#### Metadata changes

Each resolved address from an address lookup table should be stored in
the transaction metadata for quick reference. This will avoid the need for
clients to make multiple RPC round trips to fetch all accounts loaded by a
v0 transaction. It will also make it easier to use the ledger tool to
analyze account access patterns.

#### RPC changes

Fetched transaction responses will likely require a new version field to
indicate to clients which transaction structure to use for deserialization.
Clients using pre-existing RPC methods will receive error responses when
attempting to fetch a versioned transaction which will indicate that they
must upgrade.

The RPC API should also support an option for returning fully expanded
transactions to abstract away the address lookup table details from
downstream clients.

### Limitations

- Max of 256 unique accounts may be loaded by a transaction because `u8`
  is used by compiled instructions to index into transaction message `account_keys`.
- Address lookup tables can hold up to 256 entries because lookup table indexes are also `u8`.
- Transaction signers may not be loaded through an address lookup table, the full
  address of each signer must be serialized in the transaction. This ensures that
  the performance of transaction signature checks is not affected.
- Hardware wallets will not be able to display details about accounts referenced
  through address lookup tables due to inability to verify on-chain data.
- Only single level address lookup tables can be used. Recursive lookups will not be supported.

## Security Concerns

### Lookup table re-initialization

If an address lookup table can be closed and re-initialized with new addresses,
any client which is unaware of the change could inadvertently lookup unexpected
addresses. To avoid this, all address lookup tables must be initialized at an
address derived from a recent slot and they cannot be closed until the slot
used for deactivation is no longer in the slot hashes sysvar.

### Resource consumption

Enabling more account inputs in a transaction allows for more program
invocations, write-locks, and data reads / writes. Before address tables are
enabled, transaction-wide compute limits and increased costs for write locks and
data reads are required.

### Front running

If the addresses listed within an address lookup table are mutable, front
running attacks could modify which addresses are resolved for a later
transaction. For this reason, address lookup tables are append-only and may
only be closed if it's no longer possible to create a new lookup table at the
same derived address.

Additionally, a malicious actor could try to fork the chain immediately after a
new address lookup table account is added to a block. If successful, they could
add a different unexpected table entry in the fork. In order to deter this attack,
clients should wait for address lookup tables to be finalized before using them in a
transaction. Clients may also append integrity check instructions to the
transaction which verify that the correct accounts are looked up.

### Denial of service

Address lookup table accounts may be read very frequently and will therefore
be a more high profile target for denial of service attacks through write locks
similar to sysvar accounts.

For this reason, special handling should be given to address lookup tables.
When an address lookup table is used to lookup addresses for a transaction,
it can be loaded without waiting for a read lock. To avoid race conditions,
only the addresses appended in previous blocks can be used for lookups and
deactivation requires a cool-down period.

### Duplicate accounts

Transactions may not load an account more than once whether directly through
`account_keys` or indirectly through `address_table_lookups`.

## Other Proposals

1. Account prefixes

Needing to pre-register accounts in an on-chain address lookup table is cumbersome
because it adds an extra step for transaction processing. Instead, Solana
transactions could use variable length address prefixes to specify accounts.
These prefix shortcuts can save on data usage without needing to setup on-chain
state.

However, this model requires nodes to keep a mapping of prefixes to active account
addresses. Attackers can create accounts with the same prefix as a popular account
to disrupt transactions.

2. Transaction builder program

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

3. Epoch account indexes

Similarly to leader schedule calculation, validators could create a global index
of the most accessed accounts in the previous epoch and make that index
available to transactions in the following epoch.

This approach has a downside of only updating the index at epoch boundaries
which means there would be a few day delay before popular new accounts could be
referenced. It also needs to be consistently generated by all validators by
using some criteria like adding accounts in order by access count.

4. Address lists

Extend the transaction structure to support addresses that, when loaded, expand
to a list of addresses. After expansion, all account inputs are concatenated to
form a single list of account keys which can be indexed into by instructions.
Address lists would likely need to be immutable to prevent attacks. They would
also need to be limited in length to limit resource consumption.

This proposal can be thought of a special case of the proposed index account
approach. Since the full account list would be expanded, there's no need to add
additional offsets that use up the limited space in a serialized transaction.
However, the expected size of an address list may need to be encoded into the
transaction to aid the sanitization of account indexes. We would also need to
encode how many addresses in the list should be loaded as readonly vs
read-write. Lastly, special attention must be given to watch out for addresses
that exist in multiple account lists.

5. Increase transaction size

Significantly larger serialized transactions have an increased likelihood of being
dropped over the wire but this might not be a big issue since clients can retry
transactions anyways. The only time validators need to send individual transactions
over the network is when a leader forwards unprocessed transactions to the next
leader.
