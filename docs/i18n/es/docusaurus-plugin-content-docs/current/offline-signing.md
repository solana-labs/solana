---
title: Firmar Transacción sin conexión
---

Algunos modelos de seguridad requieren mantener las claves firmadas, y por lo tanto el proceso de firma, separado de la creación de transacciones y la transmisión de red. Los ejemplos incluyen:

- Recopilando firmas de firmas geográficamente separadas en un [esquema de firmas múltiples](cli/usage.md#multiple-witnesses)
- Firmar transacciones utilizando un [airgapped](https://en.wikipedia.org/wiki/Air_gap_(networking)) dispositivo de firma

Este documento describe el uso de la CLI de Solana para firmar por separado y enviar una transacción.

## Comandos que apoyan la firma sin conexión

Actualmente, los siguientes comandos soportan la firma sin conexión:

- [`crear-cuenta-Stake`](cli/usage.md#solana-create-stake-account)
- [`desactivar-stake`](cli/usage.md#solana-deactivate-stake)
- [`delegar-Stake`](cli/usage.md#solana-delegate-stake)
- [`dividir-stake`](cli/usage.md#solana-split-stake)
- [`autorizar-stake`](cli/usage.md#solana-stake-authorize)
- [`bloqueo-stake`](cli/usage.md#solana-stake-set-lockup)
- [`transferir`](cli/usage.md#solana-transfer)
- [`retirar-stake`](cli/usage.md#solana-withdraw-stake)

## Firmar transacciones fuera de línea

Para firmar una transacción sin conexión, pase los siguientes argumentos en la línea de comandos

1. `--sign-only`, evita que el cliente envíe la transacción firmada a la red. En su lugar, los pares pubkey/signature se imprimen en stdout.
2. `--blockhash BASE58_HASH`, permite al llamante especificar el valor usado para rellenar el campo `recent_blockhash` de la transacción. Esto sirve a un número de propósitos, cómo: _ Elimina la necesidad de conectarse a la red y consulta un blockhash reciente vía RPC _ Permite a los firmantes coordinar el blockhash en un esquema de múltiples firmas

### Ejemplo: Firmar un Pago sin conexión

Comando

```bash
solana@offline$ pago solana --sign-only --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    destinatario-keypair.json 1
```

Salida

```text

Blockhash: 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF
Signers (Pubkey=Signature):
  FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN

{"blockhash":"5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF","signers":["FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN"]}'
```

## Enviando transacciones firmadas sin conexión a la red

Para enviar una transacción que ha sido firmada sin conexión a la red, pase los siguientes argumentos en la línea de comandos

1. `--blockhash BASE58_HASH`, debe ser el mismo blockhash que fue usado para firmar
2. `--signner BASE58_PUBKEY=BASE58_SIGNATURE`, uno para cada firmante offline. Esto incluye los pares de pubkey/signature directamente en la transacción en lugar de firmarlo con cualquier keypair local(es)

### Ejemplo: Enviar un pago firmado sin conexión

Comando

```bash
solana@online$ solana pay --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    --signer FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
    recipient-keypair.json 1
```

Salida

```text
4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
```

## Sesiones múltiples sin conexión

La firma sin conexión también puede tener lugar en varias sesiones. En este escenario, pase la clave pública del firmante ausente para cada rol. Todas las pubkeys que fueron especificadas, pero para las que no se generó ninguna firma se listarán como ausentes en la salida de firma sin conexión

### Ejemplo: Transferir con dos Sesiones sin conexión

Comando (Sesión sin conexión #1)

```text
solana@offline1$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair fee_payer.json \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

Salida (Sesión sin conexión #1)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
Absent Signers (Pubkey):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

Comando (Sesión sin conexión #2)

```text
solana@offline2$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair from.json \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

Salida (Sesión sin conexión #2)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ
Absent Signers (Pubkey):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

Comando (Envío en línea)

```text
solana@online$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL \
    --signer 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy \
    --signer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

Salida (Envío en línea)

```text
ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

## Comprando más tiempo para firmar

Normalmente, una transacción Solana debe ser firmada y aceptada por la red dentro de un número de ranuras desde el blockhash en su campo `recent_blockhash` (~2min en en el momento de escribir este artículo). Si tu procedimiento de firma tarda más de esto, una [Durable Transacción Nonce](offline-signing/durable-nonce.md) puede darte el tiempo adicional que necesitas.
