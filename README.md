[![Solana crate](https://img.shields.io/crates/v/solana-core.svg)](https://crates.io/crates/solana-core)
[![Solana documentation](https://docs.rs/solana-core/badge.svg)](https://docs.rs/solana-core)
[![Build status](https://badge.buildkite.com/8cc350de251d61483db98bdfc895b9ea0ac8ffa4a32ee850ed.svg?branch=master)](https://buildkite.com/solana-labs/solana/builds?branch=master)
[![codecov](https://codecov.io/gh/solana-labs/solana/branch/master/graph/badge.svg)](https://codecov.io/gh/solana-labs/solana)

Blockchain Rebuilt for Scale
===

Solana&trade; is a new blockchain architecture built from the ground up for scale. The architecture supports
up to 710 thousand transactions per second on a gigabit network.

Disclaimer
===

All claims, content, designs, algorithms, estimates, roadmaps, specifications, and performance measurements described in this project are done with the author's best effort.  It is up to the reader to check and validate their accuracy and truthfulness.  Furthermore nothing in this project constitutes a solicitation for investment.

Introduction
===

It's possible for a centralized database to process 710,000 transactions per second on a standard gigabit network if the transactions are, on average, no more than 176 bytes. A centralized database can also replicate itself and maintain high availability without significantly compromising that transaction rate using the distributed system technique known as Optimistic Concurrency Control [\[H.T.Kung, J.T.Robinson (1981)\]](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.65.4735). At Solana, we're demonstrating that these same theoretical limits apply just as well to blockchain on an adversarial network. The key ingredient? Finding a way to share time when nodes can't trust one-another. Once nodes can trust time, suddenly ~40 years of distributed systems research becomes applicable to blockchain!

> Perhaps the most striking difference between algorithms obtained by our method and ones based upon timeout is that using timeout produces a traditional distributed algorithm in which the processes operate asynchronously, while our method produces a globally synchronous one in which every process does the same thing at (approximately) the same time. Our method seems to contradict the whole purpose of distributed processing, which is to permit different processes to operate independently and perform different functions. However, if a distributed system is really a single system, then the processes must be synchronized in some way. Conceptually, the easiest way to synchronize processes is to get them all to do the same thing at the same time. Therefore, our method is used to implement a kernel that performs the necessary synchronization--for example, making sure that two different processes do not try to modify a file at the same time. Processes might spend only a small fraction of their time executing the synchronizing kernel; the rest of the time, they can operate independently--e.g., accessing different files. This is an approach we have advocated even when fault-tolerance is not required. The method's basic simplicity makes it easier to understand the precise properties of a system, which is crucial if one is to know just how fault-tolerant the system is. [\[L.Lamport (1984)\]](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.71.1078)

Furthermore, and much to our surprise, it can be implemented using a mechanism that has existed in Bitcoin since day one. The Bitcoin feature is called nLocktime and it can be used to postdate transactions using block height instead of a timestamp. As a Bitcoin client, you'd use block height instead of a timestamp if you don't trust the network. Block height turns out to be an instance of what's being called a Verifiable Delay Function in cryptography circles. It's a cryptographically secure way to say time has passed. In Solana, we use a far more granular verifiable delay function, a SHA 256 hash chain, to checkpoint the ledger and coordinate consensus. With it, we implement Optimistic Concurrency Control and are now well en route towards that theoretical limit of 710,000 transactions per second.

Architecture
===

Before you jump into the code, review the online book [Solana: Blockchain Rebuilt for Scale](https://docs.solana.com/book/).

(The _latest_ development version of the online book is also [available here](https://docs.solana.com/book/v/master/).)

Release Binaries
===
Official release binaries are available at [Github Releases](https://github.com/solana-labs/solana/releases).

Additionally we provide pre-release binaries for the latest code on the edge and
beta channels.  Note that these pre-release binaries may be less stable than an
official release.

### Edge channel
#### Linux (x86_64-unknown-linux-gnu)
* [solana.tar.bz2](http://release.solana.com/edge/solana-release-x86_64-unknown-linux-gnu.tar.bz2)
* [solana-install-init](http://release.solana.com/edge/solana-install-init-x86_64-unknown-linux-gnu) as a stand-alone executable
#### mac OS (x86_64-apple-darwin)
* [solana.tar.bz2](http://release.solana.com/edge/solana-release-x86_64-apple-darwin.tar.bz2)
* [solana-install-init](http://release.solana.com/edge/solana-install-init-x86_64-apple-darwin) as a stand-alone executable
#### Windows (x86_64-pc-windows-msvc)
* [solana.tar.bz2](http://release.solana.com/edge/solana-release-x86_64-pc-windows-msvc.tar.bz2)
* [solana-install-init.exe](http://release.solana.com/edge/solana-install-init-x86_64-pc-windows-msvc.exe) as a stand-alone executable
#### All platforms
* [solana-metrics.tar.bz2](http://release.solana.com.s3.amazonaws.com/edge/solana-metrics.tar.bz2)

### Beta channel
#### Linux (x86_64-unknown-linux-gnu)
* [solana.tar.bz2](http://release.solana.com/beta/solana-release-x86_64-unknown-linux-gnu.tar.bz2)
* [solana-install-init](http://release.solana.com/beta/solana-install-init-x86_64-unknown-linux-gnu) as a stand-alone executable
#### mac OS (x86_64-apple-darwin)
* [solana.tar.bz2](http://release.solana.com/beta/solana-release-x86_64-apple-darwin.tar.bz2)
* [solana-install-init](http://release.solana.com/beta/solana-install-init-x86_64-apple-darwin) as a stand-alone executable
#### Windows (x86_64-pc-windows-msvc)
* [solana.tar.bz2](http://release.solana.com/beta/solana-release-x86_64-pc-windows-msvc.tar.bz2)
* [solana-install-init.exe](http://release.solana.com/beta/solana-install-init-x86_64-pc-windows-msvc.exe) as a stand-alone executable
#### All platforms
* [solana-metrics.tar.bz2](http://release.solana.com.s3.amazonaws.com/beta/solana-metrics.tar.bz2)

Developing
===

Building
---

Install rustc, cargo and rustfmt:

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
$ sudo apt-get install libssl-dev pkg-config zlib1g-dev llvm clang
```

Download the source code:

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana
```

Build

```bash
$ cargo build
```

Then to run a minimal local cluster
```bash
$ ./run.sh
```

Testing
---

Run the test suite:

```bash
$ cargo test
```

Local Testnet
---

Start your own testnet locally, instructions are in the book [Solana: Blockchain Rebuild for Scale: Getting Started](https://docs.solana.com/book/building-from-source).

Remote Testnets
---

* `testnet` - public stable testnet accessible via testnet.solana.com. Runs 24/7


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
