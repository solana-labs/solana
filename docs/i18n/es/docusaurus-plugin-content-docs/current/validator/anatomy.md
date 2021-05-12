---
title: Anatomía de un validador
---

![Diagramas de bloque de validadores](/img/validator.svg)

## Pipelining

Los validadores utilizan ampliamente una optimización común en el diseño de la CPU, llamada _pipelining_. El pipelining es la herramienta adecuada para el trabajo cuando hay un flujo de datos de entrada que necesita ser procesado por una secuencia de pasos, y hay diferentes hardware responsables de cada uno. El ejemplo quintaesencial es utilizar una lavadora y secadora para lavar/secar/doblar varias cargas de lavandería. El lavado debe producirse antes del secado y el secado antes del plegado, pero cada una de las tres operaciones es realizada por una unidad independiente. Para maximizar la eficiencia, se crea un pipeline de _etapas_. Llamaremos a la lavadora una etapa, a la secadora otra y al proceso de plegado una tercera. Para hacer funcionar el pipeline, se añade una segunda carga de ropa a la lavadora justo después de añadir la primera carga a la secadora. Del mismo modo, la tercera carga se añade a la lavadora después de la segunda se encuentra en la secadora y la primera se dobla. De este modo, se puede avanzar en tres cargas de lavandería simultáneamente. Dadas las cargas infinitas, el pipeline completará consistentemente una carga a la velocidad de la etapa más baja en el pipeline.

## Pipelining en el Validador

El validador contiene dos procesos en cadena, uno utilizado en modo líder llamado TPU y otro utilizado en modo validador llamado TVU. En ambos casos, el hardware que se canaliza es el mismo, la entrada de la red, las tarjetas GPU, los núcleos de la CPU, las escrituras en el disco y la salida de la red. Lo que hace con ese hardware es diferente. La TPU existe para crear entradas del ledger, mientras que la TVU existe para validarlas.
