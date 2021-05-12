---
title: Nonces de transacción duradera
---

## Problema

Para evitar repetir, las transacciones de Solana contienen un campo nonce poblado con un valor de bloque. Una transacción que contiene un blockhash demasiado antiguo (~2min a partir de esta escritura) es rechazada por la red como inválida. Lamentablemente, ciertos casos de uso, como los servicios custodiales, requieren más tiempo para producir una firma para la transacción. Se necesita un mecanismo para habilitar estos participantes potenciales en la red sin conexión.

## Requisitos

1. La firma de la transacción necesita cubrir el valor de nonce
2. El nonce no debe ser reutilizable, ni siquiera en el caso de firmar la revelación de clave

## Una solución basada en contrato

Aquí describimos una solución al problema basada en el contrato mediante el cual un cliente puede "almacenar" un valor nonce para uso futuro en el campo `recent_blockhash` de una transacción. Este enfoque es similar a la instrucción atómica Compare and Swap, implementada por algunos ISAs de CPU.

Cuando se utiliza un nonce duradero, el cliente debe consultar primero su valor a partir de los datos de la cuenta. Una transacción se construye ahora de la manera normal, pero con los siguientes requisitos adicionales:

1. El valor de nonce durable se utiliza en el campo `recent_blockhash`
2. Una instrucción de `AdvanceNonceAccount` es la primera emitida en la transacción

### Mecánicas de contrato

TODO: svgbob esto en un diagrama de flujo

```text
Iniciar
Crear cuenta
  estado = no inicializado
InstrucciónNoCe
  if state == Uninitialized
    if account.balance < rent_exempt
      error InsufficientFunds
    state = Inicializado
  elif state != Initialized
    error BadState
  if sysvar.recent_blockhashes.is_empty()
    error EmptyRecentBlockhashes
  if !sysvar.recent_blockhashes.contains(stored_nonce)
    error NotReady
  stored_hash = sysvar.recent_blockhashes[0]
  éxito
RetirarInstrucción(to, lamports)
  if state == Uninitialized
    if !signers.contains(owner)
      error MissingRequiredSignatures
  elif state == Initialized
    if !sysvar.recent_blockhashes.contains(stored_nonce)
      error NotReady
    if lamports != account.balance && lamports + rent_exempt > account.balance
      error InsufficientFunds
  account.balance -= lamports
  to.balance += lamports
  éxito
```

Un cliente que desee utilizar esta característica comienza por crear una cuenta de nonce en el programa del sistema. Esta cuenta estará en el estado `` no inicializado sin hash almacenado, y por lo tanto inutilizable.

Para inicializar una cuenta recién creada, una instrucción de `InitializeNonceAccount` debe ser emitida. Esta instrucción toma un parámetro, el `Pubkey` de la cuenta [autoridad](../offline-signing/durable-nonce.md#nonce-authority). Las cuentas de Nonce deben ser [rent-exempt](rent.md#two-tiered-rent-regime) para cumplir con los requisitos de persistencia de datos de la característica, y como tal, requieren que suficientes lamports sean depositadas antes de que puedan ser inicializadas. Tras la inicialización exitosa, el blockhash más reciente del clúster se almacena junto con la autoridad nonce especificada `Pubkey`.

La instrucción `AdvanceNonceAccount` se utiliza para gestionar el valor nonce almacenado de la cuenta. Almacena el bloque más reciente del clúster en los datos de estado de la cuenta, fallando si coincide con el valor ya almacenado allí. Esta comprobación evita la repetición de transacciones dentro del mismo bloque.

Debido al requisito de [exención de renta](rent.md#two-tiered-rent-regime) de las cuentas nonce, se utiliza una instrucción de retiro personalizada para mover los fondos de la cuenta. La instrucción `WithdrawNonceAccount` toma un solo argumento, las lamports para retirar, e impone la exención de rentas evitando que el saldo de la cuenta caiga por debajo del mínimo de alquiler. Una excepción a esta comprobación es si el saldo final sería cero lamports, lo que hace que la cuenta sea elegible para su eliminación. Este detalle de cierre de cuenta tiene el requisito adicional de que el valor nonce almacenado no debe coincidir con el blockhash más reciente del cluster, según `AdvanceNonceAccount`.

La [autoridad nonce](../offline-signing/durable-nonce.md#nonce-authority) de la cuenta puede cambiarse usando la instrucción `AuthorizeNonceAccount`. Toma un parámetro, la `Pubkey` de la nueva autoridad. La ejecución de esta instrucción otorga el control total de la cuenta y su saldo a la nueva autoridad.

> `AdvanceNonceAccount`, `WithdrawNonceAccount` y `AuthorizeNonceAccount` todos requieren la actual [autoridad de nonce](../offline-signing/durable-nonce.md#nonce-authority) para que la cuenta firme la transacción.

### Soporte de Ejecución

El contrato por sí solo no es suficiente para aplicar esta función. Para imponer un `recent_blockhash` existente en la transacción y evitar el robo de tarifas a través de la repetición de transacciones fallidas, son necesarias modificaciones en el tiempo de ejecución.

Cualquier transacción que falle la validación habitual de `check_hash_age` será probada para una Transacción Durable Nonce. Esto se indica incluyendo una instrucción `AdvanceNonceAccount` como la primera instrucción en la transacción.

Si el tiempo de ejecución determina que se está utilizando un Nonce de Transacción Duradera, tomará las siguientes acciones adicionales para validar la transacción:

1. La `NonceAccount` especificada en la instrucción `Nonce` está cargada.
2. El `NonceState` es deserializado del campo de datos de `NonceAccount`y confirmado que está en el estado `Inicializado`.
3. El valor nonce almacenado en el `NonceAccount` es probado para que coincida con el especificado en el campo `recent_blockhash` de la transacción.

Si las tres comprobaciones anteriores tienen éxito, la transacción puede continuar con validación.

Dado que a las transacciones que fallan con un `InstructionError` se les cobra una tarifa y se revierten los cambios en su estado, existe una oportunidad de robo de tarifas si se revierte una instrucción `AdvanceNonceAccount`. Un validador malicioso podría repetir la transacción fallida hasta que la nonce almacenada se avance con éxito. Los cambios en el tiempo de ejecución evitan este comportamiento. Cuando una transacción duradera de nonce falla con un `InstructionError` aparte de la instrucción `AdvanceNonceAccount`, la cuenta de nonce se devuelve a su estado previo a la ejecución como es habitual. A continuación, el tiempo de ejecución avanza su valor nonce y la cuenta nonce avanzada se almacena como si tuviera éxito.
