# Storage Rent for Accounts

Keeping accounts alive on Solana incurs a storage cost called _rent_ because the cluster must actively maintain the data to process any future transactions on it. This is different from Bitcoin and Ethereum, where storing accounts doesn't incur any costs.

The rent is debited from an account's balance by the runtime upon the first access (including the initial account creation) in the current epoch by transactions or once per an epoch if there are no transactions. The fee is currently a fixed rate, measured in bytes-times-epochs. The fee may change in the future.

For the sake of simple rent calculation, rent is always collected for a single, full epoch. Rent is not pro-rated, meaning there are neither fees nor refunds for partial epochs. This means that, on account creation, the first rent collected isn't for the current partial epoch, but collected up front for the next full epoch. Subsequent rent collections are for further future epochs. On the other end, if the balance of an already-rent-collected account drops below another rent fee mid-epoch, the account will continue to exist through the current epoch and be purged immediately at the start of the upcoming epoch.

Accounts can be exempt from paying rent if they maintain a minimum balance. This rent-exemption is described below.

## Calculation of rent

Note: The rent rate can change in the future.

As of writing, the fixed rent fee is 19.055441478439427 lamports per byte-epoch on the testnet and mainnet-beta clusters. An [epoch](../terminology.md#epoch) is targeted to be 2 days (For devnet, the rent fee is 0.3608183131797095 lamports per byte-epoch with its 54m36s-long epoch).

This value is calculated to target 0.01 SOL per mebibyte-day (exactly matching to 3.56 SOL per mebibyte-year):

```
Rent fee: 19.055441478439427 = 10_000_000 (0.01 SOL) * 365(approx. day in a year) / (1024 * 1024)(1 MiB) / (365.25/2)(epochs in 1 year)
```

And rent calculation is done with the `f64` precision and the final result is truncated to `u64` in lamports.

The rent calculation includes account metadata (address, owner, lamports, etc) in the size of an account. Therefore the smallest an account can be for rent calculations is 128 bytes.

For example, an account is created with the initial transfer of 10,000 lamports and no additional data. Rent is immediately debited from it on creation, resulting in a balance of 7,561 lamports:


```
Rent: 2,439 = 19.055441478439427 (rent rate) * 128 bytes (minimum account size) * 1 (epoch)
Account Balance: 7,561 = 10,000 (transfered lamports) - 2,439 (this account's rent fee for an epoch)
```

The account balance will be reduced to 5,122 lamports at the next epoch even if there is no activity:

```
Account Balance: 5,122 = 7,561 (current balance) - 2,439 (this account's rent fee for an epoch)
```

Accordingly, a minimum-size account will be immediately removed after creation if the transferred lamports are less than or equal to 2,439.

## Rent exemption

Alternatively, an account can be made entirely exempt from rent collection by depositing at least 2 years-worth of rent. This is checked every time an account's balance is reduced and rent is immediately debited once the balance goes below the minimum amount.

Program executable accounts are required by the runtime to be rent-exempt to avoid being purged.

Note: Use the [`getMinimumBalanceForRentExemption` RPC endpoint](jsonrpc-api.md#getminimumbalanceforrentexemption) to calculate the minimum balance for a particular account size. The following calculation is illustrative only.

For example, a program executable with the size of 15,000 bytes requires a balance of 105,290,880 lamports (=~ 0.105 SOL) to be rent-exempt:

```
105,290,880 = 19.055441478439427 (fee rate) * (128 + 15_000)(account size including metadata) * ((365.25/2) * 2)(epochs in 2 years)
```
