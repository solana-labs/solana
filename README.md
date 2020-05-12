# Distribute Solana tokens

A user may want to make payments to multiple accounts over multiple iterations.
The user will have a spreadsheet listing public keys and token amounts, and
some process for transferring tokens to them, and ensuring that no more than the
expected amount are sent. The command-line tool here automates that process.

## Distribute tokens

Send tokens to the recipients in `<BIDS_CSV>`.

Example bids.csv:

```text
primary_address,bid_amount_dollars
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv,6.6
```

```bash
solana-tokens distribute-tokens --from <KEYPAIR> --dollars-per-sol <NUMBER> --from-bids --input-csv <BIDS_CSV> --fee-payer <KEYPAIR>
```

Example transaction log before:

```text
recipient,amount,signature
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv,30,1111111111111111111111111111111111111111111111111111111111111111
```

Send tokens to the recipients in `<BIDS_CSV>` if the distribution is
not already recordered in the transaction log.

```bash
solana-tokens distribute-tokens --from <KEYPAIR> --dollars-per-sol <NUMBER> --from-bids --input-csv <BIDS_CSV> --fee-payer <KEYPAIR>
```

Example output:

```text
Recipient                                     Amount
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv  70
3ihfUy1n9gaqihM5bJCiTAGLgWc5zo3DqVUS6T736NLM  42
UKUcTXgbeTYh65RaVV5gSf6xBHevqHvAXMo3e8Q6np8k  43
```


Example transaction log after:

```bash
solana-tokens transaction-log --output-path transactions.csv
```

```text
recipient,amount,signature
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv,30,1111111111111111111111111111111111111111111111111111111111111111
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv,70,1111111111111111111111111111111111111111111111111111111111111111
3ihfUy1n9gaqihM5bJCiTAGLgWc5zo3DqVUS6T736NLM,42,1111111111111111111111111111111111111111111111111111111111111111
UKUcTXgbeTYh65RaVV5gSf6xBHevqHvAXMo3e8Q6np8k,43,1111111111111111111111111111111111111111111111111111111111111111
```

### Calculate what tokens should be sent

List the differences between a list of expected distributions and the record of what
transactions have already been sent.

```bash
solana-tokens distribute-tokens --dollars-per-sol <NUMBER> --dry-run --from-bids --input-csv <BIDS_CSV>
```

Example bids.csv:

```text
primary_address,bid_amount_dollars
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv,6.6
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv,15.4
3ihfUy1n9gaqihM5bJCiTAGLgWc5zo3DqVUS6T736NLM,9.24
UKUcTXgbeTYh65RaVV5gSf6xBHevqHvAXMo3e8Q6np8k,9.46
```

Example output:

```text
Recipient                                     Amount
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv  70
3ihfUy1n9gaqihM5bJCiTAGLgWc5zo3DqVUS6T736NLM  42
UKUcTXgbeTYh65RaVV5gSf6xBHevqHvAXMo3e8Q6np8k  43
```

## Distribute stake accounts

Distributing tokens via stake accounts works similarly to how tokens are distributed. The
big difference is that new stake accounts are split from existing ones. By splitting,
the new accounts inherit any lockup or custodian settings of the original.

```bash
solana-tokens distribute-stake --stake-account-address <ACCOUNT_ADDRESS> \
    --input-csv <ALLOCATIONS_CSV> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> --fee-payer <KEYPAIR>
```

Currently, this will subtract 1 SOL from each allocation and store it the
recipient address. That SOL can be used to pay transaction fees on staking
operations such as delegating stake. The rest of the allocation is put in
a stake account. The new stake account address is output in the transaction
log.
