# The Transaction

### Components of a `Transaction`

* **Transaction:**
  * **message:** Defines the transaction
    * **header:** Details the account types of and signatures required by
      the transaction
      *  **num_required_signatures:** The total number of signatures
         required to make the transaction valid.
      *  **num_credit_only_signed_accounts:** The last
         `num_credit_only_signed_accounts` signatures refer to signing
         credit only accounts. Credit only accounts can be used concurrently
         by multiple parallel transactions, but their balance may only be
         increased, and their account data is read-only.
      *  **num_credit_only_unsigned_accounts:** The last
         `num_credit_only_unsigned_accounts` public keys in `account_keys` refer
         to non-signing credit only accounts
    * **account_keys:** List of public keys used by the transaction, including
      by the instructions and for signatures. The first
      `num_required_signatures` public keys must sign the transaction.
    * **recent_blockhash:** The ID of a recent ledger entry. Validators will
      reject transactions with a `recent_blockhash` that is too old.
    * **instructions:** A list of [instructions](instruction.md) that are
      run sequentially and committed in one atomic transaction if all
      succeed.
  * **signatures:** A list of signatures applied to the transaction. The
    list is always of length `num_required_signatures`, and the signature
    at index `i` corresponds to the public key at index `i` in `account_keys`.
    The list is initialized with empty signatures (i.e. zeros), and
    populated as signatures are added.
  
### Transaction Signing

A `Transaction` is signed by using an ed25519 keypair to sign the
serialization of the `message`. The resulting signature is placed at the
index of `signatures` matching the index of the keypair's public key in
`account_keys`.

### Transaction Serialization

`Transaction`s (and their `message`s) are serialized and deserialized
using the [bincode](https://crates.io/crates/bincode) crate with a
non-standard vector serialization that uses only one byte for the length
if it can be encoded in 7 bits, 2 bytes if it fits in 14 bits, or 3
bytes if it requires 15 or 16 bits. The vector serialization is defined
by Solana's
[short-vec](https://github.com/solana-labs/solana/blob/master/sdk/src/short_vec.rs).
