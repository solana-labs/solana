# Create and configure a Solana validator

The validator identity keypair [you created](validator-public-key-registration.md) identifies your validator. Your validator public key, which you registered for the TdS, will receive an allotment of lamports in the genesis block that can be used to start your validator node. _Note that airdrops have been disabled so the `solana airdrop` command will fail._

## Configure Solana

Configure solana for your validator identity and Tour de SOL:

```bash
solana config set --url http://tds.solana.com --keypair ~/validator-keypair.json
```

You should see the following output:

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://tds.solana.com
* keypair: /home/solana/validator-keypair.json
```

You can see the wallet configuration at any time by running:

```text
solana config get
```

## Check Validator Balance

To view your current balance:

```text
solana balance
```

Or to see in finer detail:

```text
solana balance --lamports
```

If your validator identity keypair is created and the above command shows a non-zero balance, your validator is created and configured for TdS.

Your starting balance will be 500 SOL. Read more about the [difference between SOL and lamports here](../../../introduction.md#what-are-sols).
