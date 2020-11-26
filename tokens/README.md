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
Recipient                                     Expected Balance
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
Recipient                                     Expected Balance
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
Recipient                                     Expected Balance
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

## Distribute SPL tokens

Distributing SPL Tokens works very similarly to distributing SOL, but requires
the `--owner` parameter to sign transactions. Each recipient account must be an
system account that will own an Associated Token Account for the SPL Token mint.
The Associated Token Account will be created, and funded by the fee_payer, if it
does not already exist.

Send SPL tokens to the recipients in `<RECIPIENTS_CSV>`.
*NOTE:* the CSV expects SPL-token amounts in raw format (no decimals)

Example recipients.csv:

```text
recipient,amount
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT,75400
C56nwrDVFpPrqwGYsTgQxv1ZraTh81H14PV4RHvZe36s,10000
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr,42100
7qQPmVAQxEQ5djPDCtiEUrxaPf8wKtLG1m6SB1brejJ1,20000
```

You can check the status of the recipients before beginning a distribution. You
must include the SPL Token mint address:

```bash
solana-tokens spl-token-balances --mint <ADDRESS> --input-csv <RECIPIENTS_CSV>
```

Example output:

```text
Token: JDte736XZ1jGUtfAS32DLpBUWBR7WGSHy1hSZ36VRQ5V
Recipient                                             Expected Balance            Actual Balance                Difference
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT                    75.400                      0.000                   -75.400
C56nwrDVFpPrqwGYsTgQxv1ZraTh81H14PV4RHvZe36s                    10.000  Associated token account not yet created
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr                    42.100                      0.000                   -42.100
7qQPmVAQxEQ5djPDCtiEUrxaPf8wKtLG1m6SB1brejJ1                    20.000  Associated token account not yet created
```

To run the distribution:

```bash
solana-tokens distribute-spl-tokens --from <ADDRESS> --owner <KEYPAIR> \
    --input-csv <RECIPIENTS_CSV> --fee-payer <KEYPAIR>
```

Example output:

```text
Total in input_csv: 147.5 tokens
Distributed: 0 tokens
Undistributed: 147.5 tokens
Total: 147.5 tokens
Recipient                                             Expected Balance
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT                    75.400
C56nwrDVFpPrqwGYsTgQxv1ZraTh81H14PV4RHvZe36s                    10.000
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr                    42.100
7qQPmVAQxEQ5djPDCtiEUrxaPf8wKtLG1m6SB1brejJ1                    20.000
```

### Calculate what tokens should be sent

As with SOL, you can List the differences between a list of expected
distributions and the record of what transactions have already been sent using
the `--dry-run` parameter, or `solana-tokens balances`.

Example updated recipients.csv:

```text
recipient,amount
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT,100000
C56nwrDVFpPrqwGYsTgQxv1ZraTh81H14PV4RHvZe36s,100000
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr,100000
7qQPmVAQxEQ5djPDCtiEUrxaPf8wKtLG1m6SB1brejJ1,100000
```

Using dry-run:

```bash
solana-tokens distribute-tokens --dry-run --input-csv <RECIPIENTS_CSV>
```

Example output:

```text
Total in input_csv: 400 tokens
Distributed: 147.5 tokens
Undistributed: 252.5 tokens
Total: 400 tokens
Recipient                                             Expected Balance
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT                    24.600
C56nwrDVFpPrqwGYsTgQxv1ZraTh81H14PV4RHvZe36s                    90.000
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr                    57.900
7qQPmVAQxEQ5djPDCtiEUrxaPf8wKtLG1m6SB1brejJ1                    80.000
```

Or:

```bash
solana-tokens balances --mint <ADDRESS> --input-csv <RECIPIENTS_CSV>
```

Example output:

```text
Token: JDte736XZ1jGUtfAS32DLpBUWBR7WGSHy1hSZ36VRQ5V
Recipient                                             Expected Balance            Actual Balance                Difference
CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT                   100.000                    75.400                   -24.600
C56nwrDVFpPrqwGYsTgQxv1ZraTh81H14PV4RHvZe36s                   100.000                    10.000                   -90.000
7aHDubg5FBYj1SgmyBgU3ZJdtfuqYCQsJQK2pTR5JUqr                   100.000                    42.100                   -57.900
7qQPmVAQxEQ5djPDCtiEUrxaPf8wKtLG1m6SB1brejJ1                   100.000                    20.000                   -80.000
```
