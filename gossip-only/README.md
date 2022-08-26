# Setup Solana Gossip Only Run
### With metrics:
```
git clone git@github.com:gregcusack/solana.git
cd solana/
git checkout gossip-sim-gce-influx
```

Run gce.sh script to deploy any number of GCP nodes
```
cd net/
./gce.sh create -p $GOSSIP_INFLUXDB_NAME -n <number-of-nodes> -z us-west1-a -d pd-ssd --dedicated
```

Setup metrics server. This is what we'll be pushing gossip loop metrics to:
```
cd ../metrics
./init-metrics.sh $GOSSIP_INFLUX_USERNAME
# Then enter your password for influx when asked
```

Setup Influx Credentials as environment variables
```
export GOSSIP_INFLUX_USERNAME=<influx-username>
export GOSSIP_INFLUX_PASSWORD=<influx-password>
export GOSSIP_INFLUXDB_NAME=<influx-database-name>
```

Run Gossip Only across the GCP nodes
Option 1: Deploy a specific number of gossip instances per GCP node
```
cd ../net
./net.sh gossip-sim --gossip-instances-per-node <number-of-gossip-instances-per-node>
```

Option 2: Deploy a total number of gossip instances evenly distributed across all GCP nodes. Deployed round-robin like
```
./net.sh gossip-sim --gossip-instances <number-of-gossip-instances>
```

Note: If you deploy a large number of gossip instances like > 200 when setting `--gossip-instances`, sometimes it doesn't deploy all 200. It will deploy like ~185. I tried to figure out why, but couldn't figure it out in a reasinable amount of time. I haven't seen this issue with `--gossip-instances-per-node`

### Without metrics:
```
git checkout gossip-sim-gce
```
Similar to above but no need to run `./init-metrics.sh` or set the environment variables.
Should be good to go now to run `./net.sh <flag> <num-instances>`

### Things to Note:
If you want to decrease pull request frequency to once every X seconds. checkout `gossip-sim-gce-influx`  and go into `solana/gossip/src/cluster_info.rs`,
In the function:
```
pub fn gossip(
    self: Arc<Self>,
    bank_forks: Option<Arc<RwLock<BankForks>>>,
    sender: PacketBatchSender,
    gossip_validators: Option<HashSet<Pubkey>>,
    exit: Arc<AtomicBool>,
) -> JoinHandle<()> {
```
Uncomment the following comment lines:
```
// let mut timer0 = timestamp();
// let mut run_pull_req;

// let timer1 = timestamp();
// if Duration::from_millis(timer1 - timer0).as_secs() > 10 {
//     run_pull_req = true;
//     timer0 = timestamp();
// } else {
//     run_pull_req = false;
// }

// let _ = self.run_gossip(
//     &thread_pool,
//     gossip_validators.as_ref(),
//     &recycler,
//     &stakes,
//     &sender,
//     run_pull_req,
// );
```

And comment these lines:
```
let _ = self.run_gossip(
    &thread_pool,
    gossip_validators.as_ref(),
    &recycler,
    &stakes,
    &sender,
    generate_pull_requests,
);
```
Same issue as above, I wanted to set this as a flag but I never got to it.

## Running Gossip-Only Locally
```
cd solana
```
Depending if you want metrics or not, checkout either `gossip-sim-gce-influx` or `gossip-sim-gce`
If you want metrics, you still have to set the environment variables (see above)!

Create N accounts files with default stakes
```
cargo run --bin gossip-sim -- --write-keys --num-keys N --account-file accounts.yaml
```

Run bootstrap gossip instance:
```
cargo run --bin gossip-sim -- --bootstrap --account-file ./accounts.yaml --num-nodes 1 --entrypoint $(hostname -i):<entry-port> --shred-version <shred-version> --gossip-host $(hostname -i) --gossip-port <entry-port>
```

Run N number of gossip instances to connect to the bootstrap instance
```
cargo run --bin gossip-sim -- --account-file ./accounts.yaml --num-nodes <N-1> --entrypoint $(hostname -i):<entry-port> --shred-version <shred-version> --gossip-host $(hostname -i)
```

## O

## Write accounts with stake to config yaml file.
#### Note: Currently stake is set to default node stake: 10000000000

```
cargo run --bin gossip-sim -- --account-file <output-yaml-file> --write-keys --num-keys <number-of-keys>
```

Example:
Generate 100 keys and stakes and write to `accounts.yaml` file
```
cargo run --bin gossip-sim -- --account-file accounts.yaml --write-keys --num-keys 100
```
