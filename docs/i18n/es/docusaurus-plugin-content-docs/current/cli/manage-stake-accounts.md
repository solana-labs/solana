---
title: Administrar Cuentas de Stake
---

Si quieres delegar stake a muchos validadores diferentes, necesitarás crear una cuenta de stake diferente para cada uno de ellos. Si sigue la convención de crear la primera cuenta de stake en la semilla "0", la segunda en la "1", la tercera en la "2", y así sucesivamente, entonces la herramienta `solana-stake-accounts` le permitirá operar en todas las cuentas con una sola invocación. Puedes usarlo para sumar los balances de todas las cuentas, mover cuentas a una nueva billetera o establecer nuevas autoridades.

## Usos

### Crear una cuenta Stake

Crear y financiar una cuenta de stake derivada en la clave pública de la autoridad de la participación:

```bash
solana-stake-accounts nueva <FUNDING_KEYPAIR> <BASE_KEYPAIR> <AMOUNT> \
    --stake-authority <PUBKEY> --withdrawal -authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

### Cuentas de cuenta

Cuenta el número de cuentas derivadas:

```bash
recuento de cuentas de solana-stake- <BASE_PUBKEY>
```

### Obtener saldos de cuentas de stake

Suma el saldo de las cuentas de stake derivadas:

```bash
saldo de solana-stake-accounts <BASE_PUBKEY> --num-accounts <NUMBER>
```

### Obtener direcciones de cuentas del stake

Listar la dirección de cada cuenta de stake derivada de la clave pública dada:

```bash
direcciones de solana-stake-accounts <BASE_PUBKEY> --num-accounts <NUMBER>
```

### Establecer nuevas autoridades

Establecer nuevas autoridades en cada cuenta de stake derivado:

```bash
las cuentas solana-stake autorizan <BASE_PUBKEY> \
    --stake-authority <KEYPAIR> --withdrawal -authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdrawal -authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```

### Reubicar las cuentas de stake

Reubicar las cuentas de stake:

```bash
solana-stake-accounts rebase <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --num-accounts <NUMBER> \
    --come-payer <KEYPAIR>
```

Para volver a basar y autorizar atómicamente cada cuenta de stake, utilice el comando "move":

```bash
solana-stake-accounts move <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --withdrawal -authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdrawal -authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```
