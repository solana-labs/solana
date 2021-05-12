---
title: Economía del cluster
---

**Sujeto a cambios. Sigue los debates económicos más recientes en los foros de Solana: https://forums.solana.com**

El sistema criptoeconómico de Solana está diseñado para promover una economía sana y autosostenible a largo plazo con incentivos participantes alineados a la seguridad y descentralización de la red. Los principales participantes en esta economía son los clientes de validación. A continuación se analizan sus contribuciones a la red, la validación del estado y sus mecanismos de incentivos solicitados.

Los principales canales de las remesas de los participantes son referidos como recompensas basadas en protocolos (inflacionarios) y tasas de transacción. Las recompensas basadas en los protocolos son emisiones de una tasa de inflación global, definida por protocolos. Estas recompensas constituirán la recompensa total entregada a los clientes de validación, el resto se origina a partir de las comisiones de transacción. En los primeros días de la red, es probable que las recompensas basadas en protocolos, desplegadas en base al calendario de emisión predefinido, impulsará la mayoría de los incentivos de los participantes a participar en la red.

Estas recompensas basadas en protocolos, que se distribuirán entre los tokens que se encuentran activamente en stake en la red, deben ser el resultado de una tasa global de inflación de la oferta, calculada por épocas de Solana y distribuida entre el conjunto de validadores activos. Como se explica más adelante, la tasa de inflación anual se basa en un calendario desinflacionario predeterminado. Esto proporciona a la red una previsibilidad de la oferta monetaria que favorece la estabilidad y la seguridad económica a largo plazo.

Las comisiones por transacción son transferencias de participante a participante basadas en el mercado, que se adjuntan a las interacciones de la red como motivación y compensación necesarias para la inclusión y ejecución de una transacción propuesta. A continuación se analiza también un mecanismo de estabilidad económica a largo plazo y de protección del fork mediante la quema parcial de cada tasa de transacción.

A continuación se muestra un esquema de alto nivel del diseño criptoeconómico de Solana en **Figura 1**. Las especificaciones de la economía de validación-cliente se describen en las secciones: [Economía de validación-cliente](ed_validation_client_economics/ed_vce_overview.md), [Horario de Inflación](ed_validation_client_economics/ed_vce_state_validation_protocol_based_rewards.md)y [Tarifas de Transacción](ed_validation_client_economics/ed_vce_state_validation_transaction_fees.md). Además, la sección titulada [Delegación de toma de validación](ed_validation_client_economics/ed_vce_validation_stake_delegation.md) cierra con una discusión sobre oportunidades de delegación de validadores y mercado. Adicionalmente, en [Economía de Almacenamiento](ed_storage_rent_economics.md), describimos una implementación de alquiler de almacenamiento para responder de los costos de externalidad de mantener el estado activo del ledger. Se discute un esquema de características para un diseño económico MVP en la sección de [Diseño Económico MVP](ed_mvp.md).

![](/img/economic_design_infl_230719.png)

**Figura 1**: Descripción general del diseño de incentivo económico Solana.
