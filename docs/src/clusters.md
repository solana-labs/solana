# Solana Clusters
Solana maintains several different clusters with different purposes.

Before you begin make sure you have first
[installed the Solana command line tools](cli/install-solana-cli-tools.md)

Explorers:
* [http://explorer.solana.com/](https://explorer.solana.com/).
* [http://solanabeach.io/](http://solanabeach.io/).

## Devnet
* Devnet serves as a playground for anyone who wants to take Solana for a
test drive, as a user, token holder, app developer, or validator.
* Application developers should target Devnet.
* Potential validators should first target Devnet.
* Key differences between Devnet and Mainnet Beta:
  * Devnet tokens are **not real**
  * Devnet includes a token faucet for airdrops for application testing
  * Devnet may be subject to ledger resets
  * Devnet typically runs a newer software version than Mainnet Beta
  * Devnet may be maintained by different validators than Mainnet Beta
 * Gossip entrypoint for Devnet: `devnet.solana.com:8001`
 * RPC URL for Devnet: `https://devnet.solana.com`


##### Example `solana` command-line configuration
```bash
solana config set --url https://devnet.solana.com
```

##### Example `solana-validator` command-line
```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint devnet.solana.com:8001 \
    --expected-genesis-hash HzyuivuNXMHJKjM6q6BE2qBsR3etqW21BSvuJTpJFj9A \
    --expected-shred-version 61357 \
    --limit-ledger-size
```


## Testnet
* Testnet is where we stress test recent release features on a live
cluster, particularly focused on network performance, stability and validator
behavior.
* [Tour de SOL](tour-de-sol/README.md) initiative runs on Testnet, where we
encourage malicious behavior and attacks on the network to help us find and
squash bugs or network vulnerabilities.
* Testnet tokens are **not real**
* Testnet may be subject to ledger resets.
* Testnet typically runs a newer software release than both Devnet and
Mainnet Beta
* Testnet may be maintained by different validators than Mainnet Beta
* Gossip entrypoint for Testnet: `35.203.170.30:8001`
* RPC URL for Testnet: `https://testnet.solana.com`


##### Example `solana` command-line configuration
```bash
solana config set --url https://testnet.solana.com
```

##### Example `solana-validator` command-line
```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint 35.203.170.30:8001 \
    --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --expected-shred-version 28769 \
    --limit-ledger-size
```

## Mainnet Beta
A permissionless, persistent cluster for early token holders and launch partners.
Currently smart contracts, rewards, and inflation are disabled.
 * Tokens that are issued on Mainnet Beta are **real** SOL
 * If you have paid money to purchase/be issued tokens, such as through our
 CoinList auction, these tokens will be transferred on Mainnet Beta.
   * Note: If you are using a non-command-line wallet such as
   [Trust Wallet](wallet-guide/trust-wallet.md),
   the wallet will always be connecting to Mainnet Beta.
 * Gossip entrypoint for Mainnet Beta: `mainnet-beta.solana.com:8001`
 * RPC URL for Mainnet Beta: `https://api.mainnet-beta.solana.com`

##### Example `solana` command-line configuration
```bash
solana config set --url https://api.mainnet-beta.solana.com
```

##### Example `solana-validator` command-line
```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 \
    --trusted-validator GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ \
    --trusted-validator DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ \
    --trusted-validator CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint mainnet-beta.solana.com:8001 \
    --expected-genesis-hash 5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d \
    --expected-shred-version 54208 \
    --limit-ledger-size
```
