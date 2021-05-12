---
title: Programación de inflación
---

**Sujeto a cambios. Sigue los debates económicos más recientes en los foros de Solana: https://forums.solana.com**

Los validadores-clientes tienen dos funciones funcionales en la red Solana:

- Validar \(votación\) el estado global actual de PoH observado.
- Ser elegido como "líder" en un programa de rotación ponderado por la participación, durante el cual es responsable de recoger las transacciones pendientes e incorporarlas a su PoH observado, actualizando así el estado global de la red y proporcionando la continuidad de la cadena.

Las recompensas del validador-cliente por estos servicios se distribuirán al final de cada época de Solana. Como se ha comentado anteriormente, la compensación para los clientes validadores se realiza a través de una comisión cobrada sobre la tasa de inflación anual basada en el protocolo y dispersada en proporción al peso del stake de cada nodo validador (ver más abajo) junto con las comisiones por transacciones reclamadas por el líder disponibles durante cada rotación del líder. Es decir. durante el tiempo en que un determinado cliente-validador es elegido como líder, tiene la oportunidad de quedarse con una parte de cada tasa de transacción, menos una cantidad especificada por el protocolo que se destruye (ver [Tasas de transacción del estado del cliente-validador](ed_vce_state_validation_transaction_fees.md)).

El rendimiento anual efectivo basado en el protocolo \(%\) por epoch recibido por los clientes de validación debe ser una función de:

- la tasa de inflación global actual, derivada del programa de emisión desinflacionaria predeterminado \(ver [Economía de cliente de validación](ed_vce_overview.md)\)
- la fracción de SOLs en stake de la oferta total en circulación,
- la comisión cargada por el servicio de validación,
- el tiempo de actividad/participación \[% de las ranuras disponibles sobre las que el validador tuvo la oportunidad de votar\] de un validador dado sobre la época anterior.

El primer factor es una función de los parámetros del protocolo únicamente (es decir, independiente del comportamiento del validador en una determinada época) y da lugar a un programa de inflación diseñado para incentivar la participación temprana, proporcionar una clara estabilidad monetaria y ofrecer una seguridad óptima en la red.

Como primer paso para entender el impacto del _Programa de Inflación_ en la economía de Solana, hemos simulado los rangos superiores e inferiores de lo que podría ser la emisión de fichas a lo largo del tiempo dados los rangos actuales de los parámetros del Programa de Inflación en estudio.

Específicamente:

- _Tasa de inflación inicial_: 7-9%
- _Tasa de desinflación_: -14-16%
- _Tasa de inflación a largo plazo_: 1-2%

Utilizando estos rangos para simular una serie de posibles Programas de Inflación, podemos explorar la inflación con el tiempo:

![](/img/p_inflation_schedule_ranges_w_comments.png)

En la gráfica anterior, los valores medios del rango se identifican para ilustrar la contribución de cada parámetro. De estos *Programas de Inflación*simulados, también podemos proyectar rangos para emisión de tokens con el tiempo.

![](/img/p_total_supply_ranges.png)

Finalmente podemos estimar el _Yield en stake_ en el SOL en stake, si introdujimos un parámetro adicional, discutido anteriormente, _% del SOL en stake_:

%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}

En este caso, porque _% de SOL en stake_ es un parámetro que debe ser estimado (a diferencia de los parámetros _Inflation Schedule_), es más fácil utilizar parámetros específicos _Horario de inflación_ y explorar un rango de _% del SOL en stake_. Para el ejemplo de abajo, hemos elegido el medio de los rangos de parámetros explorados anteriormente:

- _Tasa de inflación inicial_: 8%
- _Tasa de desinflación_: -15%
- _Tasa de inflación a largo plazo_: 1.5%

Los valores del _% del SOL en stake_ varían entre 60% - 90%, que sentimos cubre el rango probable que esperamos observar, basándose en la retroalimentación de las comunidades de inversores y validadores, así como en lo que se observa en protocolos de prueba de stake comparables.

![](/img/p_ex_staked_yields.png)

Nuevamente, el ejemplo anterior muestra un _Yield en stake_ que un staker podría esperar con el tiempo en la red Solana con el _Programa de Inflación_ como se especificó. Este es un _Staked Yield_ idealizado ya que desatiende el impacto de tiempo de uso del validador en recompensas, comisiones de validadores, posibles ataques de rendimiento y posibles incidentes de slashing. Adicionalmente ignora que el _% del SOL en stake_ es dinámico por diseño - los incentivos económicos establecidos por este _Programa de Inflación_.

### Yield de Staking Ajustado

Una valoración completa del potencial de ganancias de los tokens en stake debe tener en cuenta la _Dilución de los tokens_ y su impacto en el staking yield. Para esto, definimos _rendimiento de staking yield_ como el cambio en la propiedad de la oferta fraccional de tokens en stake debido a la distribución de la emisión de inflación. Es decir. los efectos dilutivos positivos de la inflación.

Podemos examinar el r _staking yield ajustado_ en función de la tasa de inflación y el porcentaje de tokens en stake en la red. Podemos ver esto representado para varias fracciones de stake aquí:

![](/img/p_ex_staked_dilution.png)
