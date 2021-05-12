---
title: Blockstore
---

Después de que un bloque llega a la finalidad, todos los bloques desde aquel hasta el bloque génesis forman una cadena lineal con el conocido nombre blockchain. Hasta ese punto, sin embargo, el validador debe mantener todas las cadenas potencialmente válidas, llamadas _forks_. El proceso por el cual se forman naturalmente forks como resultado de la rotación de líderes se describe en [generación de forks](../cluster/fork-generation.md). La estructura de datos _blockstore_ descrita aquí es la forma en que un validador hace frente a esos forks hasta que los bloques son finalizados.

El blockstore permite a un validador registrar cada trozo que observa en la red, en cualquier orden, siempre y cuando el pedazo esté firmado por el líder esperado para una rama determinada.

Los fragmentos se trasladan a un espacio de claves que se puede forzar la tupla de `leader slot` + `shred index`\(dentro de la ranura). Esto permite que la estructura de lista salteada del protocolo Solana se almacene en su totalidad, sin a-priori eligiendo qué fork seguir, qué Entradas persistir o cuándo persistir en ellas.

Las solicitudes de reparación de fragmentos recientes se sirven de la memoria RAM o de los archivos recientes y del almacenamiento más profundo para los fragmentos menos recientes, según lo implementado por el almacén que respalda a Blockstore.

## Funcionalidades de Blockstore

1. Persistencia: el Blockstore vive en la parte frontal de la verificación de nodos

   pipeline, justo detrás de la red recibir y la verificación de firmas. Si el

   shred recibido es coherente con el horario del líder \(es decir, fue firmado por el

   líder para la ranura indicada\), se almacena inmediatamente.

2. Reparación: la reparación es la misma que la reparación de ventanas arriba, pero capaz de servir cualquier

   shred que ha sido recibido. Blockstore almacena fragmentos con firmas,

   preservando la cadena de origen.

3. Forks: Blockstore soporta acceso aleatorio de shreds, así que puede soportar la

   necesidad del validador de dar marcha atrás y repetir desde un punto de control bancario.

4. Reinicio: con una poda/eliminación adecuada, el almacén de bloques puede reproducirse mediante la

   enumeración ordenada de las entradas desde la ranura 0. La lógica de la etapa de repetición

   /(es decir, el tratamiento de los forks) tendrá que ser utilizada para las entradas más recientes en

   el Blockstore.

## Diseño de Blockstore

1. Las entradas en el Blockstore se almacenan como pares clave-valor, donde la clave es el índice de ranura concatenado y el índice de fragmentación de una entrada, y el valor son los datos de la entrada. Tenga en cuenta que los índices de shred están basados en cero para cada ranura (es decir, son relativos a la ranura).
2. El Blockstore mantiene metadatos para cada ranura, en la estructura de `SlotMeta` que contiene:

   - `slot_index` - El índice de esta ranura
   - `num_blocks` - El número de bloques en la ranura \(usado para encadenar a una ranura anterior\)
   - `consumed` - El índice más alto `n`, tal que para todos `m < n`, hay un shred en esta ranura con el índice de fragmentación igual a `n` \(i.. el índice más alto de fragmentación consecutiva\).
   - `received` - El índice más alto recibido para la ranura
   - `next_slots` - Una lista de futuras ranuras a las que esta ranura podría encadenar. Utilizado al reconstruir

     elledger para encontrar posibles puntos de forks.

   - `last_index` - El índice del shred que está marcado como el último shred de esta ranura. Esta bandera en un pedazo será fijada por el líder para una ranura cuando estén transmitiendo la última ranura para una ranura.
   - `is_rooted` - Verdadero si cada bloque de 0...ranura forma una secuencia completa sin agujeros. Podemos derivar is_rooted para cada ranura con las siguientes reglas. Sea slot\(n\) el slot con índice `n`, y slot\(n\).is_full\(\) es verdadero si el slot con índice `n` tiene todos los ticks esperados para ese slot. Sea is_rooted(n\) la afirmación de que "la ranura(n\).is_rooted es verdadera". Entonces:

     is_rooted(0\) is_rooted(n+1\) iff \(is_rooted(n\) y slot(n\).is_full(\)

3. Encadenamiento - Cuando llega un fragmento para una nueva ranura `x`, comprobamos el número de bloques \ (`num_blocks`) para esa nueva ranura \ (esta información está codificada en el fragmento). Entonces sabemos que esta nueva ranura se encadena con la ranura `x - num_blocks`.
4. Suscripciones - El Blockstore registra un conjunto de ranuras a las que se ha "suscrito". Esto significa que las entradas de la cadena a estas ranuras serán enviadas en el canal Blockstore para su consumo por la ReplayStage. Consulte las `Blockstore APIs` para más detalles.
5. Actualizar notificaciones - Blockstore notifica a los oyentes cuando slot\(n\).is_rooted es volteado de falso a verdadero para cualquier `n`.

## APIs de Blockstore

El Blockstore ofrece una API basada en suscripción que ReplayStage utiliza para pedir entradas que le interesen. Las entradas se enviarán en un canal expuesto por el Blockstore. Estas API de suscripción son las siguientes: 1. `fn get_slots_.Ue(slot_indexes: &[u64]) -> Vec<SlotMeta>`: Devuelve nuevas ranuras conectadas a cualquier elemento de la lista `slot_indexes`.

1. `fn get_slot_entries(slot_index: u64, entry_start_index: usize, max_entries: Option<u64>) -> Vec<Entry>`: Devuelve el vector de entradas para la ranura que comienza con `entry_start_index`, limitando el resultado a `max` si `max_entries == Some(max)`, de lo contrario, no se impone ningún límite superior en la longitud del vector de retorno.

Nota: De forma acumulativa, esto significa que la etapa de repetición tendrá que saber cuándo ha terminado una ranura, y suscribirse a la siguiente ranura que le interese para obtener el siguiente conjunto de entradas. Anteriormente, la carga de las ranuras encadenadas recaía sobre el Blockstore.

## Interfaz con el banco

El banco expone a la fase de repetición:

1. `prev_hash`: en qué cadena PoH está trabajando como indica el hash del último

   entrada procesada

2. `tick_height`: los ticks de la cadena PoH que están siendo verificados por este

   banco

3. `votes`: una pila de registros que contienen: 1. `prev_hashes`: a lo que cualquier cosa después de esta votación debe encadenar en PoH 2. `tick_height`: la altura de tick a la que se emitió este voto 3. `lockout period`: cuánto tiempo debe observarse una cadena en el ledger para

   poder encadenarse por debajo de este voto

La etapa de repetición utiliza las APIs de Blockstore para encontrar la cadena de entradas más larga que pueda colgar de una votación anterior. Si esa cadena de entradas no cuelga de la última votación, la etapa de repetición hace retroceder el banco hasta esa votación y repite la cadena desde ahí.

## Limpiando Blockstore

Una vez que las entradas del Blockstore son lo suficientemente antiguas, la representación de todas los posibles forks se vuelve menos útiles, y tal vez incluso problemáticos para la repetición al reiniciar. Sin embargo, una vez que los votos de un validador han alcanzado el bloqueo máximo, cualquier contenido del Blockstore que no esté en la cadena del PoH para ese voto puede ser podado, expurgado.
