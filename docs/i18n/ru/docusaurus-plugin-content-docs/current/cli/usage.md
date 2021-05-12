---
title: Справочник по использованию CLI
---

[solana-cli crate](https://crates.io/crates/solana-cli) предоставляет инструмент интерфейса командной строки для Solana

## Примеры

### Получить Pubkey

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

### Вывод баланса

```bash
// Command
$ solana balance

// Return
"3.00050001 SOL"
```

### Проверка статуса транзакции

```bash
// Command
$ solana confirm <TX_SIGNATURE>

// Return
"Confirmed" / "Not found" / "Transaction failed with error <ERR>"
```

### Развертывание программы

```bash
// Command
$ solana deploy <PATH>

// Return
<PROGRAM_ID>
```

## Использование
###
```text

```

