---
Tiered-storage design and cold-storage format for Accounts DB
---

## Problem
As the number of accounts grows, we need a more storage-efficient solution
for our accounts DB to support billions of accounts while keeping it
performant.

## Workload
The accounts DB serves both reads and writes.  It is a read-write-balanced to
write-heavy workload.  Writes to the accounts DB constantly happen as a result
of committing transactions/blocks, and the accounts DB serves the following
read workload:

* Point-lookups from transactions: transactions also read relevant account
data / meta.  Lookups are in general random, with some accounts that are
accessed more frequently than the others in a short period of time.

* Scans for recent slots for hash calculation: periodic hash calculation scans
all accounts from the recent slots.  Currently, hash calculation happens for
every 100 blocks, and we plan to reduce the frequency to once per 25k blocks
in near future with incremental accounts hashes and incremental snapshots.

* Not-found queries for account creation: each account creation checks
whether the given address already exists in the accounts DB.  This requires
an efficient way to support "not-found" queries.

* Hot accounts: we observed that some accounts are updated more frequently than
others, and each update to one account's data usually only changes a small
portion of the data instead of the entire data.

## Goals
The new design should be able to store billions of accounts while serving their
transactions performantly.  Specifically, it should be performant, efficient
in storage, size, modularizable, and extensible.

* Performant: able to efficiently serve transactions.

* Efficient in storage size: able to hold billions of accounts on affordable
validators.

* Modularizable: parts of the design can be updated/changed without
breaking other parts of the design.

* Extensible: easily extensible to handle possible changes in the future
without major compatibility issues.

## Solution Overview
To satisfy the contradicting goals, the proposed solution uses a tiered-
structure, where accounts are stored differently based on their update
frequency (or temperature).  An account is hot if it is recently accessed
frequently.  Similarly, an account is cold if it has not been accessed
for a long period of time.

- Hot storage: the storage for the most frequently accessed accounts that is
fast for in-memory access.

- Warm storage: storage for the most recent slots.  Files that contain frequently
accessed accounts may be promoted to hot storage.

- Cold storage: storage format for older slots that is optimized for smaller
storage size.

This tiered-storage design provides us the flexibility to optimize hot and cold
accounts differently to reach the performance and storage size goal.

As the existing mmaped AppendVec file format is serving as a hot / warm storage
currently, the rest of this proposal will focus on the cold-storage file format.

### Cold Storage File Format
Similar to the existing AppendVec format, each cold storage file stores data
for one or more slots, where the slot information is maintained in the accounts-
index.

Here's the proposed cold storage file layout:

    +------------------------------------------------------------------------------+
    | Account Data Blocks      | Account data in compressed blocks.                |
    | (compressed)             | Optional fields such as rent_epoch are also       |
    |                          | stored here.                                      |
    +------------------------------------------------------------------------------+
    | Account Metas Block      | Fixed-size entries for storing accounts metadata. |
    | (can be compressed)      | Metadata also includes information about how to   |
    |                          | access its owner and account data.                |
    +------------------------------------------------------------------------------+
    | Owners Block             | 32 byte (Pubkey) per entry                        |
    | (uncompressed)           | Each account meta entry has an index pointing     |
    |                          | to its owner entry.                               |
    +------------------------------------------------------------------------------+
    | Account Index Block      | Index for accessing accounts meta block           |
    | (uncompressed)           | Footer has an enum entry describing the format    |
    |                          | of this block.                                    |
    +------------------------------------------------------------------------------+
    | Footer                   | Stores information about how to read the file.    |
    | (uncompressed)           | (e.g. the offset and format of each block.)       |
    +------------------------------------------------------------------------------+

As a cold storage format, the primary goal is to make it storage size efficient
while making it cache/index friendly.  The proposed format stores account data
in a more column-oriented way so that we can optimally store different types
of data based on their characteristics.  For instance, account data and account
meta-data are more compressible than addresses.  Account addresses can also be
used as index to access account data/metadata.

The blocks are stored in reverse order as we want to make the file writer
single-pass.  The single-pass writer design also allows us to construct the
account index block optimally.  Once a file is written, it becomes immutable.

In the rest of the sections, we will introduce the format in reverse order, as the
reader of this file will also read from the bottom of the file, which is the
Footer.

#### Footer
The footer block includes information about how to read the file.

The file reader begins by reading the last 88 bytes of the file, which includes
a magic number (8 bytes), the min and max account addresses (32 bytes each),
the format version of the file, and the size of the footer.

The magic number indicates whether the file is a complete accounts data storage
file without truncation.  The min and max account addresses allow the reader
to quickly know whether the account of interest falls-in the account range of
this file.  The format version allows the reader to correctly parse the file
using the right verion.  The footer size tells the reader how to read the
remaining bytes of the footer.

The remaining part of the footer describes the structure of the file,
including the offsets of each block, the size and number of entries
in each block.

    +----------------------------------------------------------------------------------+
    | Footer                                                                           |
    +----------------------------------------------------------------------------------+
    | data_block_compression (8 bytes)   | The compression algorithm for data blocks.  |
    | data_block_size (8 bytes)          | The size of the account data block.         |
    |                                    | (Defult: 4k, uncompressed)                  |
    |                                    |                                             |
    | account_metas_offset (8 bytes)     | The offset of the account metas block.      |
    | account_meta_count (8 bytes)       | The number of account meta entries.         |
    | account_meta_entry_size (8 bytes)  | The size of each account meta entry.        |
    | account_metas_compression (8 bytes)| The compression algorithm for account metas.|
    |                                    |                                             |
    | owners_offset (8 bytes)            | The offset of the owners block.             |
    | owner_count (8 bytes)              | The number of unique owners in this file.   |
    | owner_entry_size (8 bytes)         | The size of each owner entry in bytes.      |
    |                                    |                                             |
    | account_index_offset (8 bytes)     | The offset of the account address block.    |
    | account_index_format (8 bytes)     | Describe the format of the account index    |
    |                                    | block.                                      |
    |                                    |                                             |
    | file_hash (32 bytes)               | The accounts hash of the entire file.       |
    |                                    |                                             |
    | optional_field_flags_size (8 bytes)| The size of the optional_field_flags of     |
    |                                    | each account.                               |
    | optional_field_version    (8 bytes)| The version of the account optional fields. |
    +------------------------------------+---------------------------------------------+
    | footer_size (8 bytes)              | The size of the footer block.               |
    | format_version (8 bytes)           | The format version of this file.            |
    | min_account_address (32 bytes)     | The minimum account address in this file.   |
    | max_account_address (32 bytes)     | The maximum account address in this file.   |
    | magic_number (8 bytes)             | A predefined const magic number to indicate |
    |                                    | the file type and make sure the file itself |
    |                                    | is completed without truncation.            |
    +------------------------------------+---------------------------------------------+

#### Account Index Block
This block includes information about how to read an account given its address.
Its format information is described in the `account_index_format` in the Footer.

In the basic format, the account index block is sorted by the account's address
to allow binary search.  It has two sections: `account_addrsses` and `meta_indices`.
`account_addresses` section stores every pubkeys in sorted order.  Each account
address entry has a correspinding entry in the `meta_indices` section.

    +------------------------------------+---------------------------------------------+
    | Account Index Block (Binary Search Format) -- 40 bytes per account               |
    +------------------------------------+---------------------------------------------+
    | addresses (32-byte each)           | Each entry is 32-byte (Pubkey).             |
    |                                    | Entries are sorted in alphabetical order.   |
    +------------------------------------+---------------------------------------------+
    | meta_indices (4-byte each)         | Indices to access the account meta.         |
    |                                    | The Nth entry in addresses corresponds      |
    |                                    | Nth entry in meta_offsets.                  |
    +------------------------------------+---------------------------------------------+

#### Owners Block
Because multiple accounts might share the same owner, storing unique owners in
one block and accessing them using local indices would save some disk space.

    +----------------------------------------------------------------------------------+
    | Owners Block -- 32 bytes per unique owner                                        |
    +------------------------------------+---------------------------------------------+
    | owner_addresses                    | 32-byte (Pubkey) each.                      |
    |                                    | Each account-meta entry has a local index   |
    |                                    | pointing to its owner's address.            |
    +------------------------------------+---------------------------------------------+

#### Account Metas Block
Account Metas block contains one entry for each account in this file.
Each account-meta is a fixed-size entry that includes its account's metadata and
how to access its account data, owner, and optional fields.

The size of each account meta entry is described in the `account_meta_entry_size`
field in the footer.  The `format_version` field in the footer also provides
information about how to correctly read each account-meta entry.

    +-----------------------------------------------------------------------------------+
    | Account Meta Entry -- 26 bytes per account                                        |
    +-----------------------------------------------------------------------------------+
    | lamport (8 bytes)              | The lamport balance of this account.             |
    |                                |                                                  |
    | data_block_offset (8 bytes)    | The offset to the account's data block.          |
    | intra_block_offset (2 bytes)   | The inner-block offset to the accounts' data     |
    |                                | after decompressing its data block.              |
    | uncompressed_data_len (2 bytes)| The length of the uncompressed_data.             |
    |                                | If this value is u16::MAX, then the size is the  |
    |                                | entire data block minus optional fields.         |
    |                                |                                                  |
    | owner_index (4 bytes)          | The index of the account's owner address in the  |
    |                                | owners block.                                    |
    | flags (4 bytes)                | Flags include executable and whether this account|
    |                                | has a particular optional field.                 ||
    +-----------------------------------------------------------------------------------+

#### Account Data Blocks
Each account data block is a compressed block that contains account data and
optional fields for one or more accounts.

    +-----------------------------------------------------------------------------------+
    | Account Data Blocks                                                               |
    +-----------------------------------------------------------------------------------+
    | ...                                                                               |
    +---------------------------+-------------------------------------------------------+
    | (Nth length -- 8 bytes)   | The compressed length of data block N.                |
    |                           | Note this field can also be derived from its account  |
    |                           | meta entry by find the first account meta entry which |
    |                           | data_block_offset is greater than this account's      |
    |                           | data_block_offset.  But including this field will     |
    |                           | simplify some code logic.                             |
    | Nth compressed data block |                                                       |
    +---------------------------+-------------------------------------------------------+
    | ...                                                                               |
    +-----------------------------------------------------------------------------------+


    +-----------------------------------------------------------------------------------+
    | Compressed Data Block (After de-compression)                                      |
    +-----------------------------------------------------------------------------------+
    | ...                                                                               |
    +--------------------------------+--------------------------------------------------+
    | Nth data                       | The data for the Nth account in this block.      |
    |                                | Its length is described in "data_length" field.  |
    |                                |                                                  |
    | Nth optional fields            | See the optional field section below.            |
    +-----------------------------------------------------------------------------------+
    | ...                                                                               |
    +-----------------------------------------------------------------------------------+

    +-----------------------------------------------------------------------------------+
    | Optional Fields                                                                   |
    +-----------------------+-----------------------------------------------------------+
    | field_value_1         |                                                           |
    | ...                   | If the associated bit in account meta's "flags" is on,    |
    | field_value_n         | then the exact number of bytes is reserved for this field.|
    | ...                   | Otherwise, 0 bytes is used.                               |
    |                       |                                                           |
    +-----------------------------------------------------------------------------------+

## Conclusions and Related Projects
This document introduces the idea of the tiered-storage design for accounts DB.
It stores accounts differently based on their recent update frequency.  For
frequently accessed accounts, it stores them in a more in-memory friendly
format that focuses on runtime performance while less frequently accessed
accounts are persisted in a more storage-size-efficient format.

The document also includes a basic cold storage file format that stores
compressible (accounts data and metadata) and incompressible data (
accounts and owners' addresses) into different blocks.  This modularizable
design also makes it easier to extend or optimize the format of a specific
block.

This proposal opens up several opportunities and challenges.  Potential
projects related to this proposal include:

* A more efficient way to describe hot storage.

* Better in-memory representation of account metadata.

* A better way to make account data accessible to program runtime to avoid
  memory copy.

* More efficient ways to store account indices for cold storage for faster
  not-found look-ups.

* Optimal way to compact/shrink cold storage files to periodically dedupe
  entries that are associated with the same account and only keep the latest
  version.

* Better ways to promote/demote a file/account to hotter/colder storage.

* A snapshot design that is tiered-storage-awared.  For instance, consider
snapshot layers that allow a validator to keep as much cold data as possible.

* Higher-level index or read cache for cold storage.
