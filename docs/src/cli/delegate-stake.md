# Earn Staking Rewards

After you have [received SOL](transfer-tokens.md), you might consider putting
it to use by delegating stake to a validator. Solana is a Proof of Stake
blockchain, which means that holding tokens gives you the option to influence
which validators participate in securing the ledger. The more tokens you hold,
the more influence you have. The running assumption of any Proof of Stake
blockchain is that the more tokens you hold, the more you have to lose if a
validator acts maliciously.

As a tokenholder, it is your option to use your tokens as *stake*. Stake is
what we call tokens when they reside in a *stake account*. From the stake
account, you can delegate that stake to validators. Solana weights validator
votes by the amount of stake delegated to them, which gives those validators
more influence in determining then next valid block of transactions in the
blockchain. Solana then generates new SOL periodically to reward stakers and
validators. You earn more rewards the more stake you delegate.

## Create a Stake Account

To delegate stake, you will need to transfer some tokens into a stake account.
Create a stake account with the `solana create-stake-account` command:

```bash
solana-keygen new --no-passphrase -o stake-account.json
solana create-stake-account --from=<KEYPAIR> stake-account.json <AMOUNT> --stake-authority=<KEYPAIR> --withdraw-authority=<KEYPAIR>
```

`<AMOUNT>` tokens are transferred from the account at `<KEYPAIR>` to a new
stake account at the public key of stake-account.json.  Record the account
public key, as it will be used later to perform actions on the stake account.
The stake-account.json file can then be discarded. To authorize additional
actions, you will use `<KEYPAIR>`, not the keypair at stake-account.json.

### Set Stake and Withdraw Authorities

Staking commands look to keypairs to authorize certain stake account
operations. They use the stake authority to authorize stake delegation,
deactivating stake, splitting stake, and setting a new stake authority.  They
use the withdraw authority to authorize withdrawing stake, and when setting
either a new stake or withdraw authority.

Stake and withdraw authorities can be set when creating account via the
`--stake-authority` and `--withdraw-authority` options, or afterward with the
`solana stake-authorize` command. For example, to set a new stake authority,
run:

```bash
solana stake-authorize <STAKE_ACCOUNT_ADDRESS> --stake-authority=<KEYPAIR> --new-stake-authority=<PUBKEY>
```

This will use the existing stake authority `<KEYPAIR>` to authorize a new stake
authority `<PUBKEY>` on the stake account `<STAKE_ACCOUNT_ADDRESS>`.

### Advanced: Derive Stake Account Addresses

When you delegate stake, you delegate all tokens in the stake account to a
single validator. To delegate to multiple validators, you will need multiple
stake accounts. Creating a new keypair for each account and managing those
addresses can be cumbersome. Fortunately, you can derive stake addresses using
the `--seed` option:

```bash
solana create-stake-account --from=<KEYPAIR> <STAKE_ACCOUNT_KEYPAIR> --seed=<STRING> <AMOUNT> --stake-authority=<PUBKEY> --withdraw-authority=<PUBKEY>
```

`<STRING>` is an arbitrary string up to 32 bytes, but will typically be a
number corresponding to which derived account this is. The first account might
be "0", then "1", and so on. The keypair used for `<STAKE_ACCOUNT_KEYPAIR>`
acts as base key. The command derives a new address from the base address and
seed string. To see what stake address the command will derive, use `solana
create-address-with-seed`:

```bash
solana create-address-with-seed --from=<PUBKEY> <SEED_STRING> STAKE
```

`<PUBKEY>` is the public key of the `<STAKE_ACCOUNT_KEYPAIR>` passed to
`solana create-stake-account`.

The command will output a derived address, which can be used for the
`<STAKE_ACCOUNT_ADDRESS>` argument in staking operations.

## Delegate Stake

To delegate your stake to a validator, run:

```bash
solana delegate-stake --stake-authority=<KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <VOTE_ACCOUNT_ADDRESS>
```

`<KEYPAIR>` authorizes the operation on the account with address
`<STAKE_ACCOUNT_ADDRESS>`. The stake is delegated to the vote account with
address `<VOTE_ACCOUNT_ADDRESS>`.

## Deactivate Stake

Once delegated, you can undelegate stake with the `solana deactivate-stake`
command:

```bash
solana deactivate-stake --stake-authority=<KEYPAIR> <STAKE_ACCOUNT_ADDRESS>
```

`<KEYPAIR>` authorizes the operation on the account with address
`<STAKE_ACCOUNT_ADDRESS>`.

Note that stake takes several epochs to "cool down". Attempts to delegate stake
in the cool down period will fail.

## Withdraw Stake

Transfer tokens out of a stake account with the `solana withdraw-stake` command:

```bash
solana withdraw-stake --withdraw-authority=<KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <RECIPIENT_ADDRESS> <AMOUNT>
```

`<STAKE_ACCOUNT_ADDRESS>` is the existing stake account, `<KEYPAIR>` is the
withdraw authority, and `<AMOUNT>` is the number of tokens to transfer to
`<RECIPIENT_ADDRESS>`.

## Split Stake

You may want to delegate stake to additional validators while your existing
stake is not eligible for withdrawal. It might not be eligible because it is
currently staked, cooling down, or locked up. To transfer tokens from an
existing stake account to a new one, use the `solana split-stake` command:

```bash
solana split-stake --stake-authority=<KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <NEW_STAKE_ACCOUNT_KEYPAIR> <AMOUNT>
```

`<STAKE_ACCOUNT_ADDRESS>` is the existing stake account, `<KEYPAIR>` is the
stake authority, `<NEW_STAKE_ACCOUNT_KEYPAIR>` is the keypair for the new account,
and `<AMOUNT>` is the number of tokens to transfer to the new account.

To split a stake account into a derived account address, use the `--seed`
option in the same way as with `solana create-stake-account`.
