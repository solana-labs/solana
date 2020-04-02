# Reconcile total payments with current payments

A user may want to make payments to multiple accounts over multiple iterations.
The user will have a spreadsheet listing public keys and expected balances, and
some process for transferring tokens to them, and ensuring that no more than the
expected amount of tokens are sent. The command-line tool here automates that
process.

## Scrup input

Clean, group, filter as needed.

```bash
solana-batch scrub <INPUT_CSV>
```

Example output:

```text
recipient,amount
blahblahblah,100
yayayayayada,42
nadanadanada,43
```

## Reconcile

List the differences between a list of expected payments and the record of what
payments have already been made.

```bash
solana-batch transfer --dryrun <INPUT_CSV> <STATE_CSV>
```

Example output:

```text
Recipient Address     Amount
blahblahblah          70
yayayayayada          42
nadanadanada          43
```


```bash
solana-batch transfer --from <SENDER_KEYPAIR> <INPUT_CSV> <STATE_CSV> --fee-payer <KEYPAIR>
```

Example output:

```text
Recipient Address     Amount    Signature
blahblahblah          70        blah
yayayayayada          42        yada
nadanadanada          43        nada
```

Example state before:

```text
recipient,amount,signature
blahblahblah,30,blah,orig
```

Example state after:

```text
recipient,amount,signature
blahblahblah,30,orig
blahblahblah,70,blah
yayayayayada,42,yada
nadanadanada,43,nada
```
