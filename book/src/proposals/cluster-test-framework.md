# Cluster Test Framework

This document proposes the Cluster Test Framework \(CTF\). CTF is a test harness that allows tests to execute against a local, in-process cluster or a deployed cluster.

## Motivation

The goal of CTF is to provide a framework for writing tests independent of where and how the cluster is deployed. Regressions can be captured in these tests and the tests can be run against deployed clusters to verify the deployment. The focus of these tests should be on cluster stability, consensus, fault tolerance, API stability.

Tests should verify a single bug or scenario, and should be written with the least amount of internal plumbing exposed to the test.

## Design Overview

Tests are provided an entry point, which is a `contact_info::ContactInfo` structure, and a keypair that has already been funded.

Each node in the cluster is configured with a `validator::ValidatorConfig` at boot time. At boot time this configuration specifies any extra cluster configuration required for the test. The cluster should boot with the configuration when it is run in-process or in a data center.

Once booted, the test will discover the cluster through a gossip entry point and configure any runtime behaviors via validator RPC.

## Test Interface

Each CTF test starts with an opaque entry point and a funded keypair. The test should not depend on how the cluster is deployed, and should be able to exercise all the cluster functionality through the publicly available interfaces.

```text
use crate::contact_info::ContactInfo;
use solana_sdk::signature::{Keypair, KeypairUtil};
pub fn test_this_behavior(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
)
```

## Cluster Discovery

At test start, the cluster has already been established and is fully connected. The test can discover most of the available nodes over a few second.

```text
use crate::gossip_service::discover_nodes;

// Discover the cluster over a few seconds.
let cluster_nodes = discover_nodes(&entry_point_info, num_nodes);
```

## Cluster Configuration

To enable specific scenarios, the cluster needs to be booted with special configurations. These configurations can be captured in `validator::ValidatorConfig`.

For example:

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_validator_exit = true;
let local = LocalCluster::new_with_config(
                num_nodes,
                10_000,
                100,
                &validator_config
                );
```

## How to design a new test

For example, there is a bug that shows that the cluster fails when it is flooded with invalid advertised gossip nodes. Our gossip library and protocol may change, but the cluster still needs to stay resilient to floods of invalid advertised gossip nodes.

Configure the RPC service:

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_rpc_gossip_push = true;
validator_config.rpc_config.enable_rpc_gossip_refresh_active_set = true;
```

Wire the RPCs and write a new test:

```text
pub fn test_large_invalid_gossip_nodes(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
) {
    let cluster = discover_nodes(&entry_point_info, num_nodes);

    // Poison the cluster.
    let client = create_client(entry_point_info.client_facing_addr(), VALIDATOR_PORT_RANGE);
    for _ in 0..(num_nodes * 100) {
        client.gossip_push(
            cluster_info::invalid_contact_info()
        );
    }
    sleep(Durration::from_millis(1000));

    // Force refresh of the active set.
    for node in &cluster {
        let client = create_client(node.client_facing_addr(), VALIDATOR_PORT_RANGE);
        client.gossip_refresh_active_set();
    }

    // Verify that spends still work.
    verify_spends(&cluster);
}
```

