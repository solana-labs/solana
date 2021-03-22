---
title: Interest-Bearing Tokens
---

This document describes a solution for interest-bearing tokens as an on-chain
program. Interest-bearing tokens continuously accrue interest as time passes.

## Problem

Yield on blockchain assets is a crucial feature of many decentralized finance
applications, as token holders want to be compensated for providing their tokens
as liquidity to applications. The concept of "yield farming" on tokens has even
spawned very successful tongue-in-cheek projects, such as
[$MEME farming](https://dontbuymeme.com/).

Tokens that accrue interest are an attractive and popular version of yield farming,
as seen most prominently with Aave's [aTokens](https://aave.com/atokens).

### How do these work?

Let's explain through an example: assume there's an interest-bearing token mint
with a total supply of 1,000 tokens
and a 10% annual interest rate. This means that one year later, the total supply
will be 1,100 tokens, and all token holders will have 10% more tokens in their
holding account.

Since interest is accrued continuously, however, just 1 second later, the total
supply has again increased. A 10% annualized rate gives us a continuous interest
rate of ~9.5% (`ln(1.1)`), which allows us to calculate the new instantaneous supply.

```
new_supply = previous_supply * exp(continuous_rate / seconds_per_year)
new_supply = 1100 * exp(0.095 / 31536000)
new_supply = 1100.0000033...
```

Since interest-bearing tokens are constantly accruing interest, they do not have the same
properties as normal tokens. Most importantly, interest-bearing tokens held in two different
accounts are only fungible if they have been updated to the same time. For example,
10 interest-bearing tokens in the year 1999 are worth much more than 10 interest-bearing tokens in the year 3000, since
those tokens from 1999 have over 1,000 years of interest (see Futurama). Because
of this, we have to compound interest before any minting, burning, and
transferring.

### Initial Attempt

The concept seems simple to implement. We can create an SPL token, allow
tokens to be minted by a new "interest accruing" program, and then make sure to
call it regularly.

This solution has a few problems. First, how do we compound interest regularly?
We can create a separate service which periodically sends transactions to update
all of the accounts. Unfortunately, this will cost the service a lot, especially if
there are thousands of accounts to update.

We can enforce compounding before any transfer / mint / burn, but then all programs
and web consumers need to change to make these update calls. How do we change programs
and wallets to properly update accounts without creating a big `if is_i_token` case
all over the code?

This solution creates friction for almost all SPL token users: on-chain programs,
wallets, applications, and end-users.

## Proposed Solution

Following the approach taken by Aave, we can lean on a crucial feature of interest-bearing tokens:
although the holding's token amount is constantly changing, each token holder's
proportion of total supply stays the same. Therefore, we can satisfy the
fungibility requirement for tokens on proportional ownership. The token
amount used for token transfers / mints / burns is just an interface, and
everything is converted to proportional shares inside the program.

Let's go through an example. Imagine we have an interest-bearing token mint with a total of
10,000 proportional shares, a 20% annual interest rate, and 1 share is currently
worth 100 tokens. If I transfer 1,000 tokens today, the program converts that and moves
10 shares. One year later, 1 share is worth 120 tokens, so if I transfer
1,000 tokens, the program converts and only moves 8.33 shares. The same concept
applies to mints and burns.

Following this model, we never have to worry about compounding interest. We
simply need to call the right program to perform instructions.

## Required Changes

A new interest-bearing token program is only one component of the overall solution,
which also comprises an interface for tokens (i-tokens) to be easily integrated in
on-chain programs, web apps, and wallets.

### Token Program Conformance

Since our solution entails the creation of a new token program, we're opening the
floodgates for other app developers to do the same. To ensure safe and easy integration
of new token programs, we need a set of conformance tests based on the current
SPL token test suite.

A new token program must include all of the same instructions as SPL token,
but the "unchecked" variants should return errors. Concretely, this means the
program must implement:

* `InitializeMint`
* `InitializeHolding` (previously `InitializeAccount`)
* `Revoke`
* `SetAuthority`
* `CloseHolding` (previously `CloseAccount`)
* `FreezeHolding` (previously `FreezeAccount`)
* `ThawHolding` (previously `ThawAccount`)
* `TransferChecked`
* `ApproveChecked`
* `MintToChecked`
* `BurnChecked`
* `InitializeHoldingWithOwnerAddress` (previously `InitializeAccount2`)

And the program should throw errors for:

* `Transfer`
* `Approve`
* `MintTo`
* `Burn`
* `InitializeMultisig` (use a more general multisig)

New instructions required:

* `CreateHolding`: only performs `Allocate` and `Assign` to self,
useful for creating a holding when you don't know the program that you're
interacting with. See the Associated Token Account section for how to use this.
* `TransferAllChecked`: transfers everything from the source holding to the
destination holding. Useful for closing a interest-bearing token holding since the
balance is constantly changing.

There are also new read-only instructions, which write data to a provided account
buffer:

* `NormalizeToSPLHolding`: write holding data in SPL token format
* `NormalizeToSPLMint`: write mint data in SPL token format

See the Runtime section for more information on how these are used.

Programs and wallets that wish to support i-tokens must update the instructions
to use all `Checked` variants and use the new read-only instructions to fetch
information.

The SPL token library provides wrappers compatible with any conforming program.
For the base SPL token program, the wrapper simply deserializes the account data
and returns the relevant data. For other programs, it calls the appropriate
read-only instruction using an ephemeral account input.

For interest-bearing tokens, after `NormalizeToSPLHolding` and deserialization
into a `spl_token::Holding`, the `amount` field will be in terms of tokens, and
not shares, so that UIs can properly display amounts in terms of tokens.

### Token Program Registry

We need the [token-list](https://github.com/solana-labs/token-list) to include
vetted SPL token-conforming programs and group known mints by their program id.

The token list will publish a file of known program ids at
[token-list releases](https://github.com/solana-labs/token-list/releases/latest),
to be used by RPC and programs.

TODO consider adding Rust and C versions of the registry for on-chain programs and RPC

TODO consider detailing the process of including a new program

### Runtime

#### Ephemeral Accounts

In order to reduce friction when using read-only instructions on other programs,
we need the ability for on-chain programs to dynamically create "ephemeral
accounts", which only contain data, can be passed through CPI to other programs,
and disappear after the instruction.

For a program using i-tokens, to get the balance of a holding, the program creates
an ephemeral account with 165 bytes of data, passes it to the i-token program's
`NormalizeToSPLHolding` instruction, then deserializes the data back into the base
`spl_token::Holding`. At the end of the instruction, the account disappears.

TODO What's the best approach to implement this?  For example, does an ephemeral
account even need a public key?

This solution avoids the need to pass additional scratch accounts to almost all
programs that use tokens.

TODO what limits do we consider for these? A certain number of accounts?
A total amount of bytes? Regarding security, this breaks the previous
check that a CPI must use a subset of accounts provided to the calling program.

#### Dynamic sysvars

The interest-bearing token program also requires the use of dynamic sysvars. For example,
during the `NormalizeToSPLHolding` instruction, the program needs to know the current
time in order to properly convert from token amount to share amount.

Without dynamic sysvars, we need to create a system to tell what sysvars are
needed for different instructions on each program, which creates complications
for all users.

### RPC

The validator's RPC server needs to properly handle new token program accounts
for read-only instructions and pre / post balances.

Performance will be a concern when it comes to calling into the bank to perform
serialization into an ephemeral account.

The BPF compute cost of read-only transactions may need to be lowered specially
for read-only RPC calls. The interest-bearing token program will probably be one of the more
cost-intensive computations possible since it is performing present value
discounting.

#### `getBalance` and `getSupply`

Instead of deserializing SPL token accounts and returning the data, the
RPC server runs read-only transactions on the i-token program.

RPC simply uses the same flow as on-chain programs: create an ephemeral account
then runs the read-only transaction onto the bank requested.

#### `preTokenBalances` and `postTokenBalances`

As mentioned earlier in the Token Program Conformance section, the i-token
program must implement instructions to serialize a mint and holding into the
SPL format. RPC will call into these instructions before and after the transaction
is run to generate `preTokenBalances` and `postTokenBalances`.

#### New Secondary Indexes

Token-specific RPC calls need smarter secondary indexes to pick up accounts from
new token programs.  These include:

* `getTokenAccountBalance`
* `getTokenAccountsByDelegate`
* `getTokenAccountsByOwner`
* `getTokenLargestAccounts`
* `getTokenSupply`

TODO any others?

#### Vetted Token Programs

RPC will learn about the known program ids through a file published in the
[token-list releases](https://github.com/solana-labs/token-list/releases/latest),
downloaded automatically at startup.

### Associated Token Program

The Associated Token Program needs to support all token programs seamlessly.
Currently, it performs the following sequence:

* `SystemInstruction::Transfer` the required lamports to make the SPL holding account rent-exempt
* `SystemInstruction::Allocate` space for the SPL holding account
* `SystemInstruction::Assign` the holding account to the SPL token program
* `InitializeHolding` on SPL token program

This does not work for an opaque token program, because we do not know the size required
from the outside. Conversely, if we allow a token program to take the lamports it wants,
a malicious token program could take too much from users.

To get around this, the Token Program interface has a new `CreateHolding`
instruction, which only allocates space and assigns the account to the program.
Once the space is allocated, the Associated Token Program transfers the
required lamports, and returns an error if that number is too large. The process
becomes:

* call `CreateHolding` on the token program (allocate and assign)
* calculate rent requirement based on the data size of holding account
* `SystemInstruction::Transfer` the required lamports to make the holding account rent-exempt
* call `InitializeHolding` on the token program

### Web3 / Wallets

To properly support i-tokens, wallets and applications will need to make the
following changes:

* get holdings: query for holdings owned by the public key, for all official
token programs listed in the registry (not just `Tokenkeg...`)
* get balances: avoid deserializing holding data, and instead use
`getBalance` from RPC
* transfer tokens: only use `ApproveChecked` and `TransferChecked` on the token
program
* create holding: avoid directly creating an account and initializing,
and use the associated token account program instead

### On-chain Programs

To properly support i-tokens on-chain, programs will need to make these changes:

* multiple token programs: all programs that support SPL Tokens must always
include the program id of the token(s) holdings they're passing in.
For example, token-swap needs to accept separate token programs for token A and B
* hard-coded token program: avoid using `Tokenkeg...` directly, and delete `id()`
the SPL token code
* get holding data: instead of deserializing the account data into an SPL holding, use
the new wrapper to directly deserialize or call the `NormalizeToSPLHolding`
instruction with the appropriate token program
* get mint data: same as above, but using the `NormalizeToSPLMint`
* transfer: only use `TransferChecked`, which requires passing mints

## Other New Token Programs

For awhile now, people have been coming up with ideas that likely require a new
token program to implement. These include:

### Voting Tokens

Outlined in a [GitHub issue](https://github.com/solana-labs/solana-program-library/issues/131),
voting tokens use additional data about how long a token has been held.

### Non-Fungible Tokens

It will be possible to add a lot of metadata regarding owners and traders, either
on a new mint or NFT account.

### Allow-list Tokens

Only a certain list keys are allowed (or not) to transfer to tokens, controlled
by the mint.

## Terminology

* token amount: the amount of tokens as seen from the outside. For example, an
interest-bearing token holding with 10 in token amount is converted to a proportional
ownership of 0.5% of the total supply. This amount is always shown in UIs, and
also cached in the `amount` field of 
* share amount: the proportional ownership of a holding in terms of the total
supply of shares. Internally, the interest-bearing token program uses shares to
mint / transfer / burn, and this amount is never shown in UIs.
* spl-token: the base on-chain program for tokens at address `Tokenkeg...` on
[all networks](https://explorer.solana.com/address/TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA)
which also defines the ABI standard required for other token programs that wish
to work "out of the box" with programs and wallets
* i-token: a program implementing the spl-token ABI, all of the instructions mentioned
in the "Token Program Conformance" section

## Proposed Layout

### Mint

```rust
pub struct Mint {
    // SPL Mint Fields
    pub mint_authority: COption<Pubkey>,
    pub decimals: u8,
    pub is_initialized: bool,
    pub freeze_authority: COption<Pubkey>,

    // Interest-bearing Mint Fields
    pub share_supply: Decimal, // The total amount of shares outstanding
    pub initialized_slot: Slot, // When the mint was created, used to discount present value (in terms of token amount) into share amount
}
```

### Holding

```rust
pub struct Holding {
    // SPL Holding Fields
    pub mint: Pubkey,
    pub owner: Pubkey,
    pub delegate: COption<Pubkey>,
    pub state: AccountState,
    pub is_native: COption<u64>,
    pub delegated_amount: u64,
    pub close_authority: COption<Pubkey>,

    // Interest-bearing Holding Fields
    pub share_amount: Decimal, // This holding's proportional share of the total
}
```
