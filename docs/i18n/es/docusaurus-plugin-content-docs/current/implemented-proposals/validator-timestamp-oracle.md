---
title: Validador Timestamp Oracle
---

Los usuarios externos de Solana a veces necesitan saber la hora en que se produjo un bloque en el mundo real, generalmente para cumplir con los requisitos de los auditores externos o la aplicación de la ley. Esta propuesta describe un oráculo de marca de tiempo validador que permitiría a un clúster Solana satisfacer esta necesidad.

El esquema general de la aplicación propuesta es el siguiente:

- En intervalos regulares, cada validador registra el tiempo observado para una ranura conocida en cadena (a través de una marca de tiempo añadida a una ranura de voto)
- Un cliente puede solicitar un tiempo de bloque para un bloque root usando el método RPC `getBlockTime`. Cuando un cliente solicita una marca de tiempo para el bloque N:

  1. Un validador determina una marca de tiempo de un "cluster" para una ranura reciente con marca de tiempo antes del bloque N, observando todas las instrucciones de Voto con marca de tiempo registradas en el ledger que hace referencia a esa ranura, y determinando la marca de tiempo media ponderada por el stake.

  2. Esta marca de tiempo media reciente se utiliza entonces para calcular la marca de tiempo del bloque N de usando la duración establecida en el cluster

Requisitos:

- Cualquier validador que reproduzca el ledger en el futuro debe obtener la misma hora para cada bloque desde el génesis
- Los tiempos de bloque estimados no deben deslizar más de una hora antes de resolver a datos (oráculos) del mundo real
- Los tiempos de bloque no son controlados por un único oráculo centralizado, pero idealmente basado en una función que utiliza entradas de todos los validadores
- Cada validador debe mantener un oráculo de marca de tiempo

La misma implementación puede proporcionar una estimación de marca de tiempo para un bloque aún no arraigado. Sin embargo, debido a que la ranura con la marca de tiempo más reciente puede o no estar arraigada todavía, esta marca de tiempo sería inestable (fallando potencialmente al requisito 1). La implementación inicial se centrará en los bloques arraigados, pero si hay un caso de uso para el sellado de tiempo de los bloques recientes, será trivial añadir las apis RPC en el futuro.

## Tiempo de grabación

A intervalos regulares mientras se vota en una ranura particular, cada validador registra su tiempo observado incluyendo una marca de tiempo en su envío de instrucciones de voto. La ranura correspondiente a la marca de tiempo es la ranura más nueva del vector de votos (`Vote::slots.iter().max()`). Está firmado por el keypair de identidad del validador como un Voto habitual. Para habilitar este informe, la estructura Vote necesita ser extendida para incluir un campo timestamp, `timestamp: Option<UnixTimestamp>`, que se establecerá como`None` en la mayoría de los votos.

A partir de https://github.com/solana-labs/solana/pull/10630, los validadores presentan una marca de tiempo en cada voto. Esto permite la implementación de un servicio de almacenamiento en caché de la hora del bloque que permite a los nodos calcular la marca de tiempo estimada inmediatamente después de que el bloque se arraigue, y almacenar en caché ese valor en Blockstore. Esto proporciona datos persistentes y consultas rápidas, mientras que el requisito 1) sigue siendo el anterior.

### Votar cuentas

La cuenta de voto de un validador llevará su marca de tiempo más reciente en VoteState.

### Programa de votos

El programa de Voto en cadena necesita ser extendido para procesar una marca de tiempo enviada con una instrucción de voto de validadores. Además de su funcionalidad actual de process_vote (incluyendo la carga de la cuenta de voto correcta y la verificación de que el firmante de transacción es el validador esperado), este proceso necesita comparar la marca de tiempo y la ranura correspondiente con los valores almacenados actualmente para verificar que ambos están aumentando monótonamente, y almacenar la nueva ranura y la marca de tiempo en la cuenta.

## Cálculo de la media ponderada del Stake

Para calcular la marca de tiempo estimada para un bloque en particular, un validador primero necesita identificar la ranura timestamped más recientemente:

```text
let timestamp_slot = floor(current_slot / timestamp_interval);
```

Entonces el validador necesita reunir todas las transacciones de Vote WithTimestamp del ledger que hace referencia a esa ranura, usando `Blockstore::get_slot_entries()`. Como estas transacciones podrían haber tardado algún tiempo en llegar y ser procesadas por el líder, el validador necesita escanear varios bloques completados después del timestamp_slot para obtener un conjunto razonable de Timestamps. El número exacto de ranuras tendrá que ser ajustado: Más ranuras permitirán una mayor participación del clúster y más puntos de datos de las marcas de tiempo; menos ranuras acelerarán el tiempo de filtrado de las marcas de tiempo.

A partir de esta colección de transacciones, el validador calcula la marca de tiempo media ponderada por los stake, con referencias cruzadas a lo stakes de época de `staking_utils::staked_nodes_at_epoch()`.

Cualquier validador que vuelva a reproducir el ledger debe derivar la misma marca de tiempo ponderada por la apuesta procesando las transacciones de la marca de tiempo del mismo número de ranuras.

## Cálculo del tiempo estimado para un bloque concreto

Una vez calculada la marca de tiempo media para una ranura conocida, es trivial calcular la marca de tiempo estimada para el siguiente bloque N:

```text
let block_n_timestamp = mean_timestamp + (block_n_slot_offset * slot_duration);
```

donde `block_n_slot_offset` es la diferencia entre la ranura del bloque N y el timestamp_slot, y `slot_duration` se deriva del `slots_per_year` del cluster almacenado en cada Banco
