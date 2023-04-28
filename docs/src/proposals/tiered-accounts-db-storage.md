---
Tiered Accounts DB Storage
---

## Summary
This proposal presents a hierarchical storage architecture that enables
the accounts database to efficiently manage a large number of accounts.
It stores hot (active) accounts in a format that is optimized for
performance while persisting cold (inactive) accounts in a compressed
format that conserves disk space.

On the storage saving, the proposed file format saves 70 bytes per hot
account and 220 bytes per cold account assuming average account data
size is the same as today's 270 bytes.  With the 20% hot accounts and
and 80% cold accounts setting, the proposed format is able to save
~50% storage size, or ~200GB storage size reduction when storing 1
billion accounts.

## Motivation
The existing accounts db employs AppendVec, a memory-mapped file format
and optimized for efficient runtime performance.  However, as the number of
accounts continues to expand, a storage solution is required to accommodate
billions of accounts while still maintaining performance.

## Workload
The accounts DB serves both reads and writes.  It is a read-write-balanced to
write-heavy workload.  Writes to the accounts DB constantly happen as a result
of committing transactions/blocks, and the accounts DB serves the following
read workload:

* Point-lookups from transactions: transactions also read relevant account
data / meta.  Lookups are in general random, with some accounts that are
accessed more frequently than the others in a short period of time.

* Scans for recent slots for hash calculation: periodic hash calculation scans
all accounts from the recent slots.  There're two types of hash calcaultions:
full accounts hash calculation and incremental accounts hash calculation.
For full accounts hash, it is calculated for every 25k slots by default, while
incremental accounts hash is calculated for every 100 slots by default.

* Not-found queries for account creation: each account creation checks
whether the given address already exists in the accounts DB.  This requires
an efficient way to support "not-found" queries.

* RPC calls that query for a single or multiple accounts.  Some accounts might
appear in RPC calls more often than the others.

* Hot accounts: we observed that some accounts are updated more frequently than
others, and each update to one account's data usually only changes a small
portion of the data instead of the entire data.

## Design Overview
The key idea of the proposed design is to use different storage formats
for hot(active) and cold(inactive) accounts so that we achieve storage
size saving from cold accounts while optimizing runtime performance for
hot accounts.

As accounts which have any update will be in the latest slot, we can simply
treat accounts in those recent slots as hot accounts as they have recent
updates.  Once an account went through the shrink / compaction process,
it will become cold, as it means this account hasn't been updated for a
while.

    Current Approach:

     Ancient AppendVecs             AppendVecs
     (Same format as AppendVec)
     +------+ +------+           +------+  +------+
     |      | |      |   Shrink  |      |  |      |
     | Slot | | Slot | / Compact | Slot |  | Slot |
     |  N   | | N + 1|   ......  | L - 1|  |Latest|
     |      | |      |           |      |  |      |
     +------+ +------+           +------+  +------+


    Proposed Approach

        Cold Storage                 Hot Storage
    (Compressed Format)       (Mmapped/Aligned Format)
     +------+ +------+           +------+  +------+
     |      | |      |   Shrink  |      |  |      |
     | Slot | | Slot | / Compact | Slot |  | Slot |
     |  N   | | N + 1|   .....   | L - 1|  |Latest|
     |      | |      |           |      |  |      |
     +------+ +------+           +------+  +------+

Note that for the cold storage, it does not require files to be
slot-range-partitioned.  It can support pubkey-range-partitioned as well.

## Estimated Storage Saving
For hot accounts, the storage saving comes from the improved on-disk
account meta representation, optional fields design, account owner
deduplication, deprecating unused fields and minimizing paddings.
This saves us at least 68 bytes per account (72 bytes per account meta,
down from 140 bytes), assuming all accounts owners are unique.

For cold accounts, in addition to the ~70 bytes per account storage
savings from the account meta.  All account data is stored in compressed
format.

The size of account data varies a lot.  It ranges from 0 bytes up to 10MB.
If we use today's average account data size 270 bytes and 60% compression
rate based on experiments from today's account data, then each cold account
in the new format will save us 230 bytes in average.

Assuming average account data size 270 bytes, 20% hot accounts, and 80% cold
accounts, then storing 1-billion account entries in the new format will
save us ~200GB (~70GB saving from account meta, and ~128GB saving from compressed
account data).  If we directly compare the size, it is 226GB in the new format
vs 410GB in the current format.


    +------------------+-----------------------------------------------+
    |                  |  Current Format  |  New Format  |  New Format |
    |                  |                  |    (Hot)     |    (Cold)   |
    +------------------+-----------------------------------------------+
    |  Account Meta    |     140 bytes    |    72 bytes  |   76 bytes  |
    +------------------+-----------------------------------------------+
    | Avg Account Data |     270 bytes    |   270 bytes  |  108 bytes  |
    +------------------------------------------------------------------+
    | Avg Account Size |     410 bytes    |   342 bytes  |  184 bytes  |
    +------------------------------------------------------------------+
    | Relative Size    |       100%       |      83%     |      44%    |
    +------------------------------------------------------------------+


### The Detailed Design

The proposed design uses a extensible blocked-based file format that
can be used for both cold and hot storage.  The format includes
five sections: Account Data Blocks, Account Metas Block, Owners Block,
Account Index Block, and Footer.

The Footer includes information about to read the file.  Specifically,
it includes the file format version and the format of each section.
This allows cold and hot storage shares the same file layout but optimize
differently in each section.  For instance, cold storage has its account
data blocks compressed while hot storage stores its account data blocks
in uncompressed and aligned format in order to make it memory-mapped
possible.

    +------------------------------------------------------------------------------+
    | Accounts Blocks          | Blocks for storing account metadata, data, and    |
    | (Meta + Data)            | optional fields such as rent_epoch.  The blocks   |
    | (can be compressed)      | can be compressed here.                           |
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

The blocks are stored in reverse order to make the writer single-pass.
The single-pass writer design also provides optimization opportunities
such as sorting the data before persisting, pairing entries to avoid
possible padding when the target format requires alignment.
Once a file is written, it becomes immutable.

In the rest of the sections, we will begin with sections where both cold and hot
storages share in common, then we will go deeper in each individual format.

### Footer
The footer block includes information about how to read the file.  Both cold
and hot storage share the same footer format.

The footer has two parts:  footer body and footer tail.

    +----------------------------------------------------------------------------------+
    | Footer                                                                           |
    +----------------------------------------------------------------------------------+
    |   Footer Body (Size specified in the Footer Tail to allow extensions)            |
    +----------------------------------------------------------------------------------+
    |   Footer Tail (Fixed 88-bytes)                                                   |
    +----------------------------------------------------------------------------------+

Footer Tail is the first part to read when opening the accounts file.
It is a fixed 88-byte block that includes a magic number (8 bytes), the min and max
account addresses (32 bytes each), the format version of the file, and the size of
the entire footer.

The magic number indicates whether the file is a complete accounts data storage
file without truncation.  The min and max account addresses allow the reader
to quickly know whether the account of interest falls-in the account range of
this file.  The format version allows the reader to correctly parse the file
using the right verion.  The footer size tells the reader how to seek to
the offset of the Footer Body.
    
    +----------------------------------------------------------------------------------+
    | Footer Tail (88 bytes)                                                           |
    +------------------------------------+---------------------------------------------+
    | min_account_address (32 bytes)     | The minimum account address in this file.   |
    | max_account_address (32 bytes)     | The maximum account address in this file.   |
    | footer_size (8 bytes)              | The size of the footer block.               |
    | format_version (8 bytes)           | The format version of this file.            |
    | magic_number (8 bytes)             | A predefined const magic number to indicate |
    |                                    | the file type and make sure the file itself |
    |                                    | is completed without truncation.            |
    +------------------------------------+---------------------------------------------+

The Footer Body describes the structure of the file, including the offset and format of
each section.

    +------------------------------------------------------------------------------------+
    | Footer Body                                                                        |
    +------------------------------------------------------------------------------------+
    | account_meta_format (8 bytes)         | Describes account meta format.             |
    | owners_block_format (8 bytes)         | Describes owners block format.             |
    | account_index_format (8 bytes)        | Describes account index block format.      |
    | data_block_format (8 bytes)           | Describes account data format.             |
    |                                       |                                            |
    | account_meta_count (8 bytes)          | The number of account meta entries.        |
    | account_meta_entry_size (8 bytes)     | The size of each account meta entry.       |
    | account_data_block_size (8 bytes)     | The max size of each account data block    |
    |                                       | for non-blob account.                      |
    | optional_field_version  (8 bytes)     | The version of the account optional fields.|
    | program_account_starting_index        | Index to the first program account meta.   |
    | (8 bytes)                             | Non-executable account metas store first,  |
    |                                       | then the program account.  This allows us  |
    |                                       | to distinguish regular / program accounts. |
    |                                       |                                            |
    | owner_count (8 bytes)                 | The number of unique owners in this file.  |
    | owner_entry_size (8 bytes)            | The size of each owner entry in bytes.     |
    |                                       |                                            |
    | account_blocks_offset (0 bytes)       | Omitted as it's always 0.                  |
    | account_index_offset (8 bytes)        | The offset of the account address block.   |
    | owners_offset (8 bytes)               | The offset of the owners block.            |
    |                                       |                                            |
    | file_hash (32 bytes)                  | The hash of the entire file.               |
    +------------------------------------------------------------------------------------+

### Account Index Block
This block includes information about how to load an account given its address.
Its offset / format information is described in `account_index_offset` /
`account_index_format` fields in the Footer Body.

As an ideal account index format itself can already be an independent proposal,
here we only introduce one possible format to help illustrate how Account Index
Block might look like.

In the basic format, the account index block is sorted by the account's address
to allow binary search.  It has two sections: `account_addrsses` and `meta_indices`.
`account_addresses` section stores every pubkeys in sorted order.  Each account
address entry has a correspinding entry in the `meta_indices` section.

#### Hot Account Index Block

    +-------------------------------------------------------------------------------------+
    | Hot Account Index Block (Binary Search Format) -- 40 bytes per account              |
    +------------------------------------+------------------------------------------------+
    | addresses (32-byte each)           | Each entry is 32-byte (Pubkey).                |
    |                                    | Entries are either sorted or hashed, depending |
    |                                    | on the index format specified in the footer.   |
    +------------------------------------+------------------------------------------------+
    | offsets (8-byte each)              | Indices to access the account entry.           |
    |                                    | The Nth address in the addresses section       |
    |                                    | corresponds to Nth entry in offsets section.   |
    +------------------------------------+------------------------------------------------+

#### Cold Account Index Block
As we will mention later in the cold account data block format, cold accounts are stored in
compressed blocks.  As a result, accessing one cold account entry requires the offset of
its compressed data block, the intra block offset after decompression, and the uncompressed
data size.
    
    +-------------------------------------------------------------------------------------+
    | Cold Account Index Block (Binary Search Format) -- 44 bytes per account             |
    +------------------------------------+------------------------------------------------+
    | addresses (32-byte each)           | Each entry is 32-byte (Pubkey).                |
    |                                    | Entries are either sorted or hashed, depending |
    |                                    | on the index format specified in the footer.   |
    +------------------------------------+------------------------------------------------+
    | offset_infos (12-byte each)        | Information to access the cold account.        |
    |                                    | The Nth address in the addresses section       |
    |                                    | corresponds to Nth info entry                  |
    +------------------------------------+------------------------------------------------+

    +---------------------------------------------------------------------------------+
    | Cold Account Entry Offset Info                                                  |
    +---------------------------------------------------------------------------------+
    | data_block_offset (8 bytes)    | The offset to the account's data block.        |
    | intra_block_offset (2 bytes)   | The inner-block offset to the accounts' data   |
    |                                | after decompressing its data block.            |
    |                                |                                                |
    | uncompressed_data_len (2 bytes)| The length of the uncompressed_data.           |
    |                                | If this value is u16::MAX, it means the cold   |
    |                                | account has its own account block              |
    |                                | In this case, its block size can be derived    |
    |                                | by comparing the offset of the next cold index |
    |                                | entry which has a different data_block_offset. |
    +---------------------------------------------------------------------------------+

For account data that exceeds the configured data block size (default 4K), its
account data and optional fields will have their own data block that exceeds
the data block size.  For such blob accounts which account data size exceeds
the configured data block size, their `uncompressed_data_len` will be u16::MAX,
indicating it owns the entire data block.  Its account data size can be derived
by comparing its account data block offset and the next account data block offset
minus the size of its account meta entry and the size of optional fields.


### Owners Block
The owners block includes one address for each unique owner.  As multiple accounts
might share the same owner, storing unique owners in one block and accessing them
using local indices would save disk space.  In addition, as addresses are
uncompressible, having them stored separately in one block can make the compression
easier for account metas block.

    +----------------------------------------------------------------------------------+
    | Owners Block -- 32 bytes per unique owner                                        |
    +------------------------------------+---------------------------------------------+
    | owner_addresses                    | 32-byte (Pubkey) each.                      |
    |                                    | Each account-meta entry has a local index   |
    |                                    | pointing to its owner's address.            |
    +------------------------------------+---------------------------------------------+

### Accounts Blocks
Accounts blocks contain one entry for each account in this file.  Each entry of
one account includes its account metadata (or meta), data, and optional fields.
Account meta includes lamports and information about how to access its account
data, owner, and optional fields.

The size of each account meta entry is described in the `account_meta_entry_size`
field in the footer.  The `account_metas_format` field in the footer also provides
information about how to correctly read each account-meta entry.

We make the format flexible so that cold and hot storage can use different
account meta storage format.

Note that account blocks has two sections. The first section stores non-program
accounts (i.e., accounts whose data isn't executable), and the second section
stores program accounts.  The index to the first program account is stored in the
`first_program_account_index` field under the Footer.  This allows us to know
whether an account is executable or not by simply comparing its index.
In addition, this also saves us 1 bit per account.

    +-------------------------------------------------------------------------------+
    | Non-program Account Blocks                                                    |
    +-------------------------------------------------------------------------------+
    | Program Account Blocks                                                        |
    +-------------------------------------------------------------------------------+

#### Account Entry
Both hot account entry and cold account entry contain the following three
sub-entries --- account meta, account data, and optional fields:

    +-------------------------------+
    | Account Meta                  |
    +-------------------------------+
    | Account Data                  |
    +-------------------------------+
    | Optional Fields               |
    +-------------------------------+

While they share the same logical structure, hot account and cold account
can different implementations of each sub-entry.  The following subsections
describe the details.

#### Hot Account Blocks
All entries in hot storage file format are uncompressed and aligned.  As a result,
each account data will have its own aligned and uncompressed block.

    +----------------------------------------------+
    | account entry 1                              |
    +----------------------------------------------+
    | account entry 2                              |
    +----------------------------------------------+
    | ...                                          |
    +----------------------------------------------+
    | account entry n                              |
    +----------------------------------------------+

Each account entry follows the following format.  Specifically, there's a
0-7 bytes of padding to ensure the alignment:

    +-----------------------------------------------+
    | Account Meta (16 bytes)                       |
    +-----------------------------------------------+
    | Account Data (variable length)                |
    +-----------------------------------------------+
    | 0-7 bytes of padding                          |
    +-----------------------------------------------+
    | Optional fields                               |
    +-----------------------------------------------+

And below is the hot account meta format, which includes lamports, padding information,
owner index, and flags:

    +---------------------------------------------------------------------------------+
    | Hot Account Meta -- 16 bytes per account                                        |
    +---------------------------------------------------------------------------------+
    | lamports (8 bytes)             | The lamport balance of this account.           |
    |                                |                                                |
    | padding_and_owner_index        | The high 3-bits are used to store padding size |
    | (4 bytes)                      | while the remaining bits are for owner index.  |
    |                                |                                                |
    | flags (4 bytes)                | Boolean flags, including one bit for each      |
    |                                | optional field.                                |
    +---------------------------------------------------------------------------------+
    
#### Cold Account Blocks
Different from the hot account blocks, one cold account data block may contain entries
for multiple accounts.  In addition, each account data block is compressed individually
based on the `data_block_format` field in the footer.

    +-------------------------------+
    | cold account block 1          |
    +-------------------------------+
    | cold account block 2          |
    +-------------------------------+
    | ...                           |
    +-------------------------------+
    | cold account block n          |
    +-------------------------------+


Inside each cold account block, it stores a list of cold account entries:

    +-------------------------------+
    | cold account entry 1          |
    +-------------------------------+
    | cold account entry 2          |
    +-------------------------------+
    | ...                           |
    +-------------------------------+
    | cold account entry n          |
    +-------------------------------+

The cold account entry follows the same account entry format mentioned earlier.
The difference is that cold account entry does not have padding bytes between
its account data and optional fields.
    
    +-------------------------------+
    | Account Meta                  |
    +-------------------------------+
    | Account Data                  |
    +-------------------------------+
    | Optional Fields               |
    +-------------------------------+

And here's the cold account meta entry format:

    +--------------------------------------------------------------------------------+
    | Cold Account Meta Entry -- 16 bytes per account                                |
    +--------------------------------------------------------------------------------+
    | lamports (8 bytes)             | The lamport balance of this account.          |
    |                                |                                               |
    | owner_index (4 bytes)          | The index of the account's owner address in   |
    |                                | the owners block.                             |
    | flags (4 bytes)                | Flags include executable and whether this     |
    |                                | account has a particular optional field.      |
    +--------------------------------------------------------------------------------+

## Performance Difference Between Hot and Cold Accounts (To be completed)

## Backwards Compatibility
While the existing AppendVec and the proposed can support the existing AccountsDB
API, additional steps are needed in snapshot format to handle the migration period.

As the proposed format includes extra fields describing the format of each block
section, it is extensible to allow different implementations of different block
sections that ensures backward compatibility.

## Research Projects
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
