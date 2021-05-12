---
title: Torre BFT
---

Este diseño describe el algoritmo _Tower BFT_ de Solana. Aborda los siguientes problemas:

- Es posible que algunos forks no terminen aceptados por la supermayoría del cluster, y que los votantes tengan que recuperarse de votar sobre tales forks.
- Varios forks pueden ser votadas por diferentes votantes, y cada votante puede ver un conjunto diferente de forks votables. Los forks seleccionados deberían eventualmente converger para el clúster.
- Los votos basados en recompensas tienen un riesgo asociado. Los voceros deben tener la capacidad de configurar cuánto riesgo asumen.
- El [costo de la devolución](tower-bft.md#cost-of-rollback) necesita ser computable. Es importante para los clientes que confían en alguna forma de Consistencia reconocible. Los costos para romper la consistencia necesitan ser calculables y aumentar súper lineal para los votos anteriores.
- Las velocidades ASIC son diferentes entre nodos, y los atacantes podrían emplear la prueba del historial ASICS que son mucho más rápidos que el resto del cluster. El consenso necesita ser resistente a los ataques que explotan la variabilidad de la velocidad de prueba del historial ASIC.

Por brevedad, este diseño asume que un votante con una participación es desplegado como validador individual en el cluster.

## Tiempo

El clúster de Solana genera una fuente de tiempo a través de una función de Retraso Verificable que estamos llamando [Prueba del Historia](../cluster/synchronization.md).

Prueba de la Historia se utiliza para crear un programa determinista redondo robin para todos los líderes activos. En cualquier momento dado sólo 1 líder, que puede ser calculado a partir del propio ledger, puede proponer un fork. Para más detalles, consulte [generación de forks](../cluster/fork-generation.md) y [rotación de líder](../cluster/leader-rotation.md).

## Bloqueos

El propósito del bloqueo es forzar a un validador a comprometer el costo de oportunidad a un fork específico. Los bloqueos se miden en ranuras y por lo tanto representan un retraso forzado en tiempo real que un validador necesita esperar antes de romper el compromiso con un fork.

Los validadores que violen los bloqueos y voten por un fork divergente dentro del bloqueo deben ser castigados. El castigo propuesto es reducir la participación del validador si se puede demostrar al grupo un voto concurrente dentro de un bloqueo para un fork no descendiente.

## Algoritmo

La idea básica de este enfoque es acumular votos consensuados y duplicar bloqueos. Cada votación en la pila es una confirmación de un fork. Cada fork confirmado es un ancestro del fork sobre ella. Cada voto tiene un `lockout` en unidades de espacios antes de que el validador pueda enviar un voto que no contenga la bifurcación confirmada como antepasado.

Cuando se añade una votación a la pila, los bloqueos de todos los votos anteriores en la pila se duplican \(más sobre esto en [Rollback](tower-bft.md#Rollback)\). Con cada nuevo voto, un validador compromete los votos anteriores a un bloqueo cada vez mayor. A 32 votos podemos considerar que el voto está a `max lockout` cualquier voto con un bloqueo igual o superior `1<<32` están en cola \(FIFO\). Retirar un voto es el detonante de una recompensa. Si un voto caduca antes de que se coloque en cola, éste y todos los votos arriba aparecerán \(LIFO\) de la lista de votaciones. El validador necesita comenzar a reconstruir la pila desde ese punto.

### Rollback

Antes de que la votación se pase a la pila, todos los votos previos a la votación con un tiempo de bloqueo más bajo que el nuevo voto son saltados. Después de que los bloqueos de cancelación no se dupliquen hasta que el validador alcance la altura de los votos.

Por ejemplo, una lista de votos con el siguiente estado:

| voto | momento del voto | bloqueos | tiempo de expiración de bloqueo |
| ---: | ---------------: | -------: | ------------------------------: |
|    4 |                4 |        2 |                               6 |
|    3 |                3 |        4 |                               7 |
|    2 |                2 |        8 |                              10 |
|    1 |                1 |       16 |                              17 |

_Voto 5_ es al momento 9, y el estado resultante es

| voto | momento del voto | bloqueo | tiempo de expiración de bloqueo |
| ---: | ---------------: | ------: | ------------------------------: |
|    5 |                9 |       2 |                              11 |
|    2 |                2 |       8 |                              10 |
|    1 |                1 |      16 |                              17 |

_Vota 6_ es en el momento 10

| voto | momento del voto | bloqueo | tiempo de expiración de bloqueo |
| ---: | ---------------: | ------: | ------------------------------: |
|    6 |               10 |       2 |                              12 |
|    5 |                9 |       4 |                              13 |
|    2 |                2 |       8 |                              10 |
|    1 |                1 |      16 |                              17 |

En el momento 10 los nuevos votos alcanzaron hasta las votaciones anteriores. Pero _voto 2_ expira a las 10, así que cuando _vote 7_ en el momento 11 se apliquen los votos incluyendo y por encima _voto 2_ se mostrarán.

| voto | momento del voto | bloqueo | tiempo de expiración de bloqueo |
| ---: | ---------------: | ------: | ------------------------------: |
|    7 |               11 |       2 |                              13 |
|    1 |                1 |      16 |                              17 |

El bloqueo para la votación 1 no aumentará de 16 hasta que la pila contenga 5 votos.

### Slashing y Recompensas

Los validadores deben ser recompensados por seleccionar el fork que el resto del cluster seleccionado tan a menudo como sea posible. Esto está bien alineado con la generación de una recompensa cuando la lista de votos está llena y el voto más antiguo tiene que ser retirado. Por lo tanto, se debe generar una recompensa por cada dequeue exitoso.

### Costo del Rollback

El coste del rollback del _fork A_ se define como el coste en términos de tiempo de bloqueo para que el validador confirme cualquier otro fork que no incluya el _fork A_ como antecesor.

La **finalidad económica** de _fork A_ puede calcularse como la pérdida de todas las recompensas de la reversión de _fork A_ y sus descendientes, más el coste de oportunidad de la recompensa debido al bloqueo exponencialmente creciente de los votos que han confirmado _Fork A_.

### Umbrales

Cada validador puede establecer de forma independiente un umbral de compromiso del clúster con un fork antes de que ese validador se comprometa con una fork. Por ejemplo, en el índice de pila de votos 7, el bloqueo es de 256 unidades de tiempo. Un validador puede retener votos y dejar que los votos 0-7 expiren a menos que el voto en el índice 7 tenga un compromiso mayor al 50% en el cluster. Esto permite que cada validador controle de forma independiente cuánto riesgo debe comprometerse a un fork. Comprometerse con forks con mayor frecuencia permitiría al validador ganar más recompensas.

### Parámetros del algoritmo

Los siguientes parámetros deben ser ajustados:

- Número de votos en la pila antes de que se produzca la cola \\(32\\).
- Tasa de crecimiento de bloqueos en la pila \(2x\).
- Iniciando bloqueo por defecto \(2\).
- Profundidad de umbral para el compromiso mínimo de cluster antes de comprometerse con el fork \(8\).
- Tamaño mínimo del compromiso de cluster en profundidad umbral \(50%+\).

### Elección Libre

Una "Elección libre" es una acción de validador no obligatoria. No hay forma de que el protocolo codifique y ejecute estas acciones ya que cada validador puede modificar el código y ajustar el algoritmo. Un validador que maximiza la autorecompensa sobre todos los futuros posibles debe comportarse de tal manera que el sistema sea estable, y la codiciosa elección local debe dar lugar a una elección codiciosa sobre todos los futuros posibles. Un conjunto de validadores que están participando en las elecciones para interrumpir el protocolo deben estar vinculados por su peso de stake en la negación del servicio. Dos salidas para el validador:

- un validador puede superar al anterior en la generación virtual y presentar un fork concurrente
- un validador puede retener un voto para observar múltiples forks antes de votar

En ambos casos, el validador en el cluster tiene varios forks de los que elegir simultáneamente, aunque cada fork representa una altura diferente. En ambos casos es imposible que el protocolo detecte si el comportamiento del validador es intencional o no.

### Elección avariciosa para Forks Concurrentes

Cuando se evalúan múltiples forks, cada validador debe utilizar las siguientes reglas:

1. Los forksdeben satisfacer la regla _del umbral_.
2. Seleccionar el fork que maximiza el tiempo total de bloqueo de cluster para todas los forks de antepasados.
3. Seleccione el Fork que tiene la mayor cantidad de cuotas de transacción de cluster.
4. Elige el último fork en términos de PoH.

Las comisiones de transacción del clúster son comisiones que se depositan en el pool de minería como se describe en la sección de [Recompensas de Staking](staking-rewards.md).

## Resistencia ASIC PoH

Los votos y los bloqueos crecen exponencialmente mientras que la aceleración ASIC es lineal. Hay dos vectores de ataque posibles que involucren una ASIC más rápida.

### Censura ASIC

Un atacante genera un fork concurrente que supera a los líderes anteriores en un esfuerzo por censurarlos. Un Fork propuesto por este atacante estará disponible simultáneamente con el próximo líder disponible. Para que los nodos elijan este fork, debe satisfacer la regla de _Elección Avariciosa_.

1. El fork debe tener el mismo número de votos para la fork de los antepasados.
2. El Fork no puede estar tan lejos como para causar votos caducados.
3. El fork debe tener una mayor cantidad de comisiones de transacciones de cluster.

Este ataque se limita entonces a censurar los fees anteriores de los líderes y las transacciones individuales. Pero no puede detener el cluster, o reducir el conjunto de validadores comparado con el fork concurrente. La censura de comisiones está limitada a las tasas de acceso que van a los líderes, pero no a los validadores.

### ASIC Rollback

Un atacante genera un fork concurrente de un bloque anterior para intentar deshacer el cluster. En este ataque el fork concurrente está compitiendo con forks que ya han sido votados. Este ataque está limitado por el crecimiento exponencial de los bloqueos.

- 1 voto tiene un bloqueo de 2 ranuras. El fork concurrente debe tener al menos 2 ranuras por delante, y ser producida en 1 ranura. Por lo tanto, requiere un ASIC 2x más rápido.
- 2 votos tienen un bloqueo de 4 ranuras. El fork concurrente debe tener al menos 4 ranuras por delante, y ser producida en 2 ranura. Por lo tanto, requiere un ASIC 2x más rápido.
- 3 votos tienen un bloqueo de 8 ranuras. El fork concurrente debe tener al menos 8 ranuras por delante, y ser producida en 3 ranura. Por lo tanto, requiere un ASIC 2.6x más rápido.
- 10 votos tienen un bloqueo de 1024 ranuras. un ASIC 1024/10, o 102.4x más rápido.
- 20 votos tienen un bloqueo de 2^20 ranuras. un ASIC 2^20/20, o 52,428.8x más rápido.
