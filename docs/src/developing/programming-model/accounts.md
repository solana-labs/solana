---
title: "Accounts"
---

## Storing State between Transactions

If the program needs to store state between transactions, it does so using
_accounts_. Accounts are similar to files in operating systems such as Linux.
Like a file, an account may hold arbitrary data and that data persists beyond
the lifetime of a program. Also like a file, an account includes metadata that
tells the runtime who is allowed to access the data and how.

Unlike a file, the account includes metadata for the lifetime of the file. That
lifetime is expressed in "tokens", which is a number of fractional native
tokens, called _lamports_. Accounts are held in validator memory and pay
["rent"](apps/rent.md) to stay there. Each validator periodically scans all
accounts and collects rent. Any account that drops to zero lamports is purged.

In the same way that a Linux user uses a path to look up a file, a Solana client
uses an _address_ to look up an account. The address is usually a 256-bit public
key.

## Signers

Transactions may include digital [signatures](terminology.md#signature)
corresponding to the accounts' public keys referenced by the transaction. When a
corresponding digital signature is present it signifies that the holder of the
account's private key signed and thus "authorized" the transaction and the
account is then referred to as a _signer_. Whether an account is a signer or not
is communicated to the program as part of the account's metadata. Programs can
then use that information to make authority decisions.

## Read-only

Transactions can mark some accounts as _read-only accounts_. The runtime permits
read-only accounts to be read concurrently by multiple programs. If a program
attempts to modify a read-only account, the transaction is rejected by the
runtime.

## Executable

If an account is marked "executable" in its metadata, it can be used by a
_loader_ to run programs. For example, a BPF-compiled program is marked
executable by the BPF loader during deployment once the loader has determined
that the BPF bytecode in the account's data is valid. No program is allowed to
modify the contents of an executable account once deployed and executable mark
is permanent.

## Creating

To create an account a client generates a _keypair_ and registers its public key
using the `SystemProgram::CreateAccount` instruction with preallocated a fixed
storage size in bytes. The current maximum size of an account's data is 10
megabytes.

An account address can be any arbitrary 256 bit value, and there are mechanisms
for advanced users to create derived addresses
(`SystemProgram::CreateAccountWithSeed`,
[`Pubkey::CreateProgramAddress`](program-derived-addresses.md)).

Accounts that have never been created via the system program can also be passed
to programs. When an instruction references an account that hasn't been
previously created the program will be passed an account that is owned by the
system program, has zero lamports, and zero data. But, the account will reflect
whether it is a signer of the transaction or not and therefore can be used as an
authority. Authorities in this context convey to the program that the holder of
the private key associated with the account's public key signed the transaction.
The account's public key may be known to the program or recorded in another
account and signify some kind of ownership or authority over an asset or
operation the program controls or performs.

## Ownership and Assignment to Programs

A created account is initialized to be _owned_ by a built-in program called the
System program and is called a _system account_ aptly. An account includes
"owner" metadata. The owner is a program ID. The runtime grants the program
write access to the account if its ID matches the owner. For the case of the
System program, the runtime allows clients to transfer lamports and importantly
_assign_ account ownership, meaning changing owner to different program ID. If
an account is not owned by a program, the program is only permitted to read its
data and credit the account.

## Runtime Capability of Programs

The runtime only permits the owner program to debit the account or modify its
data. The program then defines additional rules for whether the client can
modify accounts it owns. In the case of the System program, it allows users to
transfer lamports by recognizing transaction signatures. If it sees the client
signed the transaction using the keypair's _private key_, it knows the client
authorized the token transfer.

In other words, the entire set of accounts owned by a given program can be
regarded as a key-value store where a key is the account address and value is
program-specific arbitrary binary data. A program author can decide how to
manage the program's whole state as possibly many accounts.

After the runtime executes each of the transaction's instructions, it uses the
account metadata to verify that the access policy was not violated. If a program
violates the policy, the runtime discards all account changes made by all
instructions in the transaction and marks the transaction as failed.

### Policy

After a program has processed an instruction the runtime verifies that the
program only performed operations it was permitted to, and that the results
adhere to the runtime policy.

The policy is as follows:
- Only the owner of the account may change owner.
  - And only if the account is writable.
  - And only if the data is zero-initialized or empty.
- An account not assigned to the program cannot have its balance decrease.
- The balance of read-only and executable accounts may not change.
- Only the system program can change the size of the data and only if the system
  program owns the account.
- Only the owner may change account data.
  - And if the account is writable.
  - And if the account is not executable.
- Executable is one-way (false->true) and only the account owner may set it.
- No one modification to the rent_epoch associated with this account.

## Rent

Keeping accounts alive on Solana incurs a storage cost called _rent_ because the
cluster must actively maintain the data to process any future transactions on
it. This is different from Bitcoin and Ethereum, where storing accounts doesn't
incur any costs.

The rent is debited from an account's balance by the runtime upon the first
access (including the initial account creation) in the current epoch by
transactions or once per an epoch if there are no transactions. The fee is
currently a fixed rate, measured in bytes-times-epochs. The fee may change in
the future.

For the sake of simple rent calculation, rent is always collected for a single,
full epoch. Rent is not pro-rated, meaning there are neither fees nor refunds
for partial epochs. This means that, on account creation, the first rent
collected isn't for the current partial epoch, but collected up front for the
next full epoch. Subsequent rent collections are for further future epochs. On
the other end, if the balance of an already-rent-collected account drops below
another rent fee mid-epoch, the account will continue to exist through the
current epoch and be purged immediately at the start of the upcoming epoch.

Accounts can be exempt from paying rent if they maintain a minimum balance. This
rent-exemption is described below.

### Calculation of rent

Note: The rent rate can change in the future.

As of writing, the fixed rent fee is 19.055441478439427 lamports per byte-epoch
on the testnet and mainnet-beta clusters. An [epoch](../terminology.md#epoch) is
targeted to be 2 days (For devnet, the rent fee is 0.3608183131797095 lamports
per byte-epoch with its 54m36s-long epoch).

This value is calculated to target 0.01 SOL per mebibyte-day (exactly matching
to 3.56 SOL per mebibyte-year):

```text
Rent fee: 19.055441478439427 = 10_000_000 (0.01 SOL) * 365(approx. day in a year) / (1024 * 1024)(1 MiB) / (365.25/2)(epochs in 1 year)
```

And rent calculation is done with the `f64` precision and the final result is
truncated to `u64` in lamports.

The rent calculation includes account metadata (address, owner, lamports, etc)
in the size of an account. Therefore the smallest an account can be for rent
calculations is 128 bytes.

For example, an account is created with the initial transfer of 10,000 lamports
and no additional data. Rent is immediately debited from it on creation,
resulting in a balance of 7,561 lamports:


```text
Rent: 2,439 = 19.055441478439427 (rent rate) * 128 bytes (minimum account size) * 1 (epoch)
Account Balance: 7,561 = 10,000 (transfered lamports) - 2,439 (this account's rent fee for an epoch)
```

The account balance will be reduced to 5,122 lamports at the next epoch even if
there is no activity:

```text
Account Balance: 5,122 = 7,561 (current balance) - 2,439 (this account's rent fee for an epoch)
```

Accordingly, a minimum-size account will be immediately removed after creation
if the transferred lamports are less than or equal to 2,439.

### Rent exemption

Alternatively, an account can be made entirely exempt from rent collection by
depositing at least 2 years-worth of rent. This is checked every time an
account's balance is reduced and rent is immediately debited once the balance
goes below the minimum amount.

Program executable accounts are required by the runtime to be rent-exempt to
avoid being purged.

Note: Use the [`getMinimumBalanceForRentExemption` RPC
endpoint](jsonrpc-api.md#getminimumbalanceforrentexemption) to calculate the
minimum balance for a particular account size. The following calculation is
illustrative only.

For example, a program executable with the size of 15,000 bytes requires a
balance of 105,290,880 lamports (=~ 0.105 SOL) to be rent-exempt:

```text
105,290,880 = 19.055441478439427 (fee rate) * (128 + 15_000)(account size including metadata) * ((365.25/2) * 2)(epochs in 2 years)
```
