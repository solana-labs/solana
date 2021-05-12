---
title: Generación de bifurcación
---

Esta sección describe cómo ocurren naturalmente los bifurcaciones como consecuencia de [rotación del líder](leader-rotation.md).

## Resumen

Los nodos se turnan siendo líderes y generando el PoH que codifica los cambios de estado. El clúster puede tolerar la pérdida de conexión con cualquier líder sintetizando lo que el líder _**podría**_ haber generado si se hubiera conectado pero sin ingerir ningún cambio de estado. Por tanto, el número posible de bifurcaciones se limita a una lista de omisión de bifurcaciones "allí / no allí" que pueden surgir en los límites de las ranuras de rotación del líder. En cualquier espacio dado, solo se aceptarán las transacciones de un único líder.

## Flujo de mensajes

1. Las transacciones son ingeridas por el líder actual.
2. Filtros de líder para transacciones válidas.
3. El líder ejecuta transacciones válidas actualizando su estado.
4. Los paquetes líderes transaccionan en entradas basadas en su rama actual de PoH.
5. El líder transmite las entradas a los nodos de validación \ (en fragmentos firmados \)
   1. El flujo de PoH incluye ticks; entradas vacías que indican la vida del líder y el paso del tiempo en el cluster.
   2. La secuencia de un líder comienza con las entradas de tick necesarias para completar PoH de regreso al espacio de líder anterior observado más recientemente del líder.
6. Los validadores retransmiten las entradas a los pares de su conjunto y a los nodos posteriores.
7. Los validadores validan las transacciones y las ejecutan en su estado.
8. Los validadores calculan el hash del estado.
9. En momentos específicos, es decir, recuentos de ticks PoH específicos, los validadores transmiten votos al líder.
   1. Los votos son firmas del hash del estado calculado en ese recuento de ticks de PoH.
   2. Los votos también se propagan a través de chismes.
10. El líder ejecuta los votos, al igual que cualquier otra transacción, y los transmite al cluster.
11. Los validadores observan sus votos y todos los votos del cluster.

## Particiones, bifurcaciones

Los forks pueden surgir en conteos de ticks de PoH que correspondan a un voto. El próximo líder puede no haber observado la última franja horaria de votación y puede empezar su ranura con entradas virtuales generadas de PoH. Estos ticks vacíos son generados por todos los nodos en el clúster a una velocidad configurada por clúster para hashes / per / tick ` Z `.

Solo hay dos versiones posibles de PoH durante un intervalo de votación: PoH con ticks ` T ` y entradas generadas por el líder actual, o PoH con solo ticks. La versión "sólo ticks" del PoH puede ser considerada como un contador virtual, uno que todos los nodos en el clúster pueden derivar del último tick en la ranura anterior.

Los validadores pueden ignorar bifurcaciones en otros puntos \(por ejemplo, desde el líder equivocado\), o cortar al líder responsable de la bifurcación.

Los validadores votan basándose en una elección codiciosa para maximizar su recompensa descrita en [ Tower BFT ](../implemented-proposals/tower-bft.md).

### Vista del validador

#### Progresión de Tiempo

El siguiente diagrama representa la vista del validador del flujo de PoH con posibles bifurcaciones a lo largo del tiempo. L1, L2, etc. son ranuras de líderes, y `E`s representan entradas de ese líder durante la ranura de ese líder. Los `x`s representan ticks solamente, y el tiempo fluye hacia abajo en el diagrama.

![Generación de bifurcación](/img/fork-generation.svg)

Tenga en cuenta que un ` E ` que aparece en 2 bifurcaciones en la misma ranura es una condición que se puede reducir, por lo que un validador que observe ` E3 ` y ` E3 '` puede reducir L3 y elija con seguridad ` x ` para ese espacio. Una vez que un validador se compromete con una bifurcación, otras bifurcaciones se pueden descartar por debajo de ese recuento de ticks. Para cualquier ranura, los validadores solo necesitan considerar una sola cadena "tiene entradas" o una cadena "solo ticks" para ser propuesta por un líder. Pero es posible que varias entradas virtuales se superpongan ya que se vinculan con la ranura anterior.

#### División de tiempo

Es útil considerar la rotación del líder sobre el recuento de ticks de PoH como una división de tiempo del trabajo de codificación del estado del clúster. La siguiente tabla presenta el árbol anterior de bifurcaciones como un contador dividido en tiempo.

| leader slot      | L1 | L2 | L3 | L4 | L5 |
|:---------------- |:-- |:-- |:-- |:-- |:-- |
| data             | E1 | E2 | E3 | E4 | E5 |
| ticks since prev |    |    |    | x  | xx |

Tenga en cuenta que sólo los datos del líder L3 serán aceptados durante la ranura de líder L3. Los datos de L3 pueden incluir tics de "recuperación" de regreso a una ranura que no sea L2 si L3 no observó los datos de L2. Las transmisiones de L4 y L5 incluyen las entradas de PoH "ticks to prev".

Esta disposición de los flujos de datos de la red permite a los nodos guardar exactamente esto en el libro mayor para reproducir, reiniciar y puntos de control.

### Vista del líder

Cuando un nuevo líder comienza una ranura, primero debe transmitir cualquier PoH \(ticks\) necesario para enlazar la nueva ranura con la ranura más recientemente observada y votada. La bifurcación que propone el líder vincularía la ranura actual a una bifurcación anterior que el líder ha votado con ticks virtuales.
