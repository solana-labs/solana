## Stake Account Structure
This page describes the details and permissions associated with a stake account.

#### Account Address
Each stake account has a unique address which can be used to look up the account
information in the command line or in any network explorer tools.  However,
unlike a wallet address in which the holder of the address's keypair controls
the wallet, the keypair associated with a stake account address does not have
any control over the account.  In fact, a keypair or private key may not even
exist for a stake account's address.

The only time a stake account's address has a keypair file is when [creating
a stake account using the command line tools](../cli/staking-operations.md#create-a-stake-account),
a new keypair file is created first only to ensure that the stake account's
address is new and unique.

#### Understanding Account Authorities
Each stake account has two signing authorities specified by their respective address,
each of which is authorized to perform certain operations on the stake account.

The *stake authority* is used to sign transactions for the following operations:
 - Delegating stake
 - Deactivating the stake delegation
 - Splitting the stake account, creating a new stake account with a portion of the
 funds in the first account
 - Merging two undelegated stake accounts into one
 - Setting a new stake authority

The *withdraw authority* signs transactions for the following:
 - Withdrawing un-delegated stake into a wallet address
 - Setting a new withdraw authority
 - Setting a new stake authority

The stake authority and withdraw authority are set when the stake account is
created, and they can be changed to authorize a new signing address at any time.
The stake and withdraw authority can be the same address or two different
addresses.

The withdraw authority keypair holds more control over the account as it is
needed to liquidate the tokens in the stake account, and can be used to reset
the stake authority if the stake authority keypair becomes lost or compromised.

Securing the withdraw authority against loss or theft is of utmost importance
when managing a stake account.

#### Multiple Delegations
A stake account may only ever be delegated to one validator at a time. All of
the tokens in the account are either delegated or un-delegated, or in the
process of becoming delegated or un-delegated.  To delegate a fraction of your
tokens to a validator, or to delegate to multiple validators, you must create
multiple stake accounts.

This can be accomplished by creating multiple stake accounts from a wallet
address containing some tokens, or by creating a single large stake account
and using the stake authority to split the account into multiple accounts
with token balances of your choosing.

The same stake and withdraw authorities can be assigned to multiple
stake accounts.

Two stake accounts that are not delegated and that have the same stake and
withdraw authority can be merged into a single resulting stake account.

#### Delegation Warmup and Cooldown
When a stake account is delegated, or a delegation is deactivated, the operation
does not take effect immediately.

A delegation or deactivation takes several epochs to complete, with a fraction
of the delegation becoming active or inactive at each epoch boundary after
the transaction containing the instructions has been submitted to the cluster.

There is also a limit on how much total stake can become delegated or
deactivated in a single epoch, to prevent large sudden changes in stake across
the network as a whole.

#### Lockups
Stake accounts can have a lockup which prevents the withdraw authority on that
account from withdrawing the tokens before a particular date or epoch has been
reached.  While locked up, the stake account can still be delegated, un-delegated,
or split, and its stake and withdraw authorities can be changed as normal.  Only
withdrawal into a wallet address is not allowed.

A lockup can only be added when a stake account is first created, but it can be
modified later, by the *lockup authority* or *custodian*, the address of which
is set when the account is created.

#### Destroying a Stake Account
Like other types of accounts on the Solana network, a stake account that has a
balance of 0 SOL is no longer tracked.  If a stake account is not delegated
and all of the tokens it contains are withdrawn to a wallet address, the account
at that address is effectively destroyed, and will need to be manually
re-created for the address to be used again.

#### Viewing Stake Accounts
Stake account details can be viewed on the Solana Explorer by copying and pasting
an account address into the search bar.
 - http://explorer.solana.com/accounts
