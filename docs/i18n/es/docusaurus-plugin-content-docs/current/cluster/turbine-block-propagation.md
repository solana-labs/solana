---
title: Propagación del bloque turbina
---

Un clúster Solana utiliza un mecanismo de propagación de bloques de varias capas llamado _Turbina_ para transmitir las transacciones a todos los nodos con una cantidad mínima de mensajes duplicados. El clúster se divide en pequeñas colecciones de nodos, llamadas _vecindades_. Cada nodo es responsable de compartir los datos que recibe con los demás nodos de su vecindario, así como de propagar los datos a un pequeño conjunto de nodos de otros vecindarios. De esta manera, cada nodo sólo tiene que comunicarse con un pequeño número de nodos.

Durante su ranura, el nodo líder distribuye fragmentos entre los nodos validadores en el primer vecindario \\(capa 0\\). Cada validador comparte sus datos dentro de su vecindad, pero también retransmite los fragmentos a un nodo en algunas vecindades en la siguiente capa \ (capa 1\). Cada uno de los nodos de la capa 1 comparte sus datos con sus pares de la vecindad, y retransmite a los nodos de la siguiente capa, etc., hasta que todos los nodos del clúster hayan recibido todos los fragmentos.

## Asignación de vecindarios - Selección ponderada

Para que el fanout del plano de datos funcione, todo el cluster debe estar de acuerdo en cómo se divide el cluster en vecindades. Para ello, todos los nodos validadores reconocidos (los pares de la TVU) se ordenan por stake y se almacenan en una lista. A continuación, esta lista se indexa de diferentes maneras para averiguar los límites del vecindario y retransmitir a su compañeros. Por ejemplo, el líder simplemente seleccionará los primeros nodos que componen la capa 0. Estos serán automáticamente los poseedores del mayor stake, permitiendo que los votos más pesados regresen primero al líder. Los nodos de capa 0 y capa inferiores utilizan la misma lógica para encontrar a sus vecinos y a sus pares de capa siguiente.

Para reducir la posibilidad de los vectores de ataque, cada raya se transmite sobre un árbol aleatorio de vecinos. Cada nodo usa el mismo conjunto de nodos que representan el cluster. Se genera un árbol aleatorio a partir del conjunto para cada fragmento utilizando una semilla derivada de la identificación del líder, la ranura y el índice del fragmento.

## Estructura de las capas y las vecindades

El líder actual realiza sus difusiones iniciales a un máximo de nodos `DATA_PLANE_FANOUT`. Si esta capa 0 es menor que el número de nodos del clúster, el mecanismo de fanout del plano de datos añade capas por debajo debajo. Las capas subsiguientes siguen estas restricciones para determinar la capacidad de las capas: Cada vecindario contiene nodos `DATA_PLANE_FANOUT`. La capa 0 comienza con 1 vecindad con nodos de fanout. El número de nodos en cada capa adicional crece por un factor de fanout.

Como se ha mencionado anteriormente, cada nodo de una capa sólo tiene que difundir sus fragmentos a sus vecinos y a exactamente 1 nodo en algunos vecindarios de la siguiente capa, en lugar de a todos los compañeros de la TVU en el clúster. Una buena manera de pensar en esto es, la capa 0 comienza con 1 vecindario con nodos fanout, la capa 1 añade vecindarios fanout, cada uno con nodos fanout y la capa 2 tendrá `fanout * número de nodos en la capa 1` y así sucesivamente.

De esta manera, cada nodo sólo tiene que comunicarse con un máximo de `2 * DATA_PLANE_FANOUT - 1` nodos.

El siguiente diagrama muestra cómo el Líder envía fragmentos con un fanout de 2 al vecindario 0 en la Capa 0 y cómo los nodos en el vecindario 0 comparten sus datos entre sí.

![El líder envía fragmentos al vecindario 0 en la capa 0](/img/data-plane-seeding.svg)

El siguiente diagrama muestra cómo el vecindario 0 se abre a los vecindarios 1 y 2.

![Vecindario 0 Fanout a Vecindario 1 y 2](/img/data-plane-fanout.svg)

Finalmente, el siguiente diagrama muestra un clúster de dos capas con un fanout de 2.

![Clúster de dos capas con un Fanout de 2](/img/data-plane.svg)

### Valores de Configuración

`DATA_PLANE_FANOUT` - Determina el tamaño de la capa 0. Las capas posteriores crecen en un factor de `DATA_PLANE_FANOUT`. El número de nodos en un vecindario es igual al valor de fanout. Los vecindarios se llenarán hasta su capacidad antes de que se añadan otros nuevos, es decir, si un vecindario no está lleno, _debe_ ser el último.

Actualmente, la configuración se establece cuando se lanza el clúster. En el futuro, estos parámetros podrán alojarse en la cadena, lo que permitirá modificarlos sobre la marcha cuando cambie el tamaño del cluster.

## Cálculo de la tasa FEC necesaria

La turbina depende de la retransmisión de paquetes entre validadores. Debido a la retransmisión, cualquier pérdida de paquetes en la red se agrava, y la probabilidad de que el paquete no llegue a su destino aumenta en cada salto. La tasa FEC debe tener en cuenta la pérdida de paquetes en toda la red y la profundidad de propagación.

Un grupo shred es el conjunto de paquetes de datos y codificación que pueden utilizarse para reconstruirse mutuamente. Cada grupo shred tiene una probabilidad de fallo, basada en la probabilidad de que el número de paquetes que fallen supere la tasa FEC. Si un validador falla al reconstruir el grupo shred, entonces el bloque no puede ser reconstruido, y el validador tiene que depender de la reparación para arreglar los bloques.

La probabilidad de que el grupo shred falle puede calcularse usando la distribución binomial. Si la tasa de FEC es `16:4`, entonces el tamaño del grupo es 20, y al menos 4 de los fragmentos deben fallar para que el grupo falle. Que es igual a la suma de la probabilidad de que fallen 4 o más pistas de 20.

Probabilidad de que un bloque tenga éxito en la turbina:

- Probabilidad del fallo del paquete: `P = 1 - (1 - network_packet_loss_rate)^2`
- Tasa de FEC: `K:M`
- Número de pruebas: `N = K + M`
- Tasa de fallo del grupo Shred: `S = SUM de i=0 -> M para binomio (prob_failure = P, pruebas = N, fallas = i)`
- Fragmentos por bloque: `G`
- Tasa de éxito del bloque: `B = (1 - S) ^ (G / N)`
- La distribución binomial para resultados exactamente `i` con probabilidad de P en N ensayos se define como `(N elige i) * P^i * (1 - P)^(N-i)`

Por ejemplo:

- La tasa de pérdida de paquetes de red es del 15%.
- La red de 50k tps genera 6400 shreds por segundo.
- La tasa del FEC aumenta el total de fragmentos por bloque por la proporción del FEC.

Con una tasa de FEC: `16:4`

- `G = 8000`
- `P = 1 - 0.85 * 0.85 = 1 - 0.7225 = 0.2775`
- `S = SUM de i=0 -> 4 para binomio (prob_failure = 0.2775, pruebas = 20, fallas = i) = 0.689414`
- `B = (1 - 0.689) ^ (8000 / 20) = 10^-203`

Con una tasa de FEC: `16:16`

- `G = 12800`
- `S = SUM de i=0 -> 32 para binomio (prob_failure = 0.2775, pruebas = 64, fallas = i) = 0.002132`
- `B = (1 - 0.002132) ^ (12800 / 32) = 0.42583`

Con una tasa de FEC: `32:32`

- `G = 12800`
- `S = SUM de i=0 -> 32 para binomio (prob_failure = 0.2775, pruebas = 64, fallas = i) = 0.000048`
- `B = (1 - 0.000048) ^ (12800 / 64) = 0.99045`

## Vecindarios

El siguiente diagrama muestra cómo interactúan dos vecindarios en diferentes capas. Para entorpecer un vecindario, suficientes nodos \\(códigos de borrado +1\\) del vecindario anterior necesitan fallar. Dado que cada vecindario recibe fragmentos de varios nodos de un vecindario de la capa superior, necesitaríamos un gran fallo de la red en las capas superiores para acabar con datos incompletos.

![El funcionamiento interno de un vecindario](/img/data-plane-neighborhood.svg)
