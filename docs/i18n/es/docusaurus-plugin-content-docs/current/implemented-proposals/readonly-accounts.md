---
title: Cuentas de sólo lectura
---

Este diseño cubre el manejo de cuentas de solo lectura y escribible en el [runtime](../validator/runtime.md). Múltiples transacciones que modifican la misma cuenta deben ser procesadas serialmente para que siempre se repitan en el mismo orden. De lo contrario, esto podría introducir el no determinismo en el ledger. Sin embargo, algunas transacciones sólo necesitan leer, y no modificar, los datos de determinadas cuentas. Múltiples transacciones que sólo leen la misma cuenta pueden ser procesadas en paralelo, ya que la orden de repetición no importa, proporcionando un beneficio de rendimiento.

Para identificar cuentas de solo lectura, la estructura de MessageHeader de transacción contiene `num_readonly_signed_accounts` y `num_readonly_unsigned_accounts`. La instrucción `program_ids` se incluye en el vector de la cuenta como sólo leída, cuentas sin firmar, ya que las cuentas ejecutables tampoco pueden ser modificadas durante el proceso de instrucciones.

## Gestión de tiempo de ejecución

Las reglas de procesamiento de transacciones de tiempo de ejecución deben ser actualizadas rápidamente. Los programas todavía no pueden escribir o gastar cuentas que no poseen. Pero las nuevas reglas de ejecución garantizan que las cuentas de sólo lectura no puedan ser modificadas, ni siquiera por los programas que las poseen.

Las cuentas de solo lectura tienen la siguiente propiedad:

- Acceso de sólo lectura a todos los campos de la cuenta, incluyendo los lamports (no se puede abonar ni debitar) y los datos de la cuenta

Las instrucciones que abonan, cargan o modifican la cuenta de sólo lectura fallarán.

## Optimizaciones de bloqueo de cuentas

El módulo Cuentas mantiene un seguimiento de las cuentas bloqueadas actuales en tiempo de ejecución, lo que separa las cuentas de sólo lectura de las cuentas con permisos de escritura. El bloqueo de cuenta por defecto da a una cuenta la designación "escribible" y sólo puede ser accedido por un hilo de procesamiento a la vez. Las cuentas de solo lectura están bloqueadas por un mecanismo separado, permitiendo lecturas paralelas.

Aunque aún no se han implementado, las cuentas de sólo lectura podrían almacenarse en caché en memoria y compartirse entre todos los hilos que ejecutan transacciones. Un diseño ideal mantendría esta caché mientras que una cuenta de sólo lectura es referenciada por cualquier transacción que se mueva a través del tiempo de ejecución, y liberar la caché cuando la última transacción salga del tiempo de ejecución.

Las cuentas de solo lectura también podrían pasarse al procesador como referencias, guardando una copia extra.
