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

As a first step to understanding the impact of the _Inflation Schedule_ on the Solana economy, we’ve simulated the upper and lower ranges of what token issuance over time might look like given the current ranges of Inflation Schedule parameters under study.

Específicamente:

- _Initial Inflation Rate_: 7-9%
- _Dis-inflation Rate_: -14-16%
- _Long-term Inflation Rate_: 1-2%

Utilizando estos rangos para simular una serie de posibles Programas de Inflación, podemos explorar la inflación con el tiempo:

![](/img/p_inflation_schedule_ranges_w_comments.png)

En la gráfica anterior, los valores medios del rango se identifican para ilustrar la contribución de cada parámetro. From these simulated _Inflation Schedules_, we can also project ranges for token issuance over time.

![](/img/p_total_supply_ranges.png)

Finally we can estimate the _Staked Yield_ on staked SOL, if we introduce an additional parameter, previously discussed, _% of Staked SOL_:

%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}

In this case, because _% of Staked SOL_ is a parameter that must be estimated (unlike the _Inflation Schedule_ parameters), it is easier to use specific _Inflation Schedule_ parameters and explore a range of _% of Staked SOL_. Para el ejemplo de abajo, hemos elegido el medio de los rangos de parámetros explorados anteriormente:

- _Initial Inflation Rate_: 8%
- _Dis-inflation Rate_: -15%
- _Long-term Inflation Rate_: 1.5%

The values of _% of Staked SOL_ range from 60% - 90%, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example _Staked Yield_ that a staker might expect over time on the Solana network with the _Inflation Schedule_ as specified. This is an idealized _Staked Yield_ as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling and potential slashing incidents. It additionally ignores that _% of Staked SOL_ is dynamic by design - the economic incentives set up by this _Inflation Schedule_.

### Yield de Staking Ajustado

A complete appraisal of earning potential from staking tokens should take into account staked _Token Dilution_ and its impact on staking yield. For this, we define _adjusted staking yield_ as the change in fractional token supply ownership of staked tokens due to the distribution of inflation issuance. Es decir. los efectos dilutivos positivos de la inflación.

We can examine the _adjusted staking yield_ as a function of the inflation rate and the percent of staked tokens on the network. Podemos ver esto representado para varias fracciones de stake aquí:

![](/img/p_ex_staked_dilution.png)
