## 重新启动集群

### 步骤1。 确定集群将在以下位置重新启动的插槽


乐观确认的最高插槽是开始的最佳插槽，可通过查找[这里](https://github.com/solana-labs/solana/blob/0264147d42d506fb888f5c4c021a998e231a3e74/core/src/optimistic_confirmation_verifier.rs#L71)的性能指标数据。  否则，请使用最后一个根。

调用这个插槽 `SLOT_X`

### 步骤2。 停止(一个或多个) 验证节点

### 步骤3。 安装新的solana版本

### 步骤4。 在插槽`SLOT_X`上为插槽`SLOT_X`的硬分叉创建一个新的快照。

```bash
$ solana-ledger-tool -l ledger create-snapshot SLOT_X ledger --硬分叉 SLOT_X
```

现在，账本目录应包含一个新的快照。 `solana-ledger-tool-create-snapshot` 也会输出新的碎片版本和bank哈希值，分别调用 NEW\_SHRED\_VERSION 和 NEW\_BANK\_HASH。

调整验证节点的参数：

```bash
 --wait-for-supermajority SLOT_X
 --expected-bank-hash NEW_BANK_HASH
```

然后重新启动验证节点。

使用日志确认验证程序已启动，并且现在处于`SLOT_X`的等待模式，正在等待绝大多数投票。

### 步骤5。 在 Discord 上宣布重启：

在#announcements 频道发布如下内容(酌情调整文本)：

> Hi @Validators,
>
> We've released v1.1.12 and are ready to get testnet back up again.
>
> Steps: 1. Install the v1.1.12 release: https://github.com/solana-labs/solana/releases/tag/v1.1.12 2. a. Preferred method, start from your local ledger with:
>
> ```bash
solana-validator
  --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --hard-fork SLOT_X                  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --no-snapshot-fetch                 # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --entrypoint entrypoint.testnet.solana.com:8001
  --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
  --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
  --no-untrusted-rpc
  --limit-ledger-size
  ...                                # <-- your other --identity/--vote-account/etc arguments
```

b. If your validator doesn't have ledger up to slot SLOT_X or if you have deleted your ledger, have it instead download a snapshot with:

```bash
solana-validator
  --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --entrypoint entrypoint.testnet.solana.com:8001
  --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
  --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
  --no-untrusted-rpc
  --limit-ledger-size
  ...                                # <-- your other --identity/--vote-account/etc arguments
```

     You can check for which slots your ledger has with: `solana-ledger-tool -l path/to/ledger bounds`

3. 等待80%的质押在线

要确认您已经正确重新启动验证程序并且在等待80%质押： a。 查找八卦日志消息中`可见的N％活跃质押` b. 通过RPC询问它在哪个插槽上：`solana --url http://127.0.0.1:8899 slot`。  它应该返回`SLOT_X`，直到我们获得80％的质押

感谢！

### 步骤7。 等待并关注最新消息

重新启动验证器时，请对其进行监视。 回答疑问，帮助其他同伴，
