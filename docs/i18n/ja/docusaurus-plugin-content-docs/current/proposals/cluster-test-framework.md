---
title: Cluster Test Framework
---

このドキュメントでは、Cluster Test Framework \(CTF\) を提案します。 "CTF"は、"ローカル"、"インプロセスクラスタ"、または"デプロイされたクラスタ"に対してテストを実行できる"テストハーネス"です。

## 動機付け

"CTF"の目標は、クラスタの展開の場所と方法に依存"せず"にテストを作成するためのフレームワークを提供することです。 "Regressions"はこれらのテストでキャプチャすることができ、テストはデプロイされたクラスタに対して実行してデプロイを検証することができます。 これらのテストの焦点は、"クラスタの安定性"、"コンセンサス"、"フォールトトレランス"、"API の安定性"にあります。

テストは単一のバグやシナリオを検証し、テストにさらされた内部配管の最小量で書かれるべきです。

## デザインの概要

テストには、 `contact_info::ContactInfo` 構造体と、すでに資金が入っているキーペアが用意されています。

クラスタ内の各ノードは、ブート時に `validator::ValidatorConfig` を使用して構成されます。 起動時に、この設定ではテストに必要な追加のクラスタ設定を指定します。 クラスタは、プロセス内またはデータセンターで実行されるときに、設定で起動します。

一度起動すると、テストはゴシップエントリポイントを介してクラスタを発見し、バリデータ RPC 経由でランタイムの動作を設定します。

## テストインターフェース

各 CTF テストは、不透明なエントリポイントと資金を供給されたキーペアから始まります。 このテストでは、クラスターの展開方法に依存せず、公開されているインターフェイスを通じてクラスターのすべての機能を実行できる必要があります。

```text
use crate::contact_info::ContactInfo;
use solana_sdk::signature::{Keypair, Signer};
pub fn test_this_behavior(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    num_nodes: usize,
)
```

## クラスターの検出

テストを開始すると、クラスタはすでに確立され、完全に接続されます。 このテストは、利用可能なノードの大部分を数秒で発見することができます。

```text
use crate::gossip_service::discover_nodes;

// 数秒でクラスタを発見。
let cluster_nodes = discover_nodes(&entry_point_info, num_nodes);
```

## クラスター設定

特定のシナリオを有効にするには、クラスタを特別な構成で起動する必要があります。 これらの構成は、 `validator::ValidatorConfig` で取得できます。

例:

```text
let mut validator_config = ValidatorConfig::default();
validator_config.config.enable_validator_exit = true;
let local = LocalCluster::new_config(
                num_nodes,
                10_00,
                100,
                &validator_config
);
```

## 新しいテストを設計する方法

例えば、無効な宣伝されたゴシップノードがあふれたときにクラスタが失敗するというバグがあります。 私たちのゴシップライブラリとプロトコルは変更されるかもしれませんが、クラスタはまだ無効な宣伝されたゴシップノードの洪水に対して回復力を維持する必要があります。

RPC サービスを構成します。

```text
let mut validator_config = ValidatorConfig::default();
validator_config.rpc_config.enable_rpc_gossip_push = true;
validator_config.rpc_config.enable_rpc_gossip_refresh_active_set = true;
```

RPC を配線し、新しいテストを作成します。

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
