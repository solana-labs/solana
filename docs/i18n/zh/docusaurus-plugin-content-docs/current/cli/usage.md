---
title: CLI 使用参考
---

[solana-cli crate](https://crates.io/crates/solana-cli) 为 Safecoin 提供了一个命令行界面工具

## 示例：

### 获取公钥

```bash
// 命令
$safecoin-keygen pubkey

// 返回
<PUBKEY>
```

### 空投 SAFE/Lamports

```bash
// 命令
$ safecoin airdrop 2

// 返回
"2.0000000 SAFE"
```

### 获取余额

```bash
// 命令
$ safecoin balance

// 返回
"3.00050001 SAFE"
```

### 确认交易

```bash
// 命令
$ safecoin confirm <TX_SIGNATURE>

// 返回
"Confirmed" / "Not found" / "Transaction failed with error <ERR>"
```

### 部署程序

```bash
// 命令
$ safecoin deploy <PATH>

// 返回
<PROGRAM_ID>
```

## 使用方法
###
```text

```
