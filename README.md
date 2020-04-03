# Distribute Solana tokens

A user may want to make payments to multiple accounts over multiple iterations.
The user will have a spreadsheet listing public keys and token amounts, and
some process for transferring tokens to them, and ensuring that no more than the
expected amount are sent. The command-line tool here automates that process.

## Calculate what tokens should be sent

List the differences between a list of expected distributions and the record of what
transactions have already been sent.

```bash
solana-tokens distribute --dollars-per-sol <NUMBER> --dryrun <ALLOCATIONS_CSV> <TRANSACTIONS_CSV>
```

Example output:

```text
Recipient             Amount
blahblahblah          70
yayayayayada          42
nadanadanada          43
```

## Distribute tokens

Send tokens to the recipients in `<ALLOCATIONS_CSV>` if the distribution is
not already recordered in the transaction log.

```bash
solana-tokens distribute --from <SENDER_KEYPAIR> --dollars-per-sol <NUMBER> <ALLOCATIONS_CSV> <TRANSACTIONS_CSV> --fee-payer <KEYPAIR>
```

Example output:

```text
Recipient             Amount
blahblahblah          70
yayayayayada          42
nadanadanada          43
```

Example transaction log before:

```text
recipient,amount,signature
blahblahblah,30,blah,orig
```

Example transaction log after:

```text
recipient,amount,signature
blahblahblah,30,orig
blahblahblah,70,blah
yayayayayada,42,yada
nadanadanada,43,nada
```
