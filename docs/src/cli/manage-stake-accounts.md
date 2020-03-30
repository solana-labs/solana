# Manage Stake Accounts

If you want to delegate stake to many different validators, you will need
to create a separate stake account for each. If you follow the convention
of creating the first stake account at seed "0", the second at "1", the
third at "2", and so on, then the `solana-stake-accounts` tool will allow
you to operate on all accounts with single invocations. You can use it to
sum up the balances of all accounts, move accounts to a new wallet, or set
new authorities.

## Usage

### Create a stake account

Create and fund a derived stake account at the stake authority public key:

```bash
solana-stake-accounts new <FUNDING_KEYPAIR> <BASE_KEYPAIR> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY>
```

### Count accounts

Count the number of derived accounts:

```bash
solana-stake-accounts count <BASE_PUBKEY>
```

### Get stake account balances

Sum the balance of derived stake accounts:

```bash
solana-stake-accounts balance <BASE_PUBKEY> --num-accounts <NUMBER>
```

### Get stake account addresses

List the address of each stake account derived from the given public key:

```bash
solana-stake-accounts addresses <BASE_PUBKEY> --num-accounts <NUMBER>
```

### Set new authorities

Set new authorities on each derived stake account:

```bash
solana-stake-accounts authorize <BASE_PUBKEY> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER>
```

### Relocate stake accounts

Relocate stake accounts:

```bash
solana-stake-accounts rebase <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --num-accounts <NUMBER>
```

To atomically rebase and authorize each stake account, use the 'move'
command:

```bash
solana-stake-accounts move <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER>
```
