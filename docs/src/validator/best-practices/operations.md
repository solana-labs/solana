---
title: Validator Operations Best Practices
sidebar_label: General Operations
---

After you have successfully gotten a [validator running on testnet](../get-started/setup-a-validator.md) (or another cluster of your choice), you will want to become familiar with how to operate your validator on a day-to-day basis. During daily operations, you will be [monitoring your server](./monitoring.md), updating software regularly (both the Solana validator software and operating system packages), and managing your stake account and identity account.

All of these skills are critical to practice. You do **NOT** want to accidentally cause a prolonged outage of your validator when it is running on mainnet.

## Help with the validator command line

From within the Solana CLI, you can always execute the `solana-validator` command with the `--help` flag to get a better understanding of each command you may need to run.

```
solana-validator --help
```

## Restarting your validator

There are many operational reasons where you may want to restart your validator. As a best practice, you will want to avoid a restart during a leader slot. A [leader slot](../../terminology.md#leader-schedule) is the time when your validator is expected to produce blocks. You do not want to miss out on that time and miss the rewards.

The `solana-validator` command provides the `exit` subcommand that stops your validator when there is a restart window. To execute the command, run:

```
solana-validator exit
```

## Upgrading

There are many ways to upgrade the [Solana software](../../cli/install-solana-cli-tools.md). As an operator, you will need to upgrade often, so it is important to get comfortable with this process.

### Building From Source

It is a best practice to always build your Solana binaries from source. If you build from source, you are certain that the code you are building has not been tampered with before the binary was created. You may also be able to optimize your `solana-validator` binary to your specific hardware.

If you build from source on the validator machine (or a machine with the same CPU), you can target your specific architecture using the `-march` flag. Refer to the Solana docs for [instructions on building from source](../../cli/install-solana-cli-tools.md#build-from-source).

### solana-install

If you are not comfortable building from source, or you need to quickly install a new version to test something out, you could instead try using the `solana-install` command.

Assuming you want to install Solana version `1.10.13`, you would execute the following:

```
solana-install init 1.10.13
```

This command downloads the executable for `1.10.13` and installs it into a `.local` directory. You can also look at `solana-install --help` for more options.

## Snapshots

Startup time for your validator is important because you want to minimize downtime as much as possible. If your validator is offline for a short period of time, and you have a recent snapshot of the ledger on your local hard drive, you can avoid some startup time by skipping the snapshot fetching that the validator will do by default.

In your startup script, add the following flag to the `solana-validator` command:

```
--no-snapshot-fetch
```

If you use this flag with the `solana-validator` command, make sure that you run `solana catchup <pubkey>` after your validator starts to make sure that the validator is catching up in a reasonable time. If the snapshot that you are using locally is too old, it may be faster to use a snapshot from another validator. Be aware that using a snapshot instead of catching up will likely result in missing blocks in your local copy of the ledger. Because of these trade-offs, you may have to experiment with this flag to figure out what works best for you.

### Downloading Snapshots

If you are starting a validator for the first time, or your validator has fallen too far behind after a restart, then you may have to download a snapshot.

To download a snapshot, you must **_NOT_** use the `--no-snapshot-fetch` flag. Without the flag, your validator will automatically download a snapshot from your known validators that you specified with the `--known-validator` flag.

If one of the known validators is downloading slowly, you can try adding the `--minimal-snapshot-download-speed` flag to your validator. This flag will switch to another known validator if the initial download speed is below the threshold that you set.

### Manually Downloading Snapshots

In the case that there are network troubles with one or more of your known validators, then you may have to manually download the snapshot. To manually download a snapshot from one of your known validators, first, find the IP address of the validator in using the `solana gossip` command. In the example below, `5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on` is the pubkey of one of my known validators:

```
solana gossip | grep 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
```

The IP address of the validators is `139.178.68.207` and the open port on this validator is `80`. You can see the IP address and port in the fifth column in the gossip output:

```
139.178.68.207  | 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on | 8001   | 8004  | 139.178.68.207:80     | 1.10.27 | 1425680972
```

Now that the IP and port are known, you can download a full snapshot or an incremental snapshot:

```
wget --trust-server-names http://139.178.68.207:80/snapshot.tar.bz2
wget --trust-server-names http://139.178.68.207:80/incremental-snapshot.tar.bz2
```

Now move those files into your snapshot directory. If you have not specified a snapshot directory, then you should put the files in your ledger directory.

Once you have a local snapshot, you can restart your validator with the `--no-snapshot-fetch` flag.

## Regularly Check Account Balances

It is important that you do not accidentally run out of funds in your identity account, as your node will stop voting. However, this account is somewhat vulnerable because the keypair for the account must be stored on your validator to run the `solana-validator` software. How much SOL you should store there is up to you. As a best practice, make sure to check the account regularly and refill it as needed. To check the account balance do:

```
solana balance validator-keypair.json
```

## Regularly Withdraw From Vote Account

As a reminder, your withdrawer's keypair should **_NEVER_** be stored on your server. It should be stored on a hardware wallet or another secure location that mitigates hacking and theft of funds.

To withdraw your funds from your vote account, you will need to run `solana withdraw-from-vote-account` on a separate computer. To get more information on the command, use `solana withdraw-from-vote-account --help`. For a more detailed explanation of the different keypairs and other related operations refer to the [vote account management page](../../running-validator/vote-accounts.md) of the Solana docs.
