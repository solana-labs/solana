<p align="center">
  <a href="https://solana.com">
    <img alt="Solana" src="https://i.imgur.com/OMnvVEz.png" width="250" />
  </a>
</p>

[![Solana crate](https://img.shields.io/crates/v/solana-core.svg)](https://crates.io/crates/solana-core)
[![Solana documentation](https://docs.rs/solana-core/badge.svg)](https://docs.rs/solana-core)
[![Build status](https://badge.buildkite.com/8cc350de251d61483db98bdfc895b9ea0ac8ffa4a32ee850ed.svg?branch=master)](https://buildkite.com/solana-labs/solana/builds?branch=master)
[![codecov](https://codecov.io/gh/solana-labs/solana/branch/master/graph/badge.svg)](https://codecov.io/gh/solana-labs/solana)

# Developing

##Building

1. **Install rustc, cargo and rustfmt.**

```bash
$ curl https://sh.rustup.rs -sSf | sh
$ source $HOME/.cargo/env
$ rustup component add rustfmt
```

If your rustc version is lower than 1.39.0, please update it:

```bash
$ rustup update
```

On Linux systems you may need to install libssl-dev, pkg-config, zlib1g-dev, etc.  On Ubuntu:

```bash
$ sudo apt-get update
$ sudo apt-get install libssl-dev libudev-dev pkg-config zlib1g-dev llvm clang
```

2. **Download the source code.**

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana
```

3. **Build.**

```bash
$ cargo build
```

Then to run a minimal local cluster
```bash
$ ./run.sh
```

## Testing

Run the test suite:

```bash
$ cargo test
```

**Local Testnet**

Start your own testnet locally, instructions are in the online docs [Solana: Blockchain Rebuild for Scale: Getting Started](https://docs.solana.com/building-from-source).

**Remote Testnets**

* `testnet` - public stable testnet accessible via devnet.solana.com. Runs 24/7


## Deploy process

They are deployed with the `ci/testnet-manager.sh` script through a list of [scheduled
buildkite jobs](https://buildkite.com/solana-labs/testnet-management/settings/schedules).
Each testnet can be manually manipulated from buildkite as well.

## How do I reset the testnet?
Manually trigger the [testnet-management](https://buildkite.com/solana-labs/testnet-management) pipeline
and when prompted select the desired testnet

## How can I scale the tx generation rate?

Increase the TX rate by increasing the number of cores on the client machine which is running
`bench-tps` or run multiple clients. Decrease by lowering cores or using the rayon env
variable `RAYON_NUM_THREADS=<xx>`

## How can I test a change on the testnet?

Currently, a merged PR is the only way to test a change on the testnet.  But you
can run your own testnet using the scripts in the `net/` directory.

## Adjusting the number of clients or validators on the testnet
Edit `ci/testnet-manager.sh`


## Metrics Server Maintenance
Sometimes the dashboard becomes unresponsive. This happens due to glitch in the metrics server.
The current solution is to reset the metrics server. Use the following steps.

1. The server is hosted in a GCP VM instance. Check if the VM instance is down by trying to SSH
 into it from the GCP console. The name of the VM is ```metrics-solana-com```.
2. If the VM is inaccessible, reset it from the GCP console.
3. Once VM is up (or, was already up), the metrics services can be restarted from build automation.
    1. Navigate to https://buildkite.com/solana-labs/metrics-dot-solana-dot-com in your web browser
    2. Click on ```New Build```
    3. This will show a pop up dialog. Click on ```options``` drop down.
    4. Type in ```FORCE_START=true``` in ```Environment Variables``` text box.
    5. Click ```Create Build```
    6. This will restart the metrics services, and the dashboards should be accessible afterwards.

## Debugging Testnet
Testnet may exhibit different symptoms of failures. Primary statistics to check are
1. Rise in Confirmation Time
2. Nodes are not voting
3. Panics, and OOM notifications

Check the following if there are any signs of failure.
1. Did testnet deployment fail?
    1. View buildkite logs for the last deployment: https://buildkite.com/solana-labs/testnet-management
    2. Use the relevant branch
    3. If the deployment failed, look at the build logs. The build artifacts for each remote node is uploaded.
       It's a good first step to triage from these logs.
2. You may have to log into remote node if the deployment succeeded, but something failed during runtime.
    1. Get the private key for the testnet deployment from ```metrics-solana-com``` GCP instance.
    2. SSH into ```metrics-solana-com``` using GCP console and do the following.
    ```bash
    sudo bash
    cd ~buildkite-agent/.ssh
    ls
    ```
    3. Copy the relevant private key to your local machine
    4. Find the public IP address of the AWS instance for the remote node using AWS console
    5. ```ssh -i <private key file> ubuntu@<ip address of remote node>```
    6. The logs are in ```~solana\solana``` folder


Benchmarking
---

First install the nightly build of rustc. `cargo bench` requires use of the
unstable features only available in the nightly build.

```bash
$ rustup install nightly
```

Run the benchmarks:

```bash
$ cargo +nightly bench
```

Release Process
---
The release process for this project is described [here](RELEASE.md).


Code coverage
---

To generate code coverage statistics:

```bash
$ scripts/coverage.sh
$ open target/cov/lcov-local/index.html
```


Why coverage? While most see coverage as a code quality metric, we see it primarily as a developer
productivity metric. When a developer makes a change to the codebase, presumably it's a *solution* to
some problem.  Our unit-test suite is how we encode the set of *problems* the codebase solves. Running
the test suite should indicate that your change didn't *infringe* on anyone else's solutions. Adding a
test *protects* your solution from future changes. Say you don't understand why a line of code exists,
try deleting it and running the unit-tests. The nearest test failure should tell you what problem
was solved by that code. If no test fails, go ahead and submit a Pull Request that asks, "what
problem is solved by this code?" On the other hand, if a test does fail and you can think of a
better way to solve the same problem, a Pull Request with your solution would most certainly be
welcome! Likewise, if rewriting a test can better communicate what code it's protecting, please
send us that patch!

Disclaimer
===

All claims, content, designs, algorithms, estimates, roadmaps, specifications, and performance measurements described in this project are done with the author's best effort.  It is up to the reader to check and validate their accuracy and truthfulness.  Furthermore nothing in this project constitutes a solicitation for investment.
