---
title: Tarifas de Transacción por validación del estado
---

**Sujeto a cambios.**

Cada transacción enviada a través de la red, para ser procesada por el líder actual de validación-cliente y confirmada como una transacción de estado global, debe contener una tasa de transacción. Las tasas de transacción ofrecen muchos beneficios en el diseño económico de Solana, por ejemplo:

- proporcionar una compensación unitaria a la red de validadores por los recursos de la CPU/GPU necesarios para procesar el estado de transacción,
- reduce el spam de la red introduciendo el coste real de las transacciones,
- abrir vías para que un mercado de transacciones para incentivar la validación-cliente para recoger y procesar las transacciones presentadas en su función de líder,
- y proporcionar una potencial estabilidad económica a largo plazo de la red a través de un importe mínimo de tarifa por transacción capturado por el protocolo, como se describe a continuación.

Muchas economías actuales de blockchain (por ejemplo, Bitcoin, Ethereum), se basan en recompensas basadas en el protocolo para apoyar la economía a corto plazo, con el supuesto de que los ingresos generados a través de las tasas de transacción apoyarán la economía a largo plazo, cuando las recompensas derivadas del protocolo expiren. En un intento de crear una economía sostenible a través de recompensas basadas en el protocolo y tarifas de transacción, se destruye una parte fija de cada tarifa de transacción, y el resto se destina al líder actual que procesa la transacción. Una tasa de inflación global programada proporciona una fuente para las recompensas distribuidas a los clientes de validación, a través del proceso descrito anteriormente.

Las tarifas de las transacciones las fija el clúster de la red en función del rendimiento histórico reciente, véase [Tarifas en función de la congestión](../../transaction-fees.md#congestion-driven-fees). Esta parte mínima de la tarifa de cada transacción puede ajustarse dinámicamente en función del consumo histórico de gas. De este modo, el protocolo puede utilizar la tasa mínima para apuntar a una utilización deseada del hardware. Al supervisar un uso de gas especificado en el protocolo con respecto a una cantidad de uso deseada, la tarifa mínima puede aumentarse o reducirse, lo que a su vez debería reducir o aumentar el uso real de gas por bloque hasta alcanzar la cantidad objetivo. Este proceso de ajuste puede considerarse similar al algoritmo de ajuste de la dificultad en el protocolo de Bitcoin, sin embargo, en este caso está ajustando la tasa mínima de transacción para guiar el uso del hardware de procesamiento de transacciones a un nivel deseado.

Como ya se ha dicho, se destruirá una parte fija de la tasa de cada transacción. La intención de este diseño es mantener el incentivo del líder para incluir tantas transacciones como sea posible dentro del tiempo de la ranura del líder, a la vez que se proporciona un mecanismo de limitación de la inflación que protege contra los ataques de "evasión fiscal" (es decir, los pagos de comisiones por canal lateral)[1](../ed_references.md).

Además, los honorarios de los quemados pueden ser una consideración en la selección del fork. En el caso de un fork PoH con un líder malicioso y censurador, esperaríamos que las tasas totales destruidas fueran menores que las de un fork honesto comparable, debido a las tasas perdidas por la censura. Si el líder de la censura debe compensar estas tasas de protocolo perdidas, tendría que reemplazar las tasas quemadas en su fork por sí mismo, reduciendo así potencialmente el incentivo para censurar en primer lugar.
