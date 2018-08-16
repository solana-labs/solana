# TestNet debugging info

Currently we have two testnets, 'perf' and 'master', both on the master branch of the solana repo. Deploys happen
at the top of every hour with the latest code. 'perf' has more cores for the client machine to flood the network
with transactions until failure.

## Deploy process

They are deployed with the `ci/testnet-deploy.sh` script. There is a scheduled buildkite job which runs to do the deploy,
look at `testnet-deploy` to see the agent which ran it and the logs. There is also a manual job to do the deploy manually..
Validators are selected based on their machine name and everyone gets the binaries installed from snap.

## Where are the testnet logs?

For the client they are put in `/tmp/solana`; for validators and leaders they are in `/var/snap/solana/current/`.
You can also see the backtrace of the client by ssh'ing into the client node and doing:

```bash
$ sudo -u testnet-deploy
$ tmux attach -t solana
```

## How do I reset the testnet?

Through buildkite.

## How can I scale the tx generation rate?

Increase the TX rate by increasing the number of cores on the client machine which is running
`bench-tps` or run multiple clients. Decrease by lowering cores or using the rayon env
variable `RAYON_NUM_THREADS=<xx>`

## How can I test a change on the testnet?

Currently, a merged PR is the only way to test a change on the testnet.
