The external Budget language is defined by the following Transactions.  Note that this is not currently 1-1
with the src/budget.rs DSL.

A compiler will convert the input below into individual Transactions.
Initially this compiler will live within `solana-wallet`.

Notes:
* `key[0]` is always the sender's key and is automatically added by the wallet before sending
* `fee` is not described here.  Assumed to be added at transaction send time
* `key[1]` is always the recipient, but the funds may be locked for a time
* get_balance for `key[1]` returns 0 until the funds are unlocked
* issuing a new "transfer" to a locked `key[1]` will be rejected
* `key[2]` is supplied only when creating a "transfer" requiring a witness to complete.

###  Unconditional Immediate Transfer
```yml
transfer:
  tokens: 120
  to: $key[1]
```
or
```json
{
  "transfer": {
    "tokens": 120,
    "to": "$key[1]"
  }
}
```
or
```sh
$ solana-wallet pay --tokens 123 --to $key[1]
```

### Time-based Transfer
Contract creator must issue an "timestamp" transaction to unlock the tokens.
That is, elapsed time is defined by the contract creator.

```yml
transfer:
  tokens: 120
  to: $key[1]
  at: 2018-12-24T23:59
```
or
```sh
$ solana-wallet pay --tokens 123 --to $key[1] --at 2018-12-24T23:59
```

### Witness-based Transfer
A third party, `key[2]`, must issue a "witness" transaction to unlock the tokens.

```yml
transfer:
  tokens: 120
  to: $key[1]
  witness: $key[2]
```
or
```sh
$ solana-wallet pay --tokens 123 --to $key[1] --witness $key[2]
```

### Time and Witness Transfer
```yml
transfer:
  tokens: 120
  to: $key[1]
  at: 2018-12-24T23:59
  witness: $key[2]
```
or
```sh
$ solana-wallet pay --tokens 123 --to $key[1] --at 2018-12-24T23:59 --witness $key[2]
```

### Cancel Transfer
Only the `key[0]` that created the "transfer" may cancel it,
and the "transfer" may only be cancelled if it is still locked
```yml
cancel: $key[1]
```
or
```sh
$ solana-wallet cancel-transfer $key[1]
```

### Approve Transfer
The `key[0]` of this transaction must be equal to the "witness" field in the "transfer"
```yml
witness:
  to: $key[1]
```
or
```sh
$ solana-wallet witness-transfer $key[1]
```

### Indicate Elapsed Time.
Apply the timestamp to `key[1]`, potentially unlocking a time-based transfer
Only the `key[0]` that created the "transfer" may perform this transaction.
```yml
timestamp:
  to: $key[1]
  when: 2018-12-24T23:59
```
or
```sh
$ solana-wallet send-timestamp $key[1] 2018-12-24T23:59
```
