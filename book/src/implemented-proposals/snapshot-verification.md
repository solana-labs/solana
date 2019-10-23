# Snapshot Verification

## Problem

When a validator boots up from a snapshot, it needs a way to verify the account set matches what the rest of the network sees quickly. A potential
attacker could give the validator an incorrect state, and then try to convince it to accept a transaction that would otherwise be rejected.

## Solution

Currently the bank hash is derived from hashing the delta state of the accounts in a slot which is then combined with the previous bank hash value.
The problem with this is that the list of hashes will grow on the order of the number of slots processed by the chain and become a burden to both
transmit and verify successfully.

Another naive method could be to create a merkle tree of the account state. This has the downside that with each account update, the merkle tree
would have to be recomputed from the entire account state of all live accounts in the system.

To verify the snapshot, we do the following:

On account store of non-zero lamport accounts, we hash the following data:

* Account owner
* Account data
* Account pubkey
* Account lamports balance
* Fork the account is stored on

Use this resulting hash value as input to an expansion function which expands the hash value into an image value.
The function will create a 440 byte block of data where the first 32 bytes are the hash value, and the next 440 - 32 bytes are
generated from a Chacha RNG with the hash as the seed.

The account images are then combined with xor. The previous account value will be xored into the state and the new account value also xored into the state.

Voting and sysvar hash values occur with the hash of the resulting full image value.

On validator boot, when it loads from a snapshot, it would verify the hash value with the accounts set. It would then
use SPV to display the percentage of the network that voted for the hash value given.

The resulting value can be verified by a validator to be the result of xoring all current account states together.

A snapshot must be purged of zero lamport accounts before creation and during verify since the zero lamport accounts do not affect the hash value but may cause
a validator bank to read that an account is not present when it really should be.

An attack on the xor state could be made to influence its value:

Thus the 440 byte image size comes from this paper, avoiding xor collision with 0 \(or thus any other given bit pattern\): \[[https://link.springer.com/content/pdf/10.1007%2F3-540-45708-9\_19.pdf](https://link.springer.com/content/pdf/10.1007%2F3-540-45708-9_19.pdf)\]

The math provides 128 bit security in this case:

```text
O(k * 2^(n/(1+lg(k)))
k=2^40 accounts
n=440
2^(40) * 2^(448 * 8 / 41) ~= O(2^(128))
```

