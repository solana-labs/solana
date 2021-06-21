# Transactions v2 - Address maps

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

Introduce a new on-chain program which stores account address maps and add a new
transaction format which supports concise account references through the
on-chain address maps.

### Address Map Program

Here we describe a program-based solution to the problem, whereby a protocol
developer or end-user can create collections of related addresses on-chain for
concise use in a transaction's account inputs. This approach is similar to page
tables used in operating systems to succinctly map virtual addresses to physical
memory.

After addresses are stored on-chain in an address map account, they may be
succinctly referenced in a transaction using a 1-byte u8 index rather than a
full 32-byte address. This will require a new transaction format to make use of
these succinct references as well as runtime handling for looking up and loading
accounts from the on-chain mappings.

#### State

Address map accounts must be rent-exempt but may be closed with a one epoch
deactivation period. Address maps must be activated before use.

Since transactions use a u8 offset to look up mapped addresses, accounts can
store up to 2^8 addresses each. Anyone may create an address map account of any
size as long as its big enough to store the necessary metadata. In addition to
stored addresses, address map accounts must also track the latest count of
stored addresses and an authority which must be a present signer for all
appended map entries.

Map additions require one slot to activate so each map should track how many
addresses are still pending activation in their on-chain state:

```rust
struct AddressMap {
  // authority must sign for each addition and to close the map account
  authority: Pubkey,
  // record a deactivation epoch to help validators know when to remove
  // the map from their caches.
  deactivation_epoch: Epoch,
  // entries may not be modified once activated
  activated: bool,
  // list of entries, max capacity of u8::MAX
  entries: Vec<Pubkey>,
}
```

#### Cleanup

Once an address map gets stale and is no longer used, it can be reclaimed by the
authority withdrawing lamports but the remaining balance must be greater than
two epochs of rent. This ensures that it takes at least one full epoch to
deactivate a map.

Maps may not be recreated because each new map must be created at a derived
address using a monotonically increasing counter as a derivation seed.

#### Cost

Since address map accounts require caching and special handling in the runtime,
they should incur higher costs for storage. Cost structure design will be added
later.

### Versioned Transactions

In order to allow accounts to be referenced more succinctly, the structure of
serialized transactions must be modified. The new transaction format should not
affect transaction processing in the Solana VM beyond the increased capacity for
accounts and program invocations. Invoked programs will be unaware of which
transaction format was used.

The new transaction format must be distinguished from the current transaction
format. Current transactions can fit at most 19 signatures (64-bytes each) but
the message header encodes `num_required_signatures` as a `u8`. Since the upper
bit of the `u8` will never be set for a valid transaction, we can enable it to
denote whether a transaction should be decoded with the versioned format or not.

#### New Transaction Format

```rust
#[derive(Serialize, Deserialize)]
pub struct Transaction {
    #[serde(with = "short_vec")]
    pub signatures: Vec<Signature>,
    /// The message to sign.
    pub message: Message,
}

// Uses custom serialization. If the first bit is set, a versioned message is
// encoded starting from the next byte. If the first bit is not set, all bytes
// are used to encode the original unversioned `Message` format.
pub enum Message {
  Unversioned(UnversionedMessage),
  Versioned(VersionedMessage),
}

// use bincode varint encoding to use u8 instead of u32 for enum tags
#[derive(Serialize, Deserialize)]
pub enum VersionedMessage {
  Current(Box<MessageV2>)
}

#[derive(Serialize, Deserialize)]
pub struct MessageV2 {
  // unchanged
  pub header: MessageHeader,

  // unchanged
  #[serde(with = "short_vec")]
  pub account_keys: Vec<Pubkey>,

  /// The last `address_maps.len()` number of readonly unsigned account_keys
  /// should be loaded as address maps
  #[serde(with = "short_vec")]
  pub address_maps: Vec<AddressMap>,

  // unchanged
  pub recent_blockhash: Hash,

  // unchanged. Account indices are still `u8` encoded so the max number of accounts
  // in account_keys + address_maps is limited to 256.
  #[serde(with = "short_vec")]
  pub instructions: Vec<CompiledInstruction>,
}

#[derive(Serialize, Deserialize)]
pub struct AddressMap {
  /// The last num_readonly_entries of entries are read-only
  pub num_readonly_entries: u8,

  /// List of map entries to load
  #[serde(with = "short_vec")]
  pub entries: Vec<u8>,
}
```

#### Size changes

- 1 byte for `prefix` field
- 1 byte for version enum discriminant
- 1 byte for `address_maps` length
- Each map requires 2 bytes for `entries` length and `num_readonly`
- Each map entry is 1 byte (u8)

#### Cost changes

Using an address map in a transaction should incur an extra cost due to
the extra work validators need to do to load and cache them.

#### Metadata changes

Each account accessed via an address map should be stored in the transaction
metadata for quick reference. This will avoid the need for clients to make
multiple RPC round trips to fetch all accounts referenced in a v2 transaction.
It will also make it easier to use the ledger tool to analyze account access
patterns.

#### RPC changes

Fetched transaction responses will likely require a new version field to
indicate to clients which transaction structure to use for deserialization.
Clients using pre-existing RPC methods will receive error responses when
attempting to fetch a versioned transaction which will indicate that they
must upgrade.

The RPC API should also support an option for returning fully expanded
transactions to abstract away the address map details from downstream clients.

### Limitations

- Max of 256 accounts may be specified in a transaction because u8 is used by compiled
instructions to index into transaction message account keys.
- Address maps can hold up to 256 addresses because references to map entries
are encoded as `u8` in transactions.
- Transaction signers may not be referenced with an address map, the full
address of each signer must be serialized in the transaction. This ensures that
the performance of transaction signature checks is not affected.
- Hardware wallets will probably not be able to display details about accounts
referenced through address maps due to inability to verify on-chain data.
- Only single level address maps can be used. Recursive maps will not be supported.

## Security Concerns

### Resource consumption

Enabling more account inputs in a transaction allows for more program
invocations, write-locks, and data reads / writes. Before address maps are
enabled, transaction-wide compute limits and increased costs for write locks and
data reads are required.

### Front running

If the addresses listed within an address map account are modifiable, front
running attacks could modify which mapped accounts are resolved for a later
transaction. For this reason, we propose that any stored address is immutable
and that address map accounts themselves may not be recreated.

Additionally, a malicious actor could try to fork the chain immediately after a
new address map account is added to a block. If successful, they could add a
different unexpected map entry in the fork. In order to deter this attack,
clients should wait for address maps to be finalized before using them in a
transaction.  Clients may also append integrity check instructions to the
transaction which verify that the correct accounts are used.

### Denial of service

Address map accounts will be read very frequently and will therefore be a
more high profile target for denial of service attacks through write locks
similar to sysvar accounts.

For this reason, special handling should be given to address map lookups.
Address maps lookups should not be affected by account read/write locks.

### Duplicate accounts

Transactions may not load an account more than once whether directly through
`account_keys` or indirectly through `address_maps`.

## Other Proposals

1) Account prefixes

Needing to pre-register accounts in an on-chain address map is cumbersome
because it adds an extra step for transaction processing. Instead, Solana
transactions could use variable length address prefixes to specify accounts.
These prefix shortcuts can save on data usage without needing to setup on-chain
state.

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
additional offsets that use up the limited space in a serialized transaction.
However, the expected size of an address list may need to be encoded into the
transaction to aid the sanitization of account indexes.  We would also need to
encode how many addresses in the list should be loaded as readonly vs
read-write. Lastly, special attention must be given to watch out for addresses
that exist in multiple account lists.
