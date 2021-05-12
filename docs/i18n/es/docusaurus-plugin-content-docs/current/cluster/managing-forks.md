---
title: Administrando bifurcaciones
---

Se permite que el libro mayor se bifurque en los límites de las ranuras. La estructura de datos resultante forma un árbol llamado _ almacén de bloques _. Cuando el validador interpreta el almacén de bloques, debe mantener el estado de cada bifurcación de la cadena. Llamamos a cada instancia una _ bifurcación activa _. Es responsabilidad de un validador pesar esas bifurcaciones, de modo que eventualmente pueda seleccionar una bifurcación.

Un validador selecciona una bifurcación enviando un voto a un líder de ranura en esa bifurcación. El voto compromete al validador por un período de tiempo llamado _ período de bloqueo _. El validador no puede votar en una bifurcación diferente hasta que expire el período de bloqueo. Cada voto subsiguiente en la misma bifurcación duplica la duración del período de bloqueo. Después de un número de votos configurado en grupo \ (actualmente 32 \), la duración del período de bloqueo alcanza lo que se llama _ bloqueo máximo _. Hasta que se alcance el bloqueo máximo, el validador tiene la opción de esperar hasta que finalice el período de bloqueo y luego votar por otra bifurcación. Cuando vota por otra bifurcación, realiza una operación llamada _ retroceso _, mediante la cual el estado retrocede en el tiempo hasta un punto de control compartido y luego salta hacia la punta de la bifurcación que acaba de votar. La distancia máxima que una bifurcación puede retroceder se denomina _ profundidad de retroceso _. La profundidad de reversión es el número de votos necesarios para lograr el bloqueo máximo. Siempre que un validador vota, cualquier punto de control más allá de la profundidad de retroceso se vuelve inalcanzable. Es decir, no existe un escenario en el que el validador deba retroceder más allá de la profundidad de retroceso. Por lo tanto, puede _ podar _ bifurcaciones inalcanzables y _ aplastar _ todos los puntos de control más allá de la profundidad de retroceso en el punto de control raíz.

## Bifurcaciones activas

Una bifurcación activa es como una secuencia de puntos de control que tiene una longitud al menos uno más larga que la profundidad de retroceso. La bifurcación más corta tendrá una longitud exactamente, una más larga que la profundidad de retroceso. Por ejemplo:

![Bifurcaciones](/img/forks.svg)

Las siguientes secuencias son _ bifurcaciones activas _:

- {4, 2, 1}
- {5, 2, 1}
- {6, 3, 1}
- {7, 3, 1}

## Limpiar y aplastar

Un validador puede votar en cualquier punto de control del árbol. En el diagrama de arriba, son todos los nodos excepto las hojas del árbol. Después de la votación, el validador limpia los nodos que se bifurcan desde una distancia más lejana que la profundidad de retroceso y luego toma la oportunidad de minimizar su uso de memoria al aplastar cualquier nodo que pueda entrar en la raíz.

Empezando por el ejemplo anterior, con una profundidad de retroceso de 2, considere una votación sobre 5 contra una votación sobre 6. En primer lugar, una votación sobre 5:

![Bifurcaciones despues de limpiar](/img/forks-pruned.svg)

La nueva raíz es 2, y cualquier bifurcación activa que no sea descendiente de 2 se limpia.

Alternativamente, una votación sobre 6:

![Bifurcaciones](/img/forks-pruned2.svg)

El árbol permanece con una raíz de 1, ya que la bifurcación activa a partir de 6 es sólo 2 puntos de control de la raíz.
