---
title: 离线交易签名
---

一些安全模型要求保留签名密钥，因此签名过程与交易创建和网络广播分开。 示例包括：

- 从地理位置不同的签名者收集的签名在[多签名方案](cli/usage.md#multiple-witnesses)
- 使用 [气隙（airgapped）](https://en.wikipedia.org/wiki/Air_gap_(networking))签名设备来签名交易

本文档介绍了如何使用Solana的CLI分别签名和提交交易。

## 支持离线签名的命令

当前，有以下命令支持离线签名：

- [`创建质押账户`](cli/usage.md#solana-create-stake-account)
- [`停用质押`](cli/usage.md#solana-deactivate-stake)
- [`委托质押`](cli/usage.md#solana-delegate-stake)
- [`拆分质押`](cli/usage.md#solana-split-stake)
- [`质押授权`](cli/usage.md#solana-stake-authorize)
- [`设置质押锁定`](cli/usage.md#solana-stake-set-lockup)
- [`转账`](cli/usage.md#solana-transfer)
- [`提现质押`](cli/usage.md#solana-withdraw-stake)

## 离线签名交易

要离线签署交易，请在命令行上传递以下参数

1. `--sign-only`，阻止客户端将签名的交易提交到网络。 相反，pubkey/签名对被打印到stdout。
2. `--blockhash BASE58_HASH`，允许调用者指定用于填写交易的 `最近的区块哈希` 字段。 这可以满足一些目的。例如： _ 取消连接到网络的需要，并通过RPC 查询最近的区块哈希 _ 让签名者能够在多个签名中协调区块哈希方案

### 示例：离线签名付款

命令

```bash
solana@offline$ solana pay --sign-only --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    recipient-keypair.json 1
```

输出

```text

Blockhash: 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF
Signers (Pubkey=Signature):
  FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN

{"blockhash":"5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF","signers":["FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN"]}'
```

## 将离线签名交易提交到网络

若要提交已离线签名的交易，请在命令行传入下面的参数

1. `--blockhash BASE58_HASH`，必须与用于签名的区块哈希值相同
2. `--signer BASE58_PUBKEY=BASE58_SIGNATURE`，离线签名者中的一个。 这直接包含在交易中的pubkey(s) 签名，而不用任何本地秘钥对它进行签名

### 示例：提交离线已签名付款

命令

```bash
solana@online$ solana pay --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    --signer FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
    recipient-keypair.json 1
```

输出

```text
4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
```

## 多个会话的离线签名

离线签名也可以在多个会话中进行。 在这种情况下，请为每个角色传递缺席签名者的公钥。 所有指定但未生成签名的发布密钥将在离线签名输出中列出为不存在

### 示例：通过两个离线签名会话进行传输

命令(离线会话 #1)

```text
solana@offline1$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair fee_payer.json \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

输出 (离线会话 #1)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
Absent Signers (Pubkey):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

命令(离线会话 #2)

```text
solana@offline2$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair from.json \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

输出 (离线会话 #2)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ
Absent Signers (Pubkey):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

命令(在线提交)

```text
solana@online$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL \
    --signer 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy \
    --signer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

输出 (在线提交)

```text
ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

## 购买更多时间来签名

通常，Solana交易必须由网络在其`recent_blockhash`字段中距区块哈希值数个插槽内进行签名并接受(在撰写本文时约为2分钟)。 如果您的签名过程花费的时间超过此时间，则[Durable Transaction Nonce](offline-signing/durable-nonce.md) 可以为您提供所需的额外时间。
