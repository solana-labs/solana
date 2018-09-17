# TestNet debugging info

Currently we have three testnets:
* `testnet` - public beta channel testnet accessible via testnet.solana.com. Runs 24/7
* `testnet-perf` - private beta channel testnet with clients trying to flood the network
with transactions until failure.  Runs 24/7
* `testnet-master` - private edge channel testnet with clients trying to flood the network
with transactions until failure.  Runs on weekday mornings for a couple hours

## Deploy process

They are deployed with the `ci/testnet-deploy.sh` script. There is a scheduled buildkite job which runs to do the deploy,
look at `testnet-deploy` to see the agent which ran it and the logs. There is also a manual job to do the deploy manually..
Validators are selected based on their machine name and everyone gets the binaries installed from snap.

## Where are the testnet logs?

Attach to the testnet first by running one of:
```bash
$ net/gce.sh config testnet-solana-com
$ net/gce.sh config master-testnet-solana-com
$ net/gce.sh config perf-testnet-solana-com
```

Then run:
```bash
$ net/ssh.sh
```
for log location details

## How do I reset the testnet?
Manually trigger the [testnet-deploy](https://buildkite.com/solana-labs/testnet-deploy/) pipeline

## How can I scale the tx generation rate?

Increase the TX rate by increasing the number of cores on the client machine which is running
`bench-tps` or run multiple clients. Decrease by lowering cores or using the rayon env
variable `RAYON_NUM_THREADS=<xx>`

## How can I test a change on the testnet?

Currently, a merged PR is the only way to test a change on the testnet.  But you
can run your own testnet using the scripts in the `net/` directory.

## Adjusting the number of clients or validators on the testnet
Through the [testnet-deploy](https://buildkite.com/solana-labs/testnet-deploy/) settings.

