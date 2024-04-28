<p align="center">
  <a href="https://solana.com">
    <img alt="Solana" src="https://i.imgur.com/IKyzQ6T.png" width="250" />
  </a>
</p>
**UPDATING CONTRACT.SOL FOR ME PERSONALLY TO GETTING THE GAIN OF ASSETS ASSESSMENT**
#Contract.Sol
pragma solidity ^0.5.0;

contract AssetTransfer {

    enum StateType {
        Active,
        OfferPlaced,
        PendingInspection,
        Inspected,
        Appraised,
        NotionalAcceptance,
        BuyerAccepted,
        SellerAccepted,
        Accepted,
        Terminated
    }

    event ContractCreated(string applicationName, string workflowName, address originatingAddress);
    event ContractUpdated(string applicationName, string workflowName, string action, address originatingAddress);

    string internal ApplicationName = "AssetTransfer";
    string internal WorkflowName = "AssetTransfer";

    address public InstanceOwner;
    string public Description;
    uint public AskingPrice;
    StateType public State;

    address public InstanceBuyer;
    uint public OfferPrice;
    address public InstanceInspector;
    address public InstanceAppraiser;

    constructor (string memory description, uint256 price) public {
        InstanceOwner = msg.sender;
        AskingPrice = price;
        Description = description;
        State = StateType.Active;

        emit ContractCreated(ApplicationName, WorkflowName, msg.sender);
    }

    function Terminate() public {
        if (InstanceOwner != msg.sender) {
            revert("The contract can only be terminated by the owner");
        }

        State = StateType.Terminated;

        emit ContractUpdated(ApplicationName, WorkflowName, "Terminate", msg.sender);
    }

    function Modify(string calldata description, uint256 price) external {
        if (State != StateType.Active) {
            revert("Modify function can only be called when in Active state");
        }

        if (InstanceOwner != msg.sender) {
            revert("Modify function can only be called by the owner");
        }

        Description = description;
        AskingPrice = price;

        emit ContractUpdated(ApplicationName, WorkflowName, "Modify", msg.sender);
    }

    function MakeOffer(address inspector, address appraiser, uint256 offerPrice) external {
        if (inspector == address(0x000) || appraiser == address(0x000)) {
            revert("MakeOffer function need to have a valid inspector/appraiser address");
        }

        if (offerPrice == 0) {
            revert("MakeOffer function need to have an offerPrice > 0");
        }

        if (State != StateType.Active) {
            revert("MakeOffer function can only be called when in Active state");
        }

        if (InstanceOwner == msg.sender) {
            revert("MakeOffer function cannot be called by the owner");
        }

        InstanceBuyer = msg.sender;
        InstanceInspector = inspector;
        InstanceAppraiser = appraiser;
        OfferPrice = offerPrice;
        State = StateType.OfferPlaced;

        emit ContractUpdated(ApplicationName, WorkflowName, "MakeOffer", msg.sender);
    }

    function AcceptOffer() external {
        if (State != StateType.OfferPlaced) {
            revert("AcceptOffer function can only be called when an offer placed.");
        }

        if (InstanceOwner != msg.sender) {
            revert("AcceptOffer function can only be called by the owner");
        }

        State = StateType.PendingInspection;

        emit ContractUpdated(ApplicationName, WorkflowName, "AcceptOffer", msg.sender);
    }

    function Reject() external {
        if (State != StateType.OfferPlaced && State != StateType.PendingInspection &&
            State != StateType.Inspected && State != StateType.Appraised &&
            State != StateType.NotionalAcceptance && State != StateType.BuyerAccepted) {
            revert("Current state does not allow the Reject function to be called");
        }

        if (InstanceOwner != msg.sender) {
            revert("Reject function can only be called by the owner");
        }

        InstanceBuyer = address(0x000);
        State = StateType.Active;

        emit ContractUpdated(ApplicationName, WorkflowName, "Reject", msg.sender);
    }

    function Accept() external {
        if (msg.sender != InstanceBuyer && msg.sender != InstanceOwner) {
            revert("Accept function can only be called by the Buyer or the Owner");
        }

        if (msg.sender == InstanceOwner && State != StateType.NotionalAcceptance && State != StateType.BuyerAccepted) {
            revert("Accept function can only be called by the Owner and no acceptance");
        }

        if (msg.sender == InstanceBuyer && State != StateType.NotionalAcceptance && State != StateType.SellerAccepted) {
            revert("Accept function can only be called by Buyer and no acceptance");
        }

        if (msg.sender == InstanceBuyer) {
            if (State == StateType.NotionalAcceptance) {
                State = StateType.BuyerAccepted;
            }
            else if (State == StateType.SellerAccepted) {
                State = StateType.Accepted;
            }
        } else {
            if (State == StateType.NotionalAcceptance) {
                State = StateType.SellerAccepted;
            } else if (State == StateType.BuyerAccepted) {
                State = StateType.Accepted;
            }
        }

        emit ContractUpdated(ApplicationName, WorkflowName, "Accept", msg.sender);
    }

    function ModifyOffer(uint256 offerPrice) external {
        if (State != StateType.OfferPlaced) {
            revert("ModifyOffer function cannot be called if an offer has been placed.");
        }

        if (InstanceBuyer != msg.sender || offerPrice == 0) {
            revert("ModifyOffer can only be called by Buyer with an offerPrice > 0");
        }

        OfferPrice = offerPrice;

        emit ContractUpdated(ApplicationName, WorkflowName, "ModifyOffer", msg.sender);
    }

    function RescindOffer() external {
        if (State != StateType.OfferPlaced && State != StateType.PendingInspection &&
            State != StateType.Inspected && State != StateType.Appraised &&
            State != StateType.NotionalAcceptance && State != StateType.SellerAccepted) {
            revert("RescindOffer function criteria was not met");
        }

        if (InstanceBuyer != msg.sender) {
            revert("RescindOffer function can only be called by the Buyer");
        }

        InstanceBuyer = address(0x000);
        OfferPrice = 0;
        State = StateType.Active;

        emit ContractUpdated(ApplicationName, WorkflowName, "RescindOffer", msg.sender);
    }

    function MarkAppraised() external {
        if (InstanceAppraiser != msg.sender) {
            revert("MarkAppraised function can only be called by the Appraiser");
        }

        if (State == StateType.PendingInspection) {
            State = StateType.Appraised;
        } else if (State == StateType.Inspected) {
            State = StateType.NotionalAcceptance;
        } else {
            revert("MarkAppraised function was not PendingInspection or Inspection");
        }

        emit ContractUpdated(ApplicationName, WorkflowName, "MarkAppraised", msg.sender);
    }

    function MarkInspected() external {
        if (InstanceInspector != msg.sender) {
            revert("MarkInspected function cannot be called by the Inspector");
        }

        if (State == StateType.PendingInspection) {
            State = StateType.Inspected;
        } else if (State == StateType.Appraised) {
            State = StateType.NotionalAcceptance;
        } else {
            revert("MarkInspected function can only be called if PendingInspected or Appraised");
        }

        emit ContractUpdated(ApplicationName, WorkflowName, "MarkInspected", msg.sender);
    }
}
[![Solana crate](https://img.shields.io/crates/v/solana-core.svg)](https://crates.io/crates/solana-core)
[![Solana documentation](https://docs.rs/solana-core/badge.svg)](https://docs.rs/solana-core)
[![Build status](https://badge.buildkite.com/8cc350de251d61483db98bdfc895b9ea0ac8ffa4a32ee850ed.svg?branch=master)](https://buildkite.com/solana-labs/solana/builds?branch=master)
[![codecov](https://codecov.io/gh/solana-labs/solana/branch/master/graph/badge.svg)](https://codecov.io/gh/solana-labs/solana)

# Building

## **1. Install rustc, cargo and rustfmt.**

```bash
$ curl https://sh.rustup.rs -sSf | sh
$ source $HOME/.cargo/env
$ rustup component add rustfmt
```

When building the master branch, please make sure you are using the latest stable rust version by running:

```bash
$ rustup update
```

When building a specific release branch, you should check the rust version in `ci/rust-version.sh` and if necessary, install that version by running:
```bash
$ rustup install VERSION
```
Note that if this is not the latest rust version on your machine, cargo commands may require an [override](https://rust-lang.github.io/rustup/overrides.html) in order to use the correct version.

On Linux systems you may need to install libssl-dev, pkg-config, zlib1g-dev, protobuf etc.

On Ubuntu:
```bash
$ sudo apt-get update
$ sudo apt-get install libssl-dev libudev-dev pkg-config zlib1g-dev llvm clang cmake make libprotobuf-dev protobuf-compiler
```

On Fedora:
```bash
$ sudo dnf install openssl-devel systemd-devel pkg-config zlib-devel llvm clang cmake make protobuf-devel protobuf-compiler perl-core
```

## **2. Download the source code.**

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana
```

## **3. Build.**

```bash
$ ./cargo build
```

# Testing

**Run the test suite:**

```bash
$ ./cargo test
```

### Starting a local testnet

Start your own testnet locally, instructions are in the [online docs](https://docs.solanalabs.com/clusters/benchmark).

### Accessing the remote development cluster

* `devnet` - stable public cluster for development accessible via
devnet.solana.com. Runs 24/7. Learn more about the [public clusters](https://docs.solanalabs.com/clusters)

# Benchmarking

First, install the nightly build of rustc. `cargo bench` requires the use of the
unstable features only available in the nightly build.

```bash
$ rustup install nightly
```

Run the benchmarks:

```bash
$ cargo +nightly bench
```

# Release Process

The release process for this project is described [here](RELEASE.md).

# Code coverage

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

# Disclaimer

All claims, content, designs, algorithms, estimates, roadmaps,
specifications, and performance measurements described in this project
are done with the Solana Labs, Inc. (“SL”) good faith efforts. It is up to
the reader to check and validate their accuracy and truthfulness.
Furthermore, nothing in this project constitutes a solicitation for
investment.

Any content produced by SL or developer resources that SL provides are
for educational and inspirational purposes only. SL does not encourage,
induce or sanction the deployment, integration or use of any such
applications (including the code comprising the Solana blockchain
protocol) in violation of applicable laws or regulations and hereby
prohibits any such deployment, integration or use. This includes the use of
any such applications by the reader (a) in violation of export control
or sanctions laws of the United States or any other applicable
jurisdiction, (b) if the reader is located in or ordinarily resident in
a country or territory subject to comprehensive sanctions administered
by the U.S. Office of Foreign Assets Control (OFAC), or (c) if the
reader is or is working on behalf of a Specially Designated National
(SDN) or a person subject to similar blocking or denied party
prohibitions.

The reader should be aware that U.S. export control and sanctions laws prohibit 
U.S. persons (and other persons that are subject to such laws) from transacting 
with persons in certain countries and territories or that are on the SDN list. 
Accordingly, there is a risk to individuals that other persons using any of the 
code contained in this repo, or a derivation thereof, may be sanctioned persons 
and that transactions with such persons would be a violation of U.S. export 
controls and sanctions law.
