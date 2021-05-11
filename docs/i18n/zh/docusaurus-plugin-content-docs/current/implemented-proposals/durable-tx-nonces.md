---
title: 持久交易编号（Nonces）
---

## 问题

为了防止重花，Solana 交易包含一个有“最近”块哈希值的非空值。 包含太久的(撰文时为 ~2分钟) 区块哈希交易被网络拒绝为无效。 很不幸，某些情况下（例如托管服务），需要更多时间来生成交易的签名。 需要一种机制来支持这些潜在的线下网络参与者。

## 需求

1. 交易签名必须包括编号值
2. 即使是在签名密钥披露的情况下，nonce 也不可以重复使用。

## 基于合约的解决办法

这里我们描述了一种基于合约的解决方法，其中客户端可以在最近一次交易的 `recent_blockhash` 字段中“存放”一个未来可以使用的 nonce 值。 这个方法类似于通过一些 CPU ISA 实现的比较和交换原子指令。

当使用后续的 nonce 时，客户端必须首先从账户数据查询它的值。 现在的交易是正常的，但需要满足以下附加要求：

1. 后续的 nonce 值用于 `recent_blockhash` 字段
2. `AdvanceNonceAccount` 指令是交易中第一次发出的

### 合约机制

未完成工作：svgbob 将其变成一个流程图

```text
Start
Create Account
  state = Uninitialized
NonceInstruction
  if state == Uninitialized
    if account.balance < rent_exempt
      error InsufficientFunds
    state = Initialized
  elif state != Initialized
    error BadState
  if sysvar.recent_blockhashes.is_empty()
    error EmptyRecentBlockhashes
  if !sysvar.recent_blockhashes.contains(stored_nonce)
    error NotReady
  stored_hash = sysvar.recent_blockhashes[0]
  success
WithdrawInstruction(to, lamports)
  if state == Uninitialized
    if !signers.contains(owner)
      error MissingRequiredSignatures
  elif state == Initialized
    if !sysvar.recent_blockhashes.contains(stored_nonce)
      error NotReady
    if lamports != account.balance && lamports + rent_exempt > account.balance
      error InsufficientFunds
  account.balance -= lamports
  to.balance += lamports
  success
```

客户端想要使用该功能，首先需要在系统程序下创建一个 nonce 帐户。 此帐户将处于 `Uninitialized` 状态，且没有存储哈希，因此无法使用。

要初始化该新创建的帐户，必须发出 `InitializeNonceAccount` 指令。 该指令需要一个 `Pubkey`参数，它位于账户 [授权](../offline-signing/durable-nonce.md#nonce-authority) 中。 Nonce 帐户必须是 [rent-exempt](rent.md#two-tiered-rent-regime) 状态，才能满足数据持续性功能的要求， 因此它要求初始化之前先存入足够的 lamports。 初始化成功后，集群最近的区块哈希与指定 nonce 授权的 `Pubkey` 将一同存储。

`AdvanceNonceAccount` 指令用于管理帐户存储的 nonce 值。 它在账户的状态数据中存储集群最新的区块哈希，如果与已存储的值相匹配，那么会提示失败。 这个检查可以防止在同一个区块内重新广播交易。

由于 nonce 帐户的 [免租](rent.md#two-tiered-rent-regime) 要求，一个自定义提现指令用于将资产从帐户中移出。 `WithdrawNonceAccount` 指令需要一个单一参数，提示取款信号，强制免除租金，防止账户余额下降至低于免租金的最低值。 该检查的一个例外情况是，最终余额是零，从而让账户能够删除。 这个账户关闭详细信息还有一个额外要求，即存储的 nonce 值必须与集群最近的区块不匹配， 正如 `AdvanceNonceAccount`。

账户的 [nonce authority](../offline-signing/durable-nonce.md#nonce-authority) 可以通过 `AuthorizeNonceAccount` 说明进行更改。 它需要传入一个参数，新授权的 `Pubkey`。 执行该指令将把完全的帐户及其余额控制权转移给新的授权。

> `AdvanceNonceAccount`，`WithdrawNonceAccount` 和 `AuthorizeNonceAccount` 都需要当前 [nonce authority](../offline-signing/durable-nonce.md#nonce-authority) 才能签署交易。

### 运行时（Runtime）支持

合约本身并不足以实现这个功能。 为了在交易上强制执行一个现有的`recent_blockhash`，并防止通过失败的交易重放来窃取费用，runtime的修改是必要的。

任何未能通过通常的`check_hash_age`验证的交易将被测试为持久交易Nonce。 这是由包括一个`AdvanceNonceAccount`指令作为交易中的第一条指令发出的信号。

如果runtime确定使用了一个持久事务Nonce，它将采取以下额外的操作来验证事务：

1. 加载`Nonce`指令中指定的`NonceAccount`。
2. `NonceAccount` 的数据字段反序列化`NonceState`，并确认其处于`Initialized`状态。
3. 存储在`NonceAccount`中的nonce值与交易的`recent_blockhash`字段中指定的nonce值进行匹配测试。

如果上述三项检查都成功，则允许交易继续验证。

由于以`InstructionError`失败的交易会被收取费用，并且其状态的改变会被回滚，所以如果`AdvanceNonceAccount`指令被回滚，则费用可能会被盗。 恶意验证者可以重放失败的交易，直到存储的nonce被成功推进。 Runtime的更改可以防止这种行为。 当一个持久的nonce事务失败时，除了`AdvanceNonceAccount`指令外，还有一个`InstructionError`，nonce账户会像往常一样被回滚到执行前的状态。 然后，runtime将其nonce值和高级nonce账户存储起来，就像已经成功了。
