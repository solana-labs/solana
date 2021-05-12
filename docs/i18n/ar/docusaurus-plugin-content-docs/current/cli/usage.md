---
title: مرجع إستخدام أداة CLI
---

يوفر صندوق [solana-cli crate](https://crates.io/crates/solana-cli) أداة واجهة سطر الأوامر لـ Solana

## أمثلة

### الحصول على المفتاح العمومي (pubkey)

```bash
// Command
$ solana-keygen pubkey

// Return
<PUBKEY>
```

### توزيع حر لـ SOL/Lamports

```bash
// Command
$ solana airdrop 2

// Return
"2.00000000 SOL"
```

### إظهار الرصيد (Get Balance)

```bash
// Command
$ solana balance

// Return
"3.00050001 SOL"
```

### تأكيد المُعاملة (Confirm Transaction)

```bash
/ Command
$ solana confirm <TX_SIGNATURE>

// Return
"Confirmed" / "Not found" / "Transaction failed with error <ERR>"
```

### تنفيذ البرنامج (Deploy program)

```bash
// Command
$ solana deploy <PATH>

// Return
<PROGRAM_ID>
```

## الإستخدام (Usage)
###
```text

```

