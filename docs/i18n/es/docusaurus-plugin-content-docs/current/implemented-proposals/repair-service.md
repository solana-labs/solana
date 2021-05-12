---
title: Servicio de reparación
---

## Servicio de reparación

El Servicio de reparación se encarga de recuperar los fragmentos perdidos que no han podido ser entregados por los protocolos de comunicación primarios como Turbine. Se encarga de gestionar los protocolos descritos a continuación en la sección Protocolos de reparación. Se encarga de gestionar los protocolos que se describen a continuación en el apartado `Repair Protocols`.

## Desafíos:

1\) Los validadores pueden fallar al recibir fragmentos particulares debido a fallos de red

2\) Considera un escenario donde el blockstore contiene el conjunto de espacios {1, 3, 5}. Luego Blockstore recibe fragmentos para una ranura 7, donde para cada una de las ranuras b, b. parent == 6, así que la relación padre-hijo 6 -&gt; 7 se almacena en blockstore. Sin embargo, no hay forma de encadenar estas ranuras a ninguno de los bancos existentes en Blockstore, y por lo tanto el protocolo de `Reparación ` no reparará estas ranuras. Si estas ranuras forman parte de la cadena principal, se detendrá el progreso de la reproducción en este nodo.

## Primitivas relacionadas con la reparación

Ranuras de época: Cada validador anuncia por separado en gossip las distintas partes de una `Epoch Slots`:

- El `stash`: Un conjunto comprimido de una época de todas las ranuras completadas.
- El `cache`: La codificación de longitud de ejecución (RLE) de las últimas `N` ranuras completadas a partir de alguna ranura `M`, donde `N` es el número de ranuras que caben en un paquete de tamaño MTU.

`Epoch Slots` en gossip se actualizan cada vez que un validador recibe una ranura completa dentro de la época. Las ranuras completadas son detectadas por el almacén de bloques y enviadas por un canal al Servicio de Reparación. Es importante tener en cuenta que sabemos que en el momento en que una ranura `X` está completa, el calendario de épocas debe existir para la época que contiene la ranura `X` porque WindowService rechazará los fragmentos de épocas no confirmadas.

Cada `N/2` ranuras completadas, las `N/2` ranuras más antiguas se mueven de la `cache` al `stash`. El valor base `M` para el RLE también debe ser actualizado.

## Solicitud de Reparación de Protocolos

El protocolo de reparación hace los mejores intentos para progresar en la estructura de bifurcación de Blockstore.

Las diferentes estrategias de protocolo para abordar los desafíos anteriores:

1. Reparación de fragmentos \N(Dirige el desafío \N1): Es el protocolo de reparación más básico, con el objetivo de detectar y rellenar "agujeros" en el ledger. Blockstore rastrea la última ranura raíz. El Servicio de Reparación iterará periódicamente cada fork en el blockstore empezando por la ranura raíz, enviando solicitudes de reparación a los validadores para cualquier fragmento que falte. Enviará como máximo unas `N` solicitudes de reparación por iteración. La reparación de los fragmentos debe priorizar la reparación de los forks en función del peso del fork del líder. Los validadores sólo deben enviar solicitudes de reparación a los validadores que hayan marcado esa ranura como completada en sus ranuras de época. Los validadores deben priorizar la reparación de fragmentos en cada ranura que son responsables de retransmitir a través de la turbina. Los validadores pueden calcular qué fragmentos son responsables de retransmitirse porque la semilla de la turbina se basa en id de líder ranura e índice de fragmentación.

   Nota: Los validadores solo aceptarán fragmentos dentro de la época verificable actual (época en la que el validador tiene un programa líder para\).

2. Reparación preventiva de ranuras \(Desafío de direcciones \#2/):: El objetivo de este protocolo es descubrir la relación de encadenamiento de las ranuras "huérfanas" que actualmente no se encadenan a ningún fork conocido. La reparación de los fragmentos debe priorizar la reparación de las ranuras huérfanas en función del peso del fork del líder.

   - Blockstore rastreará el conjunto de espacios "huérfanos" en una familia de columnas separada.
   - El Servicio de Reparación realizará periódicamente solicitudes de `Huérfanos` para cada uno de los huérfanos del almacén de bloques.

     `Orphan(orphan)` solicita - `orphan` es la ranura huérfana de la que el solicitante quiere conocer los padres de `Orphan(orphan)` respuesta - Los fragmentos más altos para cada uno de los primeros `N` padres del `orphan` solicitado

     Al recibir las respuestas `p`, donde `p` es algún fragmento en una ranura padre, los validadores:

     - Inserta un `SlotMeta` vacío en el almacén de bloques para `p.slot` si no existe ya.
     - Si `p.slot` existe, actualiza el padre de `p` basándose en `parents`

     Nota: una vez que estas ranuras vacías se añaden al almacén de bloques, el protocolo `Shred Repair` debe intentar llenar esas ranuras.

     Nota: Los validadores sólo aceptarán respuestas que contengan fragmentos dentro de la época actual verificable \ (época para la que el validador tiene un programa líder\).

Los validadores deben intentar enviar las solicitudes de huérfanos a los validadores que hayan marcado ese huérfano como completado en sus Ranuras de época. Si no existen dichos validadores, entonces seleccione aleatoriamente un validador de una manera ponderada por el stake.

## Protocolo de respuesta a la reparación

Cuando un validador recibe una solicitud de un fragmento `S`, responde con el fragmento si lo tiene.

Cuando un validador recibe un fragmento a través de una respuesta de reparación, comprueba `EpochSlots` para ver si <= `1/3` de la red ha marcado esta ranura como completada. Si es así, vuelven a enviar este fragmento a través de su ruta de turbina asociada, pero sólo si este validador no ha retransmitido este fragmento antes.
