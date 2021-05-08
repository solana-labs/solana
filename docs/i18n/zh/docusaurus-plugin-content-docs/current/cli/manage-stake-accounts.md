---
title: 管理质押账户
---

如果想将质押分配到多个不同的验证节点，您需要为每个验证节点创建一个单独的质押帐户。 如果你按照约定创建了一个种子（seed）为"0"的质押帐户，那么第二个则是“1”，第三个是“2”，以此类推，然后您可以使用 `solana-stock-account` 工具对所有的账户进行单次调用。 您可以用它来汇总所有帐户的余额，将帐户移动到一个新钱包，或设置新的权限。

## 使用方法

### 创建一个质押账户

在质押公钥上创建一个派生的质押帐户并转账进去：

```bash
solana-stake-accounts new <FUNDING_KEYPAIR> <BASE_KEYPAIR> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

### 账户统计

统计派生账户的数量：

```bash
solana-stake-accounts count <BASE_PUBKEY>
```

### 获取质押账户余额

汇总派生抵押账户的余额：

```bash
solana-stake-accounts balance <BASE_PUBKEY> --num-accounts <NUMBER>
```

### 获取质押账户地址

列出来自给定公钥的每一个质押账户地址：

```bash
solana-stake-accounts addresses <BASE_PUBKEY> --num-accounts <NUMBER>
```

### 设置新权限

为生成的每个抵押帐户设置新权限：

```bash
solana-stake-accounts authorize <BASE_PUBKEY> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```

### 重定向质押账户

重定向质押账户:

```bash
solana-stake-accounts rebase <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --num-accounts <NUMBER> \
    --fee-payer <KEYPAIR>
```

对每个质押账户进行原子级别重置并授权，请使用 'move' 命令：

```bash
solana-stake-accounts move <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```
