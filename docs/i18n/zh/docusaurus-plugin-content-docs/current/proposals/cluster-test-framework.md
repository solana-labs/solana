---
title: 群集测试框架
---

本文档提出了集群测试框架\(CTF\)。 CTF 是一个测试工具，它对一个当地、程序中或已部署的集群进行测试。

## 激励措施

CTF 的目标是提供一个不论该组部署在何处和如何部署的情况下编写检验报告的框架。 这些试验可以根据部署的集群进行回归测试，从而核查部署情况。 这些测试的重点在于集群稳定性、共识、容错性和 API 稳定性。

测试用于验证单个错误或场景，并且写入最少接触测试的内部管道数量。

## 设计概览

测试提供了一个切入点，它是一个 `contact_info::ContactInfo` 结构和已经有金额的密钥对。

集群中的每个节点在启动时都已经配置 `validator::ValidatorConfig`。 在启动时，该配置指定了测试所需的额外集群配置。 当集群已经运行或处于数据中心时，集群应当与配置一起启动。

一旦启动后，测试将通过 gossip 入口点发现集群，并通过验证节点 RPC 配置任何运行状况。

## 测试接口

每次 CTF 测试都从一个模糊的切入点和一个有资金的密钥对开始。 试验不应取决于该群集的部署方式，并且应该通过公开的接口使用所有的集群功能。

```text
use crate::contact_info::ContactInfo;
use solana_sdk::signature::{Keypair, Signer};
pub fn test_this_behavior(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
)
```

## 群集发现

在测试开始时，集群已经建立并且完全连接。 测试可以在几秒钟内发现大多数可用的节点。

```text
use crate::gossip_service::discover_nodes;

// 在几秒钟之内发现集群。
let cluster_nodes = discover_nodes(&entry_point_info, num_nodes);
```

## 集群配置

为了启用特定场景，需要通过特殊配置启动集群。 这些配置可以在 `validator::ValidatorConfig` 中查看。

例如：

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

## 如何设计一个新测试

例如，有一个错误表示，当集群被无效散播的 gossip 节点挟持时，集群会失败。 我们的 gossip 库和协议可能会改变，但集群必须保持应对无效散播节点的能力。

配置 RPC 服务：

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_rpc_gossip_push = true;
validator_config.rpc_config.enable_rpc_gossip_refresh_active_set = true;
```

连接 RPP 并写入一个新测试：

```text
pub fn test_large_invalid_gossip_nodes(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
) {
    let cluster = discover_nodes(&entry_point_info, num_nodes);

    // 攻击集群。
    let client = create_client(entry_point_info.client_facing_addr(), VALIDATOR_PORT_RANGE);
    for _ in 0..(num_nodes * 100) {
        client.gossip_push(
            cluster_info::invalid_contact_info()
        );
    }
    sleep(Durration::from_millis(1000));

    // 强迫在线节点结合进行重启。
    for node in &cluster {
        let client = create_client(node.client_facing_addr(), VALIDATOR_PORT_RANGE);
        client.gossip_refresh_active_set();
    }

    // 验证该笔交易仍然有效。
    verify_spends(&cluster);
}
```
