---
title: CLIの利用参照
---

["solana-cli crate](https://crates.io/crates/solana-cli)は、Solana のコマンドラインインターフェースツールを提供します。

## 例:

### Pubkey を取得しよう。

```bash
// Command
$ solana-keygen pubkey

// Return
<PUBKEY>
```

### ランポートでの SOL のエアドロップ

```bash
// Command
$ solana airdrop 2

// Return
"2.0000000000 SOL"
```

### 残高の取得

```bash
// Command
$ solana airdrop 2

// Return
"2.0000000000 SOL"
```

### トランザクションの確認

```bash
// Command
$ solana confirm <TX_SIGNATURE>

// Return
"Confirmed" / "Not found" / "Transaction failed with error <ERR>"
```

### プログラムのデプロイをしよう

```bash
// Command
$ solana deploy <PATH>

// Return
<PROGRAM_ID>
```

## 使用法

###

```text

```
