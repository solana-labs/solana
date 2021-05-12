---
title: Reglas de slashing
---

A diferencia de la prueba de trabajo \(PoW\) donde los gastos de capital fuera de la cadena ya están desplegados en el momento de la construcción y votación de bloques, Los sistemas PoS requieren capital en riesgo para prevenir una estrategia lógica/óptima de votación en cadena múltiple. Pretendemos implementar reglas de slashing que, si se rompen, resultarán en alguna cantidad de stake depositada ofensiva del validador para ser removida de la circulación. Dada las propiedades de ordenamiento de la estructura de datos PoH, creemos que podemos simplificar nuestras reglas de slashing al nivel de un bloqueo de votación asignado por voto.

Es decir. Cada voto tiene un tiempo de bloqueo asociado \(duración de PoH\) que representa una duración por cualquier voto adicional de ese validador debe estar en un PoH que contiene el voto original, o una parte del stake de ese validador es propenso a un slashing. Esta duración es una función del recuento inicial de votos de PoH y todos los votos adicionales cuenta de PoH. Probablemente tome la forma:

```text
Lockouti\(PoHi, PoHj\) = PoHj + K \* exp\(\(PoHj - PoHi\) / K\)
```

Donde PoHi es la altura del voto al que se aplicará el bloqueo y PoHj es la altura del voto actual en el mismo fork. Si el validador envía una votación sobre un forkde PoH diferente en cualquier PoHk donde k &gt; j &gt; i y PoHk &lt; Lockout\(PoHi, PoHj\), entonces una parte del stake de ese validador está en riesgo de ser slashed.

Además del bloqueo de la forma funcional descrito anteriormente, la primera implementación puede ser una aproximación numérica basada en una estructura de datos First In, First Out \ (FIFO\) y la siguiente lógica:

- Cola FIFO con 32 votos por validador activo
- los nuevos votos se envían encima de la cola \(`push_front`\)
- los votos caducados son saltados de arriba \(`pop_front`\)
- ya que los votos se colocan en la cola, el bloqueo de cada voto en cola se duplica
- los votos son eliminados de la parte posterior de la cola si `queue.len() > 32`
- la altura más temprana y última que se ha eliminado de la parte trasera de la cola debe ser almacenada

Es probable que se ofrezca una recompensa como un % de la cantidad de slashing a cualquier nodo que envíe la prueba de que esta condición de slashing se viola al PoH.

### Slashing parcial

En el esquema descrito hasta ahora, cuando un validador vota en un flujo de PoH dado, se están comprometiendo con ese fork por un tiempo determinado por la votación de cierre. Una pregunta abierta es si los validadores estarán más dispuestos a comenzar votando en un fork disponible si las sanciones son percibidas demasiado duras para un error honesto o volteado un poco.

Una forma de abordar esta preocupación sería un diseño de slashing parcial que resulte en una cantidad reducida en función de:

1. la fracción de validadores, del pool total de validadores, que también fueron barridos durante el mismo periodo de tiempo \(ala Casper\)
2. la cantidad de tiempo desde que se emitió el voto \(por ejemplo, un aumento lineal % de total depositado como cantidad reducida a lo largo del tiempo\), o ambos.

Esta es una área actualmente en exploración.
