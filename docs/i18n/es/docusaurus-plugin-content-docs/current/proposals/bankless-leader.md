---
title: Líder sin banco
---

Un líder sin banco realiza la cantidad mínima de trabajo para producir un bloque válido. El líder tiene la tarea de realizar transacciones de progreso, ordenar y filtrar transacciones válidas, organizarlas en entradas, desfragmentar las entradas y transmitir las trozos. Mientras que un validador sólo necesita volver a mostrar el bloque y la ejecución de entradas bien formadas. El líder realiza 3 veces más operaciones de memoria antes de cualquier ejecución bancaria que el validador por transacción procesada.

## Racional

La operación de banco normal para un gasto tiene que hacer 2 cargas y 2 tiendas. Con este líder de diseño sólo hace 1 carga. así account_db trabaja 4 veces menos antes de generar el bloque. Es probable que las operaciones de la tienda sean más caras que las lecturas.

Cuando la etapa de repetición comienza a procesar las mismas transacciones, puede asumir que PoH es válido, y que todas las entradas son seguras para la ejecución paralela. Las cuentas de comisión que han sido cargadas para producir el bloque probablemente todavía estén en la memoria, por lo que la carga adicional debe ser caliente y el coste es probable que se amortice.

## Cuenta de comisión

La cuenta de [comisión](../terminology.md#fee_account) paga por la transacción que se incluirá en el bloque. El líder sólo tiene que validar que la cuenta de comisión tiene el saldo para pagar la comisión.

## Caché de balance

Durante la duración de los bloques consecutivos de líderes, el líder mantiene un caché de saldo temporal para todas las cuentas de comisiones procesadas. El caché es un mapa de las pubkeys a los lamports.

Al inicio del primer bloque la caché del balance está vacía. Al final del último bloque se destruye la caché.

Las búsquedas de caché de balance deben hacer referencia al mismo fork base durante toda la duración de la caché. En el límite de bloques, la caché puede reiniciarse junto con el fork base después de que la etapa de repetición termine verificando el bloque anterior.

## Verificación de saldo

Antes de la comprobación de saldo, el líder valida todas las firmas en la transacción.

1. Verifique que las cuentas no están en uso y BlockHash es válido.
2. Comprueba si la cuenta de comisiones está presente en la caché, o carga la cuenta desde accounts_db y almacena el saldo de lamport en la caché.
3. Si el saldo es inferior a la tasa, abandona la transacción.
4. Resta la cuota del saldo.
5. Para todas las claves de la transacción que son de débito de crédito y son referenciadas por una instrucción, reduce su saldo a 0 en la caché. El cargo por cuenta se declara como débito de crédito, pero siempre y cuando no se utilice en ninguna instrucción, su saldo no se reducirá a 0.

## Repetición de líder

Los líderes tendrán que volver a reproducir sus bloques como parte de la operación estándar de repetición del escenario.

## Repetición del líder con bloques consecutivos

Un líder puede ser programado para producir múltiples bloques seguidos. En ese escenario es probable que el líder esté produciendo el siguiente bloque mientras se está jugando la etapa de repetición del primer bloque.

Cuando el líder finaliza la etapa de repetición puede reiniciar la caché de saldo borrándola, y establecer un nuevo fork como base para la caché que puede activarse en el siguiente bloque.

## Reiniciando el caché de balance

1. Al inicio del bloque, si la caché de saldo no está inicializada, establece que el fork base para la caché de saldo sea el padre del bloque y crea una caché vacía.
2. si la caché está inicializada, compruebe si los padres del bloque tienen un nuevo banco congelado que es más nuevo que el fork base actual para la caché del balance.
3. si existe un fork padre más nuevo que el fork base del caché, reinicie la caché al padre.

## Impacto en los clientes

La misma cuenta de comisión puede ser reutilizada muchas veces en el mismo bloque hasta que sea utilizada una vez como crédito débito por una instrucción.

Los clientes que transmitan un gran número de transacciones por segundo deben utilizar una cuenta de comisión dedicada que no se utiliza como débito de crédito en ninguna instrucción.

Una vez que una comisión de cuenta se utiliza como Crédito-Debit, fallará la comprobación del saldo hasta que la caché del saldo se reinicie.
