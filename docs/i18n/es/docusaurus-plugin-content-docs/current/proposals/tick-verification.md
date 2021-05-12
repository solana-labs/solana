---
title: Verificación de Tick
---

Este diseño los criterios y la validación de ticks en una ranura. También describe las condiciones de manejo de errores y slashing que rodean cómo el sistema gestiona transmisiones que no cumplen estos requisitos.

# Estructura de la ranura

Cada ranura debe contener un número esperado de `ticks_per_slot` ticks. El último fragmento en una ranura debe contener sólo la totalidad del último tick, y nada más. El líder también debe marcar este fragmento que contiene el último tick con la bandera `LAST_SHRED_IN_SLOT`. Entre ticks, debe haber `hashes_per_tick` número de hashes.

# Manejar las malas transmisiones

Las transmisiones maliciosas `T` se gestionan de dos maneras:

1. Si un líder puede generar alguna transmisión errónea `T` y también alguna transmisión alternativa `T'` para la misma ranura sin violar ninguna regla de slashing para transmisiones duplicadas (por ejemplo si `T'` es un subconjunto de `T`), entonces el cluster debe manejar la posibilidad de que ambas transmisiones sean en vivo.

Esto significa que no podemos marcar la transmisión errónea `T` como muerta porque el grupo puede haber alcanzado un consenso sobre `T'`. Estos casos necesitan una prueba de slashing para castigar este mal comportamiento.

2. De lo contrario, podemos simplemente marcar la ranura como muerta y no jugable. Una prueba de slashing puede o no ser necesaria dependiendo de la factibilidad.

# Blockstore recibiendo fragmentos

Cuando la blockstore recibe un nuevo fragmento `s`, hay dos casos:

1. `s` está marcada como `Última_Red_EN_SLOT`, entonces comprueba si existe un shred `s'` en el almacén de bloques para esa ranura donde `s'.index > s.index` Si es así, juntos `s` y `s'` constituyen una prueba de slashing.

2. Blockstore ya ha recibido un `'s` marcado como `LAST_SHRED_IN_SLOT` con índice `i`. Si `s.index > i`, juntas `s` y ``constituyen una prueba de slashing. En este caso, blockstore tampoco insertará `s`.

3. Se ignoran los fragmentos duplicados del mismo índice. Los fragmentos no duplicados para el mismo índice son una condición de slash. Los detalles para este caso están cubiertos en la sección `Bloque Duplicado Líder Slashing`.

# Reproduciendo y validando ticks

1. La repetición de la etapa reproduce entradas desde el blockstore, manteniendo un seguimiento del número de ticks que ha visto por ranura, y verificando que hay un `hashes_per_tick` número de hash entre ticcks. Una vez que se ha reproducido el tick de este último fragmento, la etapa de repetición comprueba el número total de ticks.

Fallo del escenario 1: ¡Si alguna vez hay dos ticks consecutivos entre los cuales el número de hash es `! hashes_per_tick`, marque esta ranura como muerta.

Fallo escenario 2: Si el número de ticks != `ticks_per_slot`, marque la ranura como muerta.

Fallo del escenario 3: Si el número de ticks alcanza `ticks_per_slot`, pero todavía no hemos visto el `LAST_SHRED_IN_SLOT`, marque esta ranura como muerta.

2. Cuando ReplayStage alcanza un fragmento marcado como el último fragmento, comprueba si este último fragmento es un tick.

Escenario de fallo: Si el fragmento firmado con la bandera `LAST_SHRED_IN_SLOT` no puede ser deserializado en un tick (o bien falla en la deserialización o se deserializa en una entrada), marca este slot como muerto.
