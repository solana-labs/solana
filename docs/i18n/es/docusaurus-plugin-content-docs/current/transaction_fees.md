---
title: Transaction Fees
---

**Sujeto a cambios.**

Each transaction sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, contains a transaction fee. Las tasas de transacción ofrecen muchos beneficios en el diseño económico de Solana, por ejemplo:

- proporcionar una compensación unitaria a la red de validadores por los recursos de la CPU/GPU necesarios para procesar el estado de transacción,
- reduce el spam de la red introduciendo el coste real de las transacciones,
- y proporcionar una potencial estabilidad económica a largo plazo de la red a través de un importe mínimo de tarifa por transacción capturado por el protocolo, como se describe a continuación.

Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

Muchas economías actuales de blockchain (por ejemplo, Bitcoin, Ethereum), se basan en recompensas basadas en el protocolo para apoyar la economía a corto plazo, con el supuesto de que los ingresos generados a través de las tasas de transacción apoyarán la economía a largo plazo, cuando las recompensas derivadas del protocolo expiren. In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion (initially 50%) of each transaction fee is destroyed, with the remaining fee going to the current leader processing the transaction. Una tasa de inflación global programada proporciona una fuente para las recompensas distribuidas a los clientes de validación, a través del proceso descrito anteriormente.

Las tarifas de las transacciones las fija el clúster de la red en función del rendimiento histórico reciente, véase [Tarifas en función de la congestión](implemented-proposals/transaction-fees.md#congestion-driven-fees). This minimum portion of each transaction fee can be dynamically adjusted depending on historical _signatures-per-slot_. De este modo, el protocolo puede utilizar la tasa mínima para apuntar a una utilización deseada del hardware. By monitoring a protocol specified _signatures-per-slot_ with respect to a desired, target usage amount, the minimum fee can be raised/lowered which should, in turn, lower/raise the actual _signature-per-slot_ per block until it reaches the target amount. Este proceso de ajuste puede considerarse similar al algoritmo de ajuste de la dificultad en el protocolo de Bitcoin, sin embargo, en este caso está ajustando la tasa mínima de transacción para guiar el uso del hardware de procesamiento de transacciones a un nivel deseado.

Como ya se ha dicho, se destruirá una parte fija de la tasa de cada transacción. The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing an inflation limiting mechanism that protects against "tax evasion" attacks \(i.e. side-channel fee payments\).

Además, los honorarios de los quemados pueden ser una consideración en la selección del fork. En el caso de un fork PoH con un líder malicioso y censurador, esperaríamos que las tasas totales destruidas fueran menores que las de un fork honesto comparable, debido a las tasas perdidas por la censura. Si el líder de la censura debe compensar estas tasas de protocolo perdidas, tendría que reemplazar las tasas quemadas en su fork por sí mismo, reduciendo así potencialmente el incentivo para censurar en primer lugar.
