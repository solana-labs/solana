---
title: Referencia de uso del CLI
---

La [caja solana-cli](https://crates.io/crates/solana-cli) proporciona una herramienta de interfaz de línea de comandos para Solana

## Ejemplos

### Obtener Pubkey

```bash
// Comando
$ pubkey solana-keygen

// Devuelve
<PUBKEY>
```

### Airdrop SOL/Lamports

```bash
// Comando
$ solana airdrop 2

// Devuelve
"2.00000000 SOL"
```

### Obtener Saldo

```bash
// Comando
$ saldo solana

// Devuelve
"3.00050001 SOL"
```

### Confirmar transacción

```bash
// Command
$ solana confirm <TX_SIGNATURE>

// Return
"Confirmed" / "Not found" / "Transaction failed with error <ERR>"
```

### Desplegar programa

```bash
// Command
$ solana deploy <PATH>

// Return
<PROGRAM_ID>
```

## Usos

###

```text

```
