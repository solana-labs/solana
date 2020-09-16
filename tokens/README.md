# Distribute Solana tokens

A user may want to make payments to multiple accounts over multiple iterations.
The user will have a spreadsheet listing public keys and token amounts, and
some process for transferring tokens to them, and ensuring that no more than the
expected amount are sent. The command-line tool here automates that process.

## Distribute tokens

Send tokens to the recipients in `<RECIPIENTS_CSV>`.

Example recipients.csv:

```text
recipient,amount,lockup_date
3ihfUy1n9gaqihM5bJCiTAGLgWc5zo3DqVUS6T736NLM,42.0,
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT,43.0,
```

```bash
solana-tokens distribute-tokens --from <KEYPAIR> --input-csv <RECIPIENTS_CSV> --fee-payer <KEYPAIR>
```

Example transaction log before:

```text
recipient,amount,finalized_date,signature
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv,70.0,2020-09-15T23:29:26.879747Z,UB168XhBhecxzeD1w2ZRUhwTHpPSqv2WNh8NrZHqz1F2EqxxbSW6iFfVtsg3HkU9NX2cD7R92D8VRLSyArZ9xKQ
```

Send tokens to the recipients in `<RECIPIENTS_CSV>` if the distribution is
not already recorded in the transaction log.

```bash
solana-tokens distribute-tokens --from <KEYPAIR> --input-csv <RECIPIENTS_CSV> --fee-payer <KEYPAIR>
```

Example output:

```text
Recipient                                     Expected Balance (◎)
3ihfUy1n9gaqihM5bJCiTAGLgWc5zo3DqVUS6T736NLM  42
UKUcTXgbeTYh65RaVV5gSf6xBHevqHvAXMo3e8Q6np8k  43
```


Example transaction log after:

```bash
solana-tokens transaction-log --output-path transactions.csv
```

```text
recipient,amount,signature
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv,70.0,2020-09-15T23:29:26.879747Z,UB168XhBhecxzeD1w2ZRUhwTHpPSqv2WNh8NrZHqz1F2EqxxbSW6iFfVtsg3HkU9NX2cD7R92D8VRLSyArZ9xKQ
3ihfUy1n9gaqihM5bJCiTAGLgWc5zo3DqVUS6T736NLM,42.0,2020-09-15T23:31:50.264241Z,53AVNEVpQBteJBRAKp6naxXsgESDjqe1ge9Dg2HeCSpYWTuGTLqHrBpkHTnpvPJURNgKWxkJfihuRa5STVRjL2hy
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT,43.0,2020-09-15T23:33:53.680821Z,4XsMfLx9D2ZxVpdJ5xdkV2w4X4SKEQ5zbQhcH4NcRwgZDkdRNiZjvnMFaWaWHUh5eF1LwFPpQdjn6mzSsiCVj3L7
```

### Calculate what tokens should be sent

List the differences between a list of expected distributions and the record of what
transactions have already been sent.

```bash
solana-tokens distribute-tokens --dry-run --input-csv <RECIPIENTS_CSV>
```

Example recipients.csv:

```text
recipient,amount,lockup_date
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv,80,
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr,42,
```

Example output:

```text
Recipient                                     Expected Balance (◎)
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv  10
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr  42
```

## Distribute tokens: transfer-amount

This tool also makes it straightforward to transfer the same amount of tokens to a simple list of recipients. Just add the `--transfer-amount` arg to specify the amount:

Example recipients.csv:

```text
recipient
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT
```

```bash
solana-tokens distribute-tokens --transfer-amount 10 --from <KEYPAIR> --input-csv <RECIPIENTS_CSV> --fee-payer <KEYPAIR>
```

Example output:

```text
Recipient                                     Expected Balance (◎)
6Vo87BaDhp4v4GHwVDhw5huhxVF8CyxSXYtkUwVHbbPv  10
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr  10
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT  10
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
