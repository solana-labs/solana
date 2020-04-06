# Connecting to a Cluster

Solana maintains several different clusters with different purposes.  Each
cluster features a Solana-run gossip node that serves as an entrypoint to the
cluster.

## Before you Begin

 - Make sure you have first
[installed the Solana command line tools](install-solana-cli-tools.md)

## Choose a Cluster

#### Mainnet Beta
A permissionless, persistent cluster for early token holders and launch partners.
No smart contracts or inflation.
 * Tokens that are issued on Mainnet Beta are **real** SOL
 * If you have paid money to purchase/be issued tokens, such as through our
 CoinList auction, these tokens will be transferred on Mainnet Beta.
   * Note: If you are using a non-command-line wallet such as
   [Trust Wallet](../wallet/trust-wallet.md),
   the wallet will always be connecting to Mainnet Beta.
 * Gossip entrypoint for Mainnet Beta: `mainnet-beta.solana.com:8001`
 * RPC URL for Mainnet Beta: `https://api.mainnet-beta.solana.com`

#### Devnet
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

#### Testnet (Tour de SOL Cluster)
* Testnet is where we stress test recent release features on a live
cluster, particularly focused on network performance, stability and validator
behavior.
* [Tour de SOL](../tour-de-sol/README.md) initiative runs on Testnet, where we
encourage malicious behavior and attacks on the network to help us find and
squash bugs or network vulnerabilities.
* Testnet tokens are **not real**
* Testnet may be subject to ledger resets.
* Testnet typically runs a newer software release than both Devnet and
Mainnet Beta
* Testnet may be maintained by different validators than Mainnet Beta
* Gossip entrypoint for Testnet: `testnet.solana.com:8001`
* RPC URL for Testnet: `https://testnet.solana.com`

## Configure the Command-line

You can check what cluster the Solana CLI is currently targeting by
running the following command:

```bash
solana config get
```

Use `solana config set` command to target a particular cluster.  After setting
a cluster target, any future subcommands will send/receive information from that
cluster.

##### Targetting Mainnet Beta
```bash
solana config set --url https://api.mainnet-beta.solana.com
```

##### Targetting Devnet
```bash
solana config set --url https://devnet.solana.com
```

##### Targetting Testnet
```bash
solana config set --url https://testnet.solana.com
```

## Ensure Versions Match

Though not strictly necessary, the CLI will generally work best when its version
matches the software version running on the cluster. To get the locally-installed
CLI version, run:

```bash
solana --version
```

To get the cluster version, run:

```bash
solana cluster-version
```

Ensure the local CLI version is greater than or equal to the cluster version.
