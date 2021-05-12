---
title: Confirmación del bloque
---

Un validador vota un hash PoH por dos propósitos. En primer lugar, la votación indica que cree que el ledger es válido hasta ese momento. En segundo lugar, puesto que muchos forks válidas pueden existir a una altura determinada, el voto también indica soporte exclusivo para el fork. Este documento describe solamente el primero. Este último está descrito en [Tower BFT](../implemented-proposals/tower-bft.md).

## Diseño Actual

Para empezar a votar, un validador primero registra una cuenta a la que enviará sus votos. A continuación, envía votos a esa cuenta. El voto contiene la altura del tick del bloque en el que vota. La cuenta almacena las 32 alturas más altas.

### Problemas

- Sólo el validador sabe cómo encontrar sus propios votos directamente.

  Otros componentes, como el que calcula el tiempo de confirmación, deben hornearse en el código del validador. El código del validador consulta al banco para todas las cuentas que son propiedad del programa de votación.

- Las boletas de votación no contienen un hash PoH. El validador sólo vota que ha observado un bloque arbitrario a cierta altura.

- Las boletas de votación no contienen un hash del estado bancario. Sin ese hash, no hay evidencia de que el validador ejecutó las transacciones y verificó que no hubo doble gasto.

## Diseño propuesto

### No hay bloque cruzado de estado inicialmente

En este momento se produce un bloque, el líder añadirá una transacción de NewBlock al libro principal con un número de tokens que representan la recompensa de validación. Es efectivamente una transacción multifirma incremental que envía tokens desde el fondo de minería a los validadores. La cuenta debe asignar suficiente espacio para recolectar los votos necesarios para obtener una supermayoría. Cuando un validador observa la transacción de NewBlock tiene la opción de someter un voto que incluye un hash de su estado principal (el estado bancario). Una vez que la cuenta tenga votos suficientes, el programa de votación debe dispersar los tokens a los validadores, lo que hace que la cuenta sea eliminada.

#### Hora de confirmación de registro

El banco tendrá que estar al tanto del programa de votación. Después de cada transacción, debe verificar si es una transacción de voto y si es así, compruebe el estado de esa cuenta. Si la transacción causó que se obtuviera la supermayoría, debería registrar el tiempo desde que se envió la transacción de NewBlock.

### Finalidad y Pagos

[Tower BFT](../implemented-proposals/tower-bft.md) es el algoritmo de selección de fork propuesto. Propone que el pago a los mineros sea pospuesto hasta que el _stack_ de votos del validador alcance una cierta profundidad, en cuyo punto no es económicamente factible retroceso. El programa de votación puede, por lo tanto, implementar Tower BFT. Las instrucciones de voto necesitarían hacer referencia a una cuenta global de la torre para que pueda rastrear el estado entre bloques.

## Desafíos

### Votación en cadena

Usar programas y cuentas para implementar esto es un poco tedioso. La parte más difícil es averiguar cuánto espacio asignar en NewBlock. Las dos variables son el _conjunto activo_ y el stake de esos validadores. Si calculamos el activo en el momento de enviar NewBlock, el número de validadores para asignar espacio es conocido de antemano. Sin embargo, si permitimos que nuevos validadores voten bloques antiguos, entonces necesitaríamos una forma de asignar espacio dinámicamente.

De forma similar, si el líder almacena en caché los stake en el momento del Nuevo Bloque, el programa de votación no necesita interactuar con el banco cuando procesa los votos. Sino lo hacemos, entonces tenemos la opción de permitir que los stake floten hasta que una votación sea enviada. Un validador podría hacer referencia a su propia cuenta de stake, pero ese sería el valor de la cuenta corriente en lugar del valor de la cuenta del estado financiero finalizado más recientemente. Actualmente el banco no ofrece un medio para referenciar cuentas desde determinados puntos a tiempo.

### Implicaciones de votación en bloques anteriores

¿Un voto en una altura implica un voto en todos los bloques de alturas inferiores de ese fork? Si lo hace, necesitaremos una forma de buscar las cuentas de todos los bloques que aún no han alcanzado la supermayoría. Si no, el validador podría enviar votos a todos los bloques explícitamente para obtener las recompensas del bloque.
