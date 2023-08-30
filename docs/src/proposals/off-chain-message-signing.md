# Off-chain message signing
## Motivation

There is ecosystem demand for a method of signing non-transaction messages with
a Solana wallet. Typically this is some kind of "proof of wallet ownership" for
entry into a whitelisted system.

Some inspiration can be gleaned from relevant portions of Ethereum's
[EIP-712](https://eips.ethereum.org/EIPS/eip-712)

## Considerations

* Security
  * Off-chain message signatures should not be valid transaction message signatures
* Future-proofing
  * Versioning
  * Co-exist with versioned transaction messages and extended length transactions
* Compatibility
  * Hardware wallet signing
  * Localization

## Message Preamble

| Field | Start offset | Length (bytes) | Description
| :---- | :----------: | :------------: | :----------
| Signing Domain | 0x00 | 16 | \[[1](#signing-domain-specifier)\]
| Header version | 0x10 | 1 | \[[2](#header-version)\]
| Application domain | 0x11 | 32 | \[[3](#application-domain)\]
| Message format | 0x31 | 1 | \[[4](#message-format)\]
| Signer count | 0x32 | 1 | Number of signers committing to the message. An 8-bit, unsigned integer. **MUST NOT** be zero.
| Signers | 0x33 | `SIGNER_COUNT` * 32 | `SIGNER_COUNT` ed25519 public keys of signers
| Message length | 0x33 + `SIGNER_CNT` * 32 | 2 | Length of message in bytes. A 16-bit, unsigned, little-endian integer. **MUST NOT** be zero.

### Signing Domain Specifier

The signing domain specifier is a prefix byte string used to give similar structure
to all off-chain message signatures. We assign its value to be:
```
b"\xffsolana offchain"
```
The first byte, `\xff`, was chosen for the following reasons
1. It corresponds to a value that is illegal as the first byte in a transaction
`MessageHeader`.
1. It avoids unintentional misuse in languages with C-like, null-terminated strings.

The remaining bytes, `b"solana offchain"`, were chosen to be descriptive and
reasonably long, but are otherwise arbitrary.

This field **SHOULD NOT** be displayed to users

### Header

#### Header version

The header version is represented as an 8-bit unsigned integer. Only the version
0 header format is specified in this document

This field **SHOULD NOT** be displayed to users

#### Application domain

A 32-byte array identifying the application requesting off-chain message signing.
This may be any arbitrary bytes. For instance the on-chain address of a program,
DAO instance, Candy Machine, etc.

This field **SHOULD** be displayed to users as a base58-encoded ASCII string rather
than interpretted otherwise.

#### Message Format

Version `0` headers specify three message formats allowing for trade-offs between
compatibility and composition of messages.

| ID  | Encoding              | Maximum Length \* | Hardware Wallet Support |
| :-: | :-------------------: | :---------------: | :---------------------: |
|  0  | Restricted ASCII \*\* | 1232              | Yes                     |
|  1  | UTF-8                 | 1232              | Blind sign only         |
|  2  | UTF-8                 | 65535             | No                      |

\* Combined length of the [message preamble](#message-preamble) and message body<br/>
\*\* Those characters for which [`isprint(3)`](https://linux.die.net/man/3/isprint)
returns true.  That is, `0x20..=0x7e`.

Both the message encoding and maximum message length **MUST** be enforced by
signer and verifier.

Formats `0` and `1` are motivated by hardware wallet support where both RAM
to store the payload and font character support are limited.

This field **SHOULD NOT** be displayed to users

## Signing

Solana off-chain messages **MUST** only be signed using the ed25519 digital
signature scheme. Before signing, the message **MUST** be strictly checked to
conform to the associated preamble. The message body is then appended to the
[message preamble](#message-preamble). Finally the result is ed25519 signed.

## Verification
Upon successful ed25519 verification of _all_ attached signatures, the message
**MUST** be strictly checked to conform to the [message preamble](#message-preamble).
A message that does not conform to its preamble is invalid, regardless of the
validity of any signatures.

## Envelope

When passing around signed off-chain messages a common format is helpful. The
recommended binary representation is as follows:

| Field | Start offset | Length (bytes) | Description
| :---- | :----------: | :------------: | :----------
| Signature Count \* | 0x00 | 1 | Number of signatures. An 8-bit, unsigned integer
| Signatures | 0x01 | `SIG_COUNT` * 64 | `SIG_COUNT` ed25519 signatures
| Message Preamble | 0x01 + `SIG_COUNT` * 64 | `PREAMBLE_LEN` | The [message preamble](#message-preamble)
| Message Body | 0x01 + `SIG_COUNT * 64 + `PREAMBLE_LEN` | `MESSAGE_LEN` | The message content

The signature count **MUST** match the value of signers count from the message
preamble.

Signatures **MUST** be ordered to match their corresponding public keys as
specified in the message preamble.

## Runtime Considerations

To prevent social attacks by which the signer is tricked into signing a transaction,
the runtime **MUST NOT** accept signed off-chain messages as transactions under any
circumstances. The first byte of the [signing domain specifier](#signing-domain-specifier)
is chosen such that it corresponds to a value (`0xff`) which is implicitly illegal
as the first byte in a transaction `MessageHeader` today. The property is implicit
because the top bit in the first byte of a `MessageHeader` being set signals a
versioned transaction, but only a value of
[zero is supported](https://github.com/solana-labs/solana/blob/b6ae6c1fe17e4b64c5051c651ca2585e4f55468c/sdk/program/src/message/versions/mod.rs#L269-L281)
at this time. The runtime will need to be modified to reserve 127 as an illegal
version number, making this property explicit.

### Implementation

The runtime changes described above have been implemented in PR [#29807](https://github.com/solana-labs/solana/pull/29807)
