---
title: Métricas de rendimiento
---

El rendimiento del cluster de Solana se mide como el número promedio de transacciones por segundo que la red puede sostener \(TPS\). Y, cuánto tiempo tarda una transacción en ser confirmada por la súper mayoría del clúster \(Tiempo de Confirmación\).

Cada nodo de cluster mantiene varios contadores que se incrementan en ciertos eventos. Estos contadores se suben periódicamente a una base de datos basada en la nube. El tablero de métricas de Solana obtiene estos contadores y calcula las métricas de rendimiento y las muestra en el panel.

## TPS

El tiempo de ejecución de cada nodo mantiene un recuento de transacciones que ha procesado. El panel primero calcula el recuento medio de transacciones en todos los nodos habilitados para métricas en el clúster. El recuento medio de transacciones de cluster es entonces promediado en un período de 2 segundos y se muestra en el gráfico de series de tiempo TPS. El panel de control también muestra las estadísticas de promedio TPS, Max TPS y Total de transacciones que se calculan a partir del recuento de transacciones medianas.

## Tiempo de confirmación

Cada nodo de validación mantiene una lista de bifurcaciones de contabilidad activas que son visibles para el nodo. Se considera que una bifurcación está congelada cuando el nodo ha recibido y procesado todas las entradas correspondientes a la bifurcación. Se considera que una bifurcación está confirmada cuando recibe un voto de supermayoría acumulativa y cuando una de sus bifurcaciones secundarias se congela.

El nodo asigna una marca de tiempo a cada nueva bifurcación y calcula el tiempo que tardó en confirmar la bifurcación. Esta vez se refleja como el tiempo de confirmación del validador en las métricas de rendimiento. El panel de desempeño muestra el promedio del tiempo de confirmación de cada nodo validador como un gráfico de la serie de tiempo.

## Configuración de hardware

El software validador está desplegado en instancias GCP n1-standard-16 con disco pd-sd 1TB y 2x GPU Nvidia V100. Estos están desplegados en la región us-west-1.

solana-bench-tps se inicia después de que la red converge desde una máquina cliente con una instancia n1-standard-16 de solo CPU con los siguientes argumentos: ` --tx \ _count = 50000 --thread-batch-sleep 1000 `

Las métricas TPS y de confirmación se capturan a partir de los números de tablero en un promedio de 5 minutos de cuando comienza la etapa de transferencia de las tps de los bancos.
