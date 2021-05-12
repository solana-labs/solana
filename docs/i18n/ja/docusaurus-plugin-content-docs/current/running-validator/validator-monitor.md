---
title: バリデータのモニタリング
---

## ゴシップをチェック

バリデータのIPアドレスと**個別公開キー**がゴシップネットワーク上に表示されていることを確認します。

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

## 残高を確認してください。

あなたのアカウント残高は、あなたのバリデータが投票を行うとトランザクション手数料分だけ減少し、リーダを務めると増加します。 `--lamports`は、より細かく観察するためのパスです。

```bash
solana balance --lamports
```

## 投票のアクティビティを確認

`solana vote-account`コマンドは、バリデータの最近の投票状況を表示します。

```bash
solana vote-account ~/vote-account-keypair.json
```

## クラスター情報の取得

クラスタ上のバリデータやクラスタの健全性を監視するための便利な" JSON-RPC エンドポイント"がいくつかあります。

```bash
# Similar to solana-gossip, you should see your validator in the list of cluster nodes
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getClusterNodes"}' http://devnet.solana.com
# If your validator is properly voting, it should appear in the list of `current` vote accounts. If staked, `stake` should be > 0
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}' http://devnet.solana.com
# Returns the current leader schedule
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}' http://devnet.solana.com
# Returns info about the current epoch. slotIndex should progress on subsequent calls.
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://devnet.solana.com
```
