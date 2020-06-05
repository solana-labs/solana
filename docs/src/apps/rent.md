# Storage Rent for Accounts

Keeping accounts alive incurs storage cost called _rent_ because the cluster must actively maintain the data to process any future transactions on it. This is not like Bitcoin or Ethereum, where account storages don't incur any costs.

The rent is debited from an account's balance by the runtime upon the first access in the current epoch by transactions or once per an epoch if there are no transactions. The rent is paid upfront for the next epoch. If an account can't pay rent for the next epoch, it's purged immediately at the start of a new epoch. The fee is currently a fixed rate, measured in bytes-times-epochs. The fee may change in the future.

Accounts can be exempt from paying rent by maintaining a minimum balance, this rent-exemption is described below.

## Calculation of rent

Note: The rent rate can change in the future. And this applies to the testnet and mainnet-beta.

As of writing, the fixed rent fee is 19.055441478439427 lamports per byte-epoch. And an epoch is roughly 2 days.

Firstly, the rent calculation considers the size of an account including the metadata including its address, owner, lamports, etc. Thus the rent fee starts from 128 bytes as the minimum to be rented even if an account has no data.

For example, if a no-data account is created with the initial transfer of 10,000 lamports. The rent is immediately debited from it on creation, resulting in a balance of 7,561 lamports.

You can calculate like this:

```
7,561 = 10,000 (= transfered lamports) - 2,439 (= this account's rent fee for a epoch)
2,439 = 19.055441478439427 (= rent rate) * 128 bytes (= minimum account size) * 1 (= epoch)
```

And the account balance will be reduced to 5,122 lamports at the next epoch even if there is no activity:

```
5,122 = 7,561 (= current balance) - 2,439 (= this account's rent fee for a epoch)
```

This also indicates an account will be immediately removed after creation if the transferred lamports is less than or equal to 2,439.

## Rent exemption

Alternatively, an account can be exempt from rent collection entirely by depositing a certain amount of lamports. Such a minimum amount is defined as the 2 years worth of rent fee. This is checked every time an account's balance is reduced and the rent is immediately debited once the balance goes below the minimum amount.

Program executable account required to be rent-exempt by the runtime to avoid being purged.

Note: there is an RPC endpoint specifically to calculate this ([`getMinimumBalanceForRentExemption`](jsonrpc-api.md#getminimumbalanceforrentexemption)). Apps should rely on it. The following calculation is illustrative only.

For example, 105,290,880 lamports (=~ 0.105 SOL) is needed to be rent-exempt for a program executable with the size of 15,000 bytes:

```
105,290,880 = 19.055441478439427 (= fee rate) * (128 + 15_000)(= account size including metadata) * ((365.25/2) * 2)(=epochs in 2 years)
```
