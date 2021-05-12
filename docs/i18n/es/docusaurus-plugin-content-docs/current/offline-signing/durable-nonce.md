---
title: Nonces de transacción duradera
---

Los nonces de una transacción duradera son un mecanismo para eludir el típico tiempo de vida corto de una transacción [`recent_blockhash`](developing/programming-model/transactions.md#recent-blockhash). Se implementan como un programa Solana, cuya mecánica puede leerse en la [propuesta](../implemented-proposals/durable-tx-nonces.md).

## Ejemplos de uso

Detalles de uso completo para comandos CLI durables de nonce se pueden encontrar en la [referencia CLI](../cli/usage.md).

### Autoridad Nonce

La autoridad sobre una cuenta nonce puede asignarse opcionalmente a otra cuenta. Al hacerlo, la nueva autoridad hereda el control total sobre la cuenta nonce de la autoridad anterior, incluido el creador de la cuenta. Esta función permite la creación de acuerdos de propiedad de cuentas más complejos y direcciones derivadas de la cuenta no asociadas con un keypair. El argumento `--nonce-authority <AUTHORITY_KEYPAIR>` se utiliza para especificar esta cuenta y es soportado por los siguientes comandos

- `create-nonce-account`
- `new-nonce`
- `withdraw-from-nonce-account`
- `authorize-nonce-account`

### Creación de cuenta Nonce

La función de nonce de transacción duradera utiliza una cuenta para almacenar el siguiente valor de nonce. Las cuentas de nonce durables deben ser [rent-exempt](../implemented-proposals/rent.md#two-tiered-rent-regime), así que necesitan llevar el saldo mínimo para lograr esto.

Una cuenta nonce se crea primero generando un nuevo keypair y luego crear la cuenta en cadena

- Comando

```bash
solana-keygen new -o nonce-keypair.json
solana create-nonce-account nonce-keypair.json 1
```

- Salida

```text
2SymGGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrLySdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL
```

> Para mantener el keypair completamente desconectado, utilice [Papel Wallet](wallet-guide/paper-wallet.md) generando [instrucciones](wallet-guide/paper-wallet.md#seed-phrase-generation) en su lugar

> [Documentación de uso completo](../cli/usage.md#solana-create-nonce-account)

### Consultar el valor Nonce Almacenado

Crear una transacción nonce duradera requiere pasar el valor nonce almacenado como el valor al argumento `--blockhash` al firmar y enviar. Obtén el valor nonce actualmente almacenado con

- Comando

```bash
solana nonce nonce-keypair.json
```

- Salida

```text
8GRipryfxcsxN8mAGjy8zbFo9ezaUsh47TsPzmZbuytU
```

> [Documentación de uso completo](../cli/usage.md#solana-get-nonce)

### Avanzando el valor Nonce Almacenado

Aunque normalmente no se necesita fuera de una transacción más útil, el valor nonce almacenado puede ser avanzado por

- Comando

```bash
solana new-nonce nonce-keypair.json
```

- Salida

```text
44jYe1yPKrjuYDmoFTdgPjg8LFpYyh1PFKJqm5SC1PiSyAL8iw1bhadcAX1SL7KDmREEkmHpYvreKoNv6fZgfvUK
```

> [Documentación de uso completo](../cli/usage.md#solana-new-nonce)

### Mostrar cuenta Nonce

Inspeccionar una cuenta de nonce en un formato más agradable con

- Comando

```bash
solana nonce-account nonce-keypair.json
```

- Salida

```text
balance: 0.5 SOL
balance mínimo requerido: 0.00136416 SOL
nonce: DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS
```

> [Documentación de uso completo](../cli/usage.md#solana-nonce-account)

### Retirar fondos de una cuenta Nonce

Retira fondos de una cuenta de nonce con

- Comando

```bash
solana withdraw-from-nonce-account nonce-keypair.json ~/.config/solana/id.json 0.5
```

- Salida

```text
3foNy1SBqwXSsfSfTdmYKDuhnVheRnKXpoPySiUDBVeDEs6iMVokgqm7AqfTjbk7QBE8mqomvMUMNQhtdMvFLide
```

> Cerrar una cuenta de nonce retirando el saldo completo

> [Documentación de uso completo](../cli/usage.md#solana-withdraw-from-nonce-account)

### Asignar una nueva autoridad a una cuenta Nonce

Reasignar la autoridad de una cuenta nonce después de la creación con

- Comando

```bash
solana authorize-nonce-account nonce-keypair.json nonce-authority.json
```

- Salida

```text
3F9cg4zN9wHxLGx4c3cUKmqpej4oa67QbALmChsJbfxTgTffRiL3iUehVhR9wQmWgPua66jPuAYeL1K2pYYjbNoT
```

> [Documentación de uso completo](../cli/usage.md#solana-authorize-nonce-account)

## Otros comandos que soportan los nonces duraderos

Para hacer uso de los nonces duraderos con otros subcomandos de la CLI, se deben admitir dos argumentos.

- `--nonce`, especifica la cuenta almacenando el valor de nonce
- `--nonce-authority`, especifica una [autoridad nonce opcional](#nonce-authority)

Los siguientes subcomandos han recibido este tratamiento hasta ahora

- [`pagar`](../cli/usage.md#solana-pay)
- [`delegar-Stake`](../cli/usage.md#solana-delegate-stake)
- [`desactivar-stake`](../cli/usage.md#solana-deactivate-stake)

### Ejemplo de pago usando Nonce Durable

Aquí demostramos que Alice paga a Bob 1 SOL usando un nonce duradero. El procedimiento es el mismo para todos los subcomandos que soportan nonces duraderos

#### - Crear cuentas

Primero necesitamos algunas cuentas para Alice, el nonce de Alice y Bob

```bash
$ solana-keygen nuevo -o alice.json
$ solana-keygen nuevo -o nonce.json
$ solana-keygen nuevo -o bob.json
```

#### - Cuenta de Fondo de Alice

Alice necesitará algunos fondos para crear una cuenta de nonce y enviar a Bob. Airdrop algunos SOL a ella

```bash
$ solana airdrop -k alice.json 10
10 SOL
```

#### - Crear la cuenta nonce de Alice

Ahora Alice necesita una cuenta nonce. Crear uno

> En este caso, no se emplea ninguna autoridad [nonce authority](#nonce-authority) por separado, por lo que `alice.json` tiene plena autoridad sobre la cuenta nonce

```bash
$ solana create-nonce-account -k alice.json nonce.json 1
3KPZr96BTsL3hqera9up82KAU462Gz31xjqJ6eHUAjF935Yf8i1kmfEbo6SVbNaACKE5z6gySrNjVRvmS8DcPuwV
```

#### - Un primer intento fallido de pagar a Bob

Alicia intenta pagar a Bob, pero toma demasiado tiempo para firmar. El blockhash especificado expira y la transacción falla

```bash
$ solana pay -k alice.json --blockhash expiredDTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 bob.json 1
[2020-01-02T18:48:28.462911000Z ERROR solana_cli::cli] Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
Error: Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
```

#### - ¡Nonce al rescate!

Alice reintenta la transacción, esta vez especificando su cuenta de nonce y el blockhash almacenado allí

> Recuerda, `alice.json` es la [autoridad nonce](#nonce-authority) en este ejemplo

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
balance mínimo requerido: 0.00136416 SOL
nonce: F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7
```

```bash
$ solana pay -k alice.json --blockhash F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 --nonce nonce.json bob.json 1
HR1368UKHVZyenmH7yVz5sBAijV6XAPeWbEiXEGVYQorRMcoijeNAbzZqEZiH8cDB8tk65ckqeegFjK8dHwNFgQ
```

#### - ¡Exito!

¡La transacción tiene éxito! Bob recibe 1 SOL de Alice y el nonce almacenado de Alice avanza a un nuevo valor

```bash
$ solana balance -k bob.json
1 SOL
```

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
balance mínimo requerido: 0.00136416 SOL
nonce: 6bjroqDcZgTv6Vavhqf81oBHTv3aMnX19UTB51YhAZnN
```
