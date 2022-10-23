---
title: Reduce the size of AppendVec storage in AccountsDB
---

## Problems
The storage format, StoredAccountMeta, of each entry in the AppendVec currently
has 129 bytes overhead + the size of account data for each account + extra
bytes for alignment.

StoredAccountMeta currently has the following format:

```
pub struct StoredAccountMeta<'a> {
    pub meta: &'a StoredMeta {
        pub write_version: StoredMetaWriteVersion, // 8 bytes
        pub pubkey: Pubkey,  // 32 bytes
        pub data_len: u64,   //  8 bytes
    },
    pub account_meta: &'a AccountMeta {
        pub lamports: u64,     //  8 bytes
        pub owner: Pubkey,     // 32 bytes
        pub executable: bool,  //  1 byte
        pub rent_epoch: Epoch, //  8 bytes
    },
    pub data: &'a [u8],     //  N bytes
    pub offset: usize,      //  0 bytes in storage, 8 bytes in memory
    pub stored_size: usize, //  0 bytes in storage, 8 bytes in memory
    pub hash: &'a Hash,     // 32 bytes
}

```

## Workload
TODO(yhchiang): add workload

* Account DB Hash calculation (scan)
* Transaction (point-lookup)

## Proposed Solution

### Overview

The new accounts data storage file consists of the following blocks:

```
+---------------------------------------------------------------------------+
| account data blocks      | Account data in compressed blocks.             |
| (compressed)             | Optional fields such as rent_epoch are also    |
|                          | stored here.                                   |
+---------------------------------------------------------------------------+
| account metas block      | 20 bytes per account                           |
|                          | For accounts with its data smaller than        |
|                          | account_data_block_size                        |
+---------------------------------------------------------------------------+
| account addresses block  | 32 bytes (Pubkey) per account                  |
+---------------------------------------------------------------------------+
| owners block             | 32 bytes (Pubkey) per unique owner             |
+---------------------------------------------------------------------------+
| footer                   | Stores information about how to read the file  |
|                          | and sanity check.                              |
+---------------------------------------------------------------------------+
```

To make the writer single pass, the file reader first read the footer located
at the ending part of the file to obtain information about how to read the
entire file such as the size of the footer and the offsets of each block.

### Format Details

#### Footer Block
The footer block includes essential information about how to read the file.

The file reader reads from the ending part of the file, obtaining essential
information about how to read the file.

In its end, it includes a magic number, indicating the file is a complete
accounts data storage file without truncation.  Then, it includes a format
version that allows the reader to know how to correctly read the file. The
version is followed by the size of the footer, allow the reader to know
how much bytes it has to read in order to obtain the complete information
about the file.  Then, it includes the file hash that allows the reader
to perform sanity and consistent check.

The remaining part of the footer describes the structure of the file,
including the offsets of each block, and the number of entries and the
size of entries in each block.

```
+----------------------------------------------------------------------------------+
| Footer                                                                           |
+----------------------------------------------------------------------------------+
| account_meta_count (8 bytes)       | The number of account meta entries.         |
| account_meta_entry_size (8 bytes)  | The size of each account meta entry.        |
| account_data_block_size (8 bytes)  | The size of account data block.             |
|                                    | (Deafult: 4k, uncompressed)                 |
| owner_count (8 bytes)              | The number of unique owners in this file.   |
| owner_entry_size (8 bytes)         | The size of each owner entry in bytes.      |
| account_metas_offset (8 bytes)     | The offset of the account metas block.      |
| account_addresses_offset (8 bytes) | The offset of the account address block.    |
| owners_offset (8 bytes)            | The offset of the owners block.             |
+------------------------------------+---------------------------------------------+
| file_hash (32 bytes)               | The hash of the entire file.                |
| footer_size (8 bytes)              | The size of the footer block.               |
| format_version (8 bytes)           | The format version of this file.            |
| magic_number (8 bytes)             | A predefined const magic number to indicate |
|                                    | the file type and make sure the file itself |
|                                    | is completed without truncation.            |
+------------------------------------+---------------------------------------------+
```

#### Account Meta Block
Account meta block, as suggested by its name, stores the metadata of each account.
Each entry has the same size described in the footer block.

Note that optional fields of an account are stored separately in the account data blocks.

```
+--------------------------------------+
| account meta entry 1                 |
+--------------------------------------+
| account meta entry 2                 |
+--------------------------------------+
|         ...                          |
+--------------------------------------+
| account meta entry N                 |
+--------------------------------------+
```

```
+------------------------------------------------------------------------------------------+
| Account Meta Entry (26 bytes)                                                            |
+------------------------------------------------------------------------------------------+
| lamport (8 bytes)                 | The lamport balance of this account.                 |
| block_offset (8 bytes)            | The offset of the account's data block in the file.  |
|                                   | (Note that the compressed size of each data block is |
|                                   | omitted here as it can be dynamically derived by the |
|                                   | difference between the offsets of two consecutive    |
|                                   | data blocks.)                                        |
| owner_index (4 bytes)             | The index of the account's owner address in the      |
|                                   | owners block.                                        |
+-----------------------------------+------------------------------------------------------+
| uncompressed_data_size (2 bytes)  | The uncompressed size of the account data.           |
|                                   | This number is std::u16::MAX if the account data     |
|                                   | exceeds the account data block size.                 |
|                                   | (default 4k, defined in the footer)                  |
| intra_block_offset (2 bytes)      | The uncompressed offset of the account data inside   |
|                                   | its (possibly shared) account data block.            |
|                                   | This number is always 0 for those account data that  |
|                                   | has its own block.                                   |
| flags (1 byte)                    | Flags such as executable, whether the account        |
|                                   | has marker or rent epoch                             |
| optional_fields_size (1 byte)     | The total size of the optional fields appended       |
|                                   | in its data block.                                   |
+------------------------------------------------------------------------------------------+

```

#### Account Pubkey Block
The account pubkey is accessed via the `local_id` of the `StoredAccountMetaV2`,
which is derived from the location of one account meta when an AppendVec is
first opened (i.e., it's location inside the vector).  Specifically, the account
pubkey of one account is located at

```
account_pubkey_block_offset + HASH_SIZE (32) + local_id * ACCOUNT_PUBKEY_SIZE
```

Again, as the offset of each block is aligned.  A 32-byte hash entry means each
entry is naturally 64-bit aligned without padding.

```
+--------------------------------------+
| Hash (32 bytes for the entire block) |
+--------------------------------------+
| account address 1                    |
+--------------------------------------+
| account address 2                    |
+--------------------------------------+
|         ...                          |
+--------------------------------------+
| account address N                    |
+--------------------------------------+
```

```
+-------------------------------------------------------------+
| Account Address (32 bytes each)                             |
+-------------------------------------------------------------+
| address         | 32 bytes                                  |
+-------------------------------------------------------------+
```

#### Owners Block
The owners block stores the owner's information.  An owner entry can be accessed
via `owner_block_offset` + HASH_SIZE (32) + `owner_local_id` * `OWNER_ENTRY_SIZE`.

Again, as the offset of each block is aligned.  A 32-byte hash entry means each
entry is naturally 64-bit aligned without padding.

Note that the existing 8-byte `write_version` field in `StoredMeta` is removed
in this design as we now have AccountsCache as a write cache so that we don't
need to maintain the `write_version` to enable multiple AppendVec per slot.

```
+--------------------------------------+
| Hash (32 bytes for the entire block) |
+--------------------------------------+
| owner 1                              |
+--------------------------------------+
| owner 2                              |
+--------------------------------------+
|         ...                          |
+--------------------------------------+
| owner N                              |
+--------------------------------------+
```

```
+-------------------------------------------------------------+
| Owner (32 bytes each)                                       |
+-------------------------------------------------------------+
| pubkey          | 32 bytes                                  |
+-------------------------------------------------------------+
```

#### Account Data Blocks
Account data blocks are compressed data blocks that stores account data.

There're two types of account data blocks: regular account data block and
blob account data block.  For account data that is smaller than the account
data block size (default 4K) specified in the footer will share its account
data block with other accounts.  For account data that is bigger than the
account data block size is called blob account data.  It has its own account
data block with variable size.

##### Regular Account Data Block

```
+-----------------------------------+
| Account data block                |
+-----------------------------------+
| Account data 1                    |
| Optional fields of account 1      |
+-----------------------------------+
| Account data 2                    |
| Optional fields of account 2      |
+-----------------------------------+
| ...                               |
+-----------------------------------+
| Account data N                    |
| Optional fields of account N      |
+-----------------------------------+
```

##### Blob Account Data Block
Blob account data block has exactly the same format as the regular acocunt
data block except it has only one account associated with it.

```
+-----------------------------------+
| Blob account data block           |
+-----------------------------------+
| Account data 1                    |
| Optional fields of account 1      |
+-----------------------------------+
```
