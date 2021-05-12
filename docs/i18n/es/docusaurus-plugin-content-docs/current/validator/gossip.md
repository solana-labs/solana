---
title: Servicio Gossip
---

El servicio Gossip actúa como una puerta de entrada a los nodos en el plano de control. Los validadores utilizan el servicio para asegurar que todos los demás nodos estén disponibles en un cluster. El servicio transmite información mediante un protocolo Gossip.

## Vista general Gossip

Los nodos comparten continuamente objetos de datos firmados entre sí para gestionar un cluster. Por ejemplo, comparten su información de contacto, la altura del ledger y los votos.

Cada décima de segundo, cada nodo envía un mensaje "push" y/o un mensaje "pull". Los mensajes push y pull pueden generar respuestas, y los mensajes push pueden ser enviados a otros en el cluster.

Gossip se ejecuta en un conocido puerto UDP/IP o un puerto en un rango conocido. Una vez que un clúster se inicia, los nodos se anuncian mutuamente donde encontrar su punto final gossip \(una dirección de socket\).

## Registros Gossip

Los registros compartidos sobre gossip son arbitrarios, pero firmados y versionados \(con una marca de tiempo\) según sea necesario para tener sentido al nodo que los recibe. Si un nodo recibe dos registros de la misma fuente, actualiza su propia copia con el registro con la marca de tiempo más reciente.

## Interfaz de servicio Gossip

### Mensaje Push

Un nodo envía un mensaje push para decirle al clúster que tiene información para compartir. Los nodos envían mensajes push a `PUSH_FANOUT` pares push.

Al recibir un mensaje push, un nodo examina el mensaje para:

1. Duplicación: si el mensaje ha sido visto antes, el nodo suelta el mensaje y puede responder con `PushMessagePrune` si es reenviado desde un nodo bajo
2. Nuevos datos: si el mensaje es nuevo en el nodo

   - Guarda la nueva información con una versión actualizada en su información de clúster y

     purga cualquier valor anterior antiguo

   - Guarda el mensaje en `pushed_once` \\(usado para detectar duplicados,

     purgado después de `PUSH_MSG_TIMEOUT * 5` ms\)

   - Retransmite los mensajes a sus propios pares push

3. Caducidad: los nodos abandonan los mensajes push que son más antiguos que `PUSH_MSG_TIMEOUT`

### Empujar pares, purgar mensaje

Un nodo selecciona sus pares push al azar del conjunto activo de pares conocidos. El nodo mantiene esta selección por un tiempo relativamente largo. Cuando se recibe un mensaje de poda, el nodo abandona el peer push que envió la poda. La poda es un indicio de que hay otro camino ponderado más alto que el empuje directo.

El conjunto de pares push se mantiene fresco girando un nuevo nodo en el conjunto cada `PUSH_MSG_TIMEOUT/2` milisegundos.

### Mensaje Pull

Un nodo envía un mensaje pull para preguntar al clúster si hay alguna nueva información. Un mensaje pull es enviado a un solo par al azar y comprende un filtro de Bloom que representa cosas que ya tiene. Un nodo que recibe un mensaje pull iterará sobre sus valores y construirá una respuesta pull de cosas que faltan el filtro y caben en un mensaje.

Un nodo construye el filtro pull Bloom iterando sobre los valores actuales y los valores purgados recientemente.

Un nodo maneja los elementos en una respuesta de la misma manera que maneja los nuevos datos en un mensaje push.

## Purgando

Los nodos conservan versiones anteriores de los valores \\(los actualizados por un pull o push\\) y caducaron los valores \\(los anteriores que `GOSSIP_PULL_CRDS_TIMEOUT_MS`\\) en `purged_values` \\(cosas que recientemente tenía\\). Los nodos purged_values `purged_values` que son más antiguos que `5 * GOSSIP_PULL_CRDS_TIMEOUT_MS`.

## Ataques Eclipse

Un ataque eclipse es un intento de tomar el control del conjunto de conexiones de nodo con puntos finales adversarios.

Esto es relevante para nuestra implementación de las siguientes maneras.

- Los mensajes Pull seleccionan un nodo al azar de la red. Un ataque de eclipse sobre _pull_ requeriría que un atacante influyera en la selección aleatoria de tal manera que solo se seleccionen nodos adversarios para el pull.
- Los mensajes push mantienen un conjunto activo de nodos y selecciona un fanout aleatorio para cada mensaje push. Un ataque de eclipse sobre _push_ influiría en la selección de conjunto activo, o en la selección de fanout aleatoria.

### Pesos basados en el tiempo y el Stake

Los pesos se calculan en función del `time since last picked` y el `natural log` del `stake weight`.

Tomar el `ln` del peso del stake permite dar a todos los nodos una oportunidad más justa de la cobertura de la red en una cantidad razonable de tiempo. Esto ayuda a normalizar las posibles diferencias de `stake weight` entre nodos. De esta manera un nodo con bajo `bstake weight`, comparado con un nodo con un gran `stake weight` sólo tendrá que esperar unos cuantos múltiplos de ln\(`stake`\) segundos antes de ser elegido.

No hay forma de que un adversario influya en estos parámetros.

### Mensaje Pull

Un nodo es seleccionado como un pull target basado en las influencias descritas anteriormente.

### Mensaje Push

Un mensaje de poda sólo puede eliminar un adversario de una conexión potencial.

Al igual que _pull message_, los nodos se seleccionan en el conjunto activo basado en los pesos.

## Diferencias notables de PlumTree

El protocolo push activo descrito aquí se basa en [Plum Tree](https://haslab.uminho.pt/sites/default/files/jop/files/lpr07a.pdf). Las principales diferencias son:

- Los mensajes push tienen un reloj firmado por el creador. Una vez que el reloj expira el mensaje es soltado. Un límite del salto es difícil de aplicar en un contexto adverso.
- Un push vago no se implementa porque no es obvio cómo evitar que un adversario falsifique la huella digital del mensaje. Un enfoque ingenuo permitiría dar prioridad a un adversario en función de sus aportaciones.
