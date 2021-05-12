---
title: Transmisión de voto fiable
---

Los votos de validadores son mensajes que tienen una función crítica para el consenso y el funcionamiento continuo de la red. Por lo tanto, es fundamental que se entreguen y codifiquen de forma fiable en el ledger.

## Desafíos

1. La rotación del líder es activada por el PoH, que es un reloj con alta deriva. Es probable que muchos nodos tengan una visión incorrecta si el próximo líder está activo en tiempo real o no.
2. El próximo líder puede ser fácilmente inundado. Por lo tanto, un DDOS no sólo impediría la entrega de transacciones regulares, sino también mensajes de consenso.
3. UDP no es confiable, y nuestro protocolo asíncrono requiere que cualquier mensaje que se transmita sea retransmitido hasta que sea observado en el contador. La retransmisión podría causar potencialmente un _rebote involuntario_ contra el líder con un gran número de validadores. El peor caso de inundación sería `(num_nodos * num_retransmisiones)`.
4. El seguimiento de si el voto se ha transmitido o no a través del ledger no garantiza que aparezca en un bloque confirmado. El bloque observado actual puede ser desenrollado. Los validadores tendrían que mantener el estado de cada voto y fork.

## Diseño

1. Envía los votos como un mensaje push a través de los gossip. Esto garantiza la entrega del voto a todos los próximos líderes, no sólo al próximo futuro.
2. Los líderes leerán la tabla Crds para nuevos votos y codificarán los nuevos votos recibidos en los bloques que proponen. Esto permite que los votos de los validadores sean incluidos en los forks de cancelación por todos los futuros líderes.
3. Los validadores que reciban votos en el ledger los agregarán a su tabla local de crds, no como una solicitud push, sino que simplemente se añaden a la tabla. Esto acorta el protocolo de mensajes push, por lo que los mensajes de validación no necesitan ser retransmitidos dos veces por la red.
4. Valor de CrdsValue para el voto debe verse como este `Votos (Vec<Transaction>)`

Cada transacción de votación debe mantener un `reloj de pared` en sus datos. La estrategia de fusión de votos mantendrá el último conjunto N de votos configurado por el cliente local. Para empujar/tirar el vector se recorre recursivamente y cada transacción se trata como un CrdsValue individual con su propio reloj de pared local y su firma.

El gossip está diseñado para una reproducción eficiente del estado. Los mensajes que se envían a través de gossip-push se envían por lotes y se propagan con un árbol de expansión mínimo al resto de la red. Cualquier fallo parcial en el árbol se reparan activamente con el protocolo de gossip-pull mientras se minimiza la cantidad de datos transferidos entre cualquier nodo.

## Cómo resuelve este diseño los Desafíos

1. Porque no hay una manera fácil de sincronizar los validadores con los líderes en el estado "activo" del líder, el gossip permite una entrega eventual independientemente de ese estado.
2. El Gossip enviará los mensajes a todos los líderes subsiguientes, así que si el líder actual es inundado, el próximo líder ya habría recibido estos votos y es capaz de codificarlos.
3. Gossip minimiza el número de peticiones a través de la red manteniendo un árbol de expansión eficiente, y usando filtros bloom para reparar el estado. Por lo tanto, no es necesario retransmitir la retroceso y los mensajes son por lotes.
4. Los líderes que lean la tabla de crds de votos codificarán todos los nuevos votos válidos que aparecen en la tabla. Incluso si el bloque de este líder está desenrollado, el próximo líder intentará añadir los mismos votos sin ningún trabajo adicional realizado por el validador. De este modo, no sólo la entrega final, sino la codificación final en el ledger.

## Rendimiento

1. Lo peor del tiempo de propagación para el próximo líder es Log\(N\) salta con una base dependiendo del fanout. Con nuestro actual fanout predeterminado de 6, es de unos 6 saltos a 20k nodos.
2. El líder debe recibir 20k votos de validación agregados por gossip en fragmentos de MTUsize. Lo que reduciría el número de paquetes para la red 20k a 80 trozos.
3. Cada voto de validadores es replicado en toda la red. Para mantener una cola de 5 votos anteriores la tabla de Crds crecería 25 megabytes. `(20,000 nodos * 256 bytes * 5)`.

## Implementación en dos etapas

Inicialmente la red puede funcionar de forma fiable con sólo 1 voto transmitido y mantenido a través de la red con la implementación actual del voto. Para redes pequeñas un fanout de 6 es suficiente. Con una pequeña red la memoria y la sobrecarga de empuje es menor.

### Sub1k red de validadores

1. Crds sólo mantiene el voto más reciente de los validadores.
2. Los votos son empujados y retransmitidos sin importar si están apareciendo en el ledger.
3. Fanout de 6.
4. Peor caso de 256kb sobrecarga de memoria por nodo.
5. En el peor de los casos, 4 saltos para propagarse a cada nodo.
6. El líder debe recibir todo el voto del validador establecido en 4 fragmentos de mensajes push.

### Sub20k de red

Todo por encima de lo siguiente:

1. La tabla CRDS mantiene un vector de 5 últimos votos de validador.
2. Los votos codifican un reloj de pared. CrdsValue::Votes es un tipo que se repite en el vector de transacciones para todos los protocolos gossip.
3. Aumenta el fanout a 20.
4. En el peor de los casos, 25 mb de memoria por nodo.
5. Sub 4 saltos en el peor de los casos para entregar a toda la red.
6. 80 fragmentos recibidos por el líder para todos los mensajes del validador.
