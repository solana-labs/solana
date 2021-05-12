---
title: CLI 사용범 예제
---

[ solana-cli crate](https://crates.io/crates/solana-cli)는 솔라나를위한 커맨드라인 인터페이스 도구를 제공합니다.

## 예제

### Get Pubkey

```bash
// Command
$ solana-keygen pubkey

// Return
<PUBKEY>
```

### Airdrop SOL/Lamports

```bash
// Command
$ solana airdrop 2

// Return
"2.00000000 SOL"
```

### Get Balance

```bash
// Command
$ solana balance

// Return
"3.00050001 SOL"
```

### Confirm Transaction

```bash
// Command
$ solana confirm <TX_SIGNATURE>

// Return
"Confirmed" / "Not found" / "Transaction failed with error <ERR>"
```

### Deploy program

```bash
// Command
$ solana deploy <PATH>

// Return
<PROGRAM_ID>
```

## 사용법

###

```text

```
