---
title: Confirmaciones optimistas y slashing
---

El progreso en la confirmación optimista puede ser rastreado aquí

https://github.com/solana-labs/solana/projects/52

A finales de mayo, el mainnet-beta se está moviendo a 1,1, y la red de pruebas se está moviendo a 1,2. Con la versión 1.2, testnet se comportará como si tuviera una finalidad optimista siempre que al menos no más del 4,66% de los validadores actúen con malicia. Las aplicaciones pueden suponer que 2/3+ de los votos observados en los gossip confirman un bloque o que al menos el 4,66% de la red está violando el protocolo.

## ¿Cómo funciona?

La idea general es que los validadores deben continuar votando después de su última bifurcación, a menos que el validador pueda construir una prueba de que su bifurcación actual no puede alcanzar esa finalidad. La forma en que los validadores construyen esta prueba es recogiendo los votos de todas las bifurcaciones excluyendo la suya. Si el conjunto de votos válidos representa más de 1/3+X del peso del stake de la época, puede que no haya forma de que la horquilla actual de los validadores alcance los 2/3+ de la finalidad. El validador hace un hash de la prueba (crea un testigo) y la presenta con su voto para el fork alternativo. Pero si 2/3+ votan por el mismo bloque, es imposible que ninguno de los validadores construya esta prueba, y por lo tanto ningún validador es capaz de cambiar de bifurcación y este bloque será eventualmente finalizado.

## Tradeoffs

El margen de seguridad es 1/3+X, donde X representa la cantidad mínima de stake que se cortará en caso de que se viole el protocolo. El tradeoff es que la capacidad de respuesta se reduce en 2 veces en el peor de los casos. Si más de 1/3 - 2X de la red no está disponible, la red puede bloquearse y sólo reanudará la finalización de bloques cuando la red se recupere por debajo de 1/3 - 2X de los nodos que fallan. Hasta ahora, no hemos observado un gran golpe de indisponibilidad en nuestra red principal, en Cosmos o en Tezos. En el caso de nuestra red, compuesta principalmente por sistemas de alta disponibilidad, esto parece poco probable. Actualmente, hemos fijado el porcentaje de umbral en el 4,66%, lo que significa que si el 23,68% ha fallado, la red puede dejar de finalizar bloques. Para nuestra red, que se compone principalmente de sistemas de alta disponibilidad, una caída del 23,68% en la disponibilidad parece poco probable. 1:10^12 probabilidades asumiendo cinco nodos con un stake de 4.7% con 0.995 de tiempo de actividad.

## Seguridad

La media de votos por franja horaria a largo plazo ha sido de 670.000.000 de votos / 12.000.000 de franjas horarias, es decir, 55 de los 64 validadores de votos. Esto incluye los bloques perdidos debido a los fallos del productor de bloques. Cuando un cliente ve 55/64, o ~86% confirmando un bloque, puede esperar que un ~24% o `(86 - 66.666.. + 4.666..)%` de la red debe ser barrida para que este bloque falle la finalización completa.

## ¿Por qué Solana?

Este enfoque puede basarse en otras redes. pero la complejidad de la implementación se reduce significativamente en Solana porque nuestros votos tienen tiempos de espera demostrables basados en VDF. No está claro si las pruebas de conmutación pueden construirse fácilmente en redes con suposiciones débiles sobre el tiempo.

## Mapa de ruta del slashing

El slashing es un problema difícil, y se hace más difícil cuando el objetivo de la red es tener la menor latencia posible. Los tradeoffs son especialmente aparentes al optimizar la latencia. Por ejemplo, lo ideal es que los validadores emitan y propaguen sus votos antes de que la memoria se haya sincronizado con el disco, lo que significa que el riesgo de corrupción del estado local es mucho mayor.

Fundamentalmente, nuestro objetivo para el slashing es recortar el 100% en los casos en los que el nodo está tratando de violar maliciosamente las reglas de seguridad y el 0% durante el funcionamiento rutinario. La forma en que pretendemos conseguirlo es, en primer lugar, aplicar las pruebas de slashing sin ningún tipo de corte automático.

Ahora mismo, para consensos regulares, después de una violación de seguridad, la red se detendrá. Podemos analizar los datos y averiguar quién fue el responsable y proponer que se corte el stake tras el reinicio. Un enfoque similar se utilizará con una configuración optimista. Una violación optimista de la seguridad de la configuración es fácilmente observable, pero en circunstancias subnormales, una violación optimista de la seguridad de la configuración puede no detener la red. Una vez observada la infracción, los validadores congelarán el stake afectada en la siguiente época y decidirán en la siguiente actualización si la infracción requiere un slashing.

A largo plazo, las transacciones deberían poder recuperar una parte de la garantía de slashing si se demuestra la violación optimista de la seguridad. En ese escenario, cada bloque está asegurado efectivamente por la red.
