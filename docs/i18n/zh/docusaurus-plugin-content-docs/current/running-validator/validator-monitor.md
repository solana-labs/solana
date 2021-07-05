---
title: 监控验证节点
---

## 检查 Gossip

通过运行以下命令，确认您的验证节点的IP地址和**身份pubkey**在八卦网络中处于可见状态：

```bash
safecoin-gossip spy --entrypoint devnet.safecoin.org:10015
```

## 检查余额

当您的验证节点提交选票时，您的帐户余额应减少交易费用，而在担任领导者后，您的帐户余额应增加。 通过`lamports`进行更详细的观察：

```bash
safecoin balance --lamports
```

## 检查投票活动

`safecoin vote-account`命令可以显示验证者最近的投票活动：

```bash
safecoin vote-account ~/vote-account-keypair.json
```

## 获取集群信息

有几个有用的JSON-RPC端点，用于监视集群上的验证节点以及集群的运行状况：

```bash
＃与safecoin-gossip相似，您应该在集群节点列表中看到您的验证节点
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getClusterNodes"}' http://api.devnet.safecoin.org
# 如果您的验证节点进行了正确的投票，那么它应该出现在“当前”投票帐户列表中。 如果已经质押，那么`stake` 应当为 > 0
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}' http://api.devnet.safecoin.org
# 返回当前的领导者安排表
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}' http://api.devnet.safecoin.org
# 返回当前 epoch 的信息 slotIndex 应该在随后的调用中获得进展。
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://api.devnet.safecoin.org
```
