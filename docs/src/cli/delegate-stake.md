# Earn Staking Rewards

After you have [recieved SOL](transfer-tokens.md), you might consider putting
it to use by delegating stake to a validator node. Solana is a Proof of Stake
blockchain, which means that holding tokens gives you the option to influence
which validator nodes participate in securing the ledger. The more tokens you
hold, the more influence you have. The running assumption of any Proof of Stake
blockchain is that the more tokens you hold, the more you have to lose if a
validator node acts maliciously.

As a tokenholder, it is your option to use your tokens as *stake*. Stake is
what we call tokens when they reside in a *stake account*. From the stake
account, you can delegate that stake to validators. Solana weights validator
votes by the amount of stake delegated to them, which gives those validators
more influence in determining then next valid block of transactions in the
blockchain. Solana then generates new SOL periodically to reward stakers and
validators. You earn more rewards the more stake you delegate.

## Create a Stake Account

To delegate stake, you will need to move your tokens into a stake account.
All commands in this section assume you have
[set a default fee-payer](transfer-tokens.md#set-a-default-fee-payer).

Create a stake account with the `solana create-stake-account` command:

```bash
solana create-stake-account --from=<KEYPAIR> <ACCOUNT_KEYPAIR> <AMOUNT>
```

`<AMOUNT>` tokens are transferred from the account at `<KEYPAIR>` to a new
stake account at the public key of `<ACCOUNT_KEYPAIR>`.  Record the account
public key, as it will be used later to perform actions on the stake account.
The keypair file can be discarded. To authorize additional actions, you will
use `<KEYPAIR>`, not `<ACCOUNT_KEYPAIR>`.

### Advanced: Derive Stake Account Addresses

When we delegate stake, we delegate all tokens to a single validator. To
delegate to multiple validators we'll need multiple stake accounts. Creating
a new keypair for each account and managing those addresses can be cumbersome.
Fortunately, we can derive stake addresses using the `--seed` option:

```bash
solana create-stake-account --from=<KEYPAIR> <KEYPAIR> --seed=<STRING> <AMOUNT>
```

`<STRING>` is arbitrary but typically a number corresponding to which derived
account this is. The first account might be "0", then "1", and so on.
`<KEYPAIR>` is the same for both the source keypair and account keypair,
because the second instance is used only as a base address. The command
derives a new address from the base address and seed string. To see what stake
address the command will derive, use `solana create-address-with-seed`:

```bash
solana create-address-with-seed --from=<PUBKEY> <SEED_STRING> STAKE
```

It will output a derived public key, which can be used for the
`<ACCOUNT_PUBKEY>` argument in staking operations.

## Set Stake and Withdraw Authorities

Staking commands look to keypairs to authorize certain stake account
operations. It uses a stake authority to authorize stake delegation,
deactivating stake, splitting stake, and setting a new stake authority.  It
uses a withdraw authority to authorize withdrawing stake, and setting either
a new stake or withdraw authority.

Stake and withdraw authorities can be set when creating account via the
`--stake-authority` and `--withdraw-authority` options, or afterward with
the `solana stake-authorize` command. For example, to set a new authorized
staker, run:

```bash
solana stake-authorize <ACCOUNT_KEYPAIR> --stake-authority=<KEYPAIR> --new-stake-authority=<PUBKEY>
```

This will use the existing stake authority `<KEYPAIR>` to authorize a new
stake authority `<PUBKEY>` on the stake account `<ACCOUNT_KEYPAIR>`.

## Delegate Stake

To delegate stake, run:

```bash
solana delegate-stake --stake-authority=<KEYPAIR> <ACCOUNT_PUBKEY> <VOTE_PUBKEY>
```

`<KEYPAIR>` authorizes the operation on the account with address
`<ACCOUNT_PUBKEY>`. The stake is delegated to the vote account with
public key `<VOTE_PUBKEY>`.

## Deactivate Stake

Once delegated, you can deactivate stake with the `solana deactivate-stake`
command:

```bash
solana deactivate-stake --stake-authority=<KEYPAIR> <ACCOUNT_PUBKEY>
```

`<KEYPAIR>` authorizes the operation on the account with address
`<ACCOUNT_PUBKEY>`.

Note that stake takes several epochs to "cool down". Attempts to delegate
stake in the cooldown period will fail.

## Split Stake

All stake in a stake account is delegated to a single validator. If you'd
prefer to spread your stake over multiple validators use the
`solana split-stake` command to duplicate stake accounts and split your
tokens between them:

```bash
solana split-stake --stake-authority=<KEYPAIR> <ACCOUNT_PUBKEY> <NEW_ACCOUNT_KEYPAIR> <AMOUNT>
```

`<ACCOUNT_PUBKEY>` is the existing stake account, `<KEYPAIR>` is the
authorized staker, `<NEW_ACCOUNT_KEYPAIR>` is the keypair for the new account,
and `<AMOUNT>` is the number of tokens to transfer to the new account.

## Withdraw Stake

Transfer tokens out of a stake account with the `solana withdraw-stake` command:

```bash
solana withdraw-stake --withdraw-authority=<KEYPAIR> <ACCOUNT_PUBKEY> <RECIPIENT_PUBKEY> <AMOUNT>
```

`<ACCOUNT_PUBKEY>` is the existing stake account, `<KEYPAIR>` is the
authorized staker, and `<AMOUNT>` is the number of tokens to transfer
to `<RECIPIENT_PUBKEY>`.
