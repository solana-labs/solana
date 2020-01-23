# Anatomy of a Transaction

This chapter documents the binary format of a transaction.

## Transaction Format

A transaction contains a [compact-array](#Compact-Array-Format) of signatures,
followed by a [message](#Message-Format).  Each item in the signatures array is
a [digital signature](#Signature-Format) of the given message. The Solana
runtime verifies that the number of signatures matches the number in the first
8 bits of the [message header](#Message-Header-Format). It also verifies that
each signature was signed by the private key corresponding to the public key at
the same index in the message's account addresses array.

### Signature Format

Each digital signature is in the ed25519 binary format and consumes 64 bytes.


## Message Format

A message contains a [header](#Message-Header-Format), followed by a
compact-array of [account addresses](#Account-Address-Format), followed by a
recent [blockhash](#Blockhash-Format), followed by a compact-array of
[instructions](#Instruction-Format).

### Message Header Format

The message header contains three unsigned 8-bit values. The first value is the
number of required signatures in the containing transaction. The second value
is the number of those corresponding account addresses that are read-only.  The
third value in the message header is the number of read-only account addresses
not requiring signatures.

### Account Addresses Format

The addresses that require signatures appear at the beginning of the account
address array, with addresses requesting write access first and read-only
accounts following. The addresses that do not require signatures follow the
addresses that do, again with read-write accounts first and read-only accounts
following.


### Blockhash Format

A blockhash contains a 32-byte SHA-256 hash. It is used to indicate when a
client last observed the ledger. Validators will reject transactions when the
blockhash is too old.


## Instruction Format

An instruction contains a program ID index, followed by a compact-array of
account address indexes, followed by a compact-array of opaque 8-bit data. The
program ID index is used to identify an on-chain program that can interpret the
opaque data.  The program ID index is an unsigned 8-bit index to an account
address in the message's array of account addresses. The account address
indexes are each an unsigned 8-bit index into that same array.


## Compact-Array Format

A compact-array is serialized as the array length, followed by each array item.
The array length is a special multi-byte encoding called compact-u16.

### Compact-u16 Format

A compact-u16 is a multi-byte encoding of 16 bits. The first byte contains the
lower 7 bits of the value in its lower 7 bits.  If the value is above 0x7f, the
high bit is set and the next 7 bits of the value are placed into the lower 7
bits of a second byte. If the value is above 0x3fff, the high bit is set and
the remaining 2 bits of the value are placed into the lower 2 bits of a third
byte.

## Account Address Format

An account address is 32-bytes of arbitrary data. When the address requires a
digital signature, the runtime interprets it as the public key of an ed25519
keypair.
