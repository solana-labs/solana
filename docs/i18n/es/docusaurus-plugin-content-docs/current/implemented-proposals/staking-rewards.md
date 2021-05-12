---
title: Recompensas de Staking
---

Aquí se describe un diseño de Proof of Stake (PoS\), (es decir, el uso de activos en el protocolo, SOL, para proporcionar un consenso seguro.) el diseño se describe aquí. Solana implementa un esquema proof of stake de recompensa/seguridad para los nodos validadores del clúster. El propósito es triple:

- Alinear incentivos del validador con el del grupo mayor a través

  de depósitos en juego bajo riesgo

- Evitar cuestiones de "nada en stake" en el fork mediante la aplicación de reglas de slashing

  destinado a promover la convergencia del fork

- Proporcionar una vía para las recompensas del validador proporcionadas como una función del validador

  de la participación en el cluster.

Aunque muchos de los detalles de la implementación específica están siendo considerados actualmente y se espera que se concentren a través de estudios específicos de modelado y exploración de parámetros en la red de pruebas Solana, esbozamos aquí nuestro pensamiento actual sobre los principales componentes del sistema PoS. Gran parte de este pensamiento se basa en el estado actual de Casper FFG, con optimizaciones y atributos específicos a ser modificados como es permitido por la estructura de datos de la Prueba de Historia de Solana \(PoH\).

## Resumen general

El diseño de validación del ledger de Solana se basa en una transacción de transmisión líder giratoria, ponderada por participantes, en una estructura de datos PoH para validar nodos. Estos nodos, al recibir la transmisión del líder, tener la oportunidad de votar sobre el estado actual y altura PoH firmando una transacción en el flujo de PoH.

Para convertirse en un validador de Solana, uno debe depositar o bloquear alguna cantidad de SOL en un contrato. Este SOL no será accesible por un período de tiempo específico. No se ha determinado la duración exacta del periodo de bloqueo de staking. Sin embargo, podemos considerar tres fases de este tiempo para las que serán necesarios parámetros específicos:

- _Warm-up period_: donde SOL es depositado e inaccesible para el nodo,

  sin embargo, la validación de la transacción PoH no ha comenzado. Probablemente por orden de

  días a semanas

- _Período de validación_: duración mínima durante la cual el SOL depositado será

  inaccesible, con riesgo de slashing \(ver reglas de slashing abajo\) y ganar

  recompensas para la participación del validador. Probablemente la duración sea de meses a un

  año.

- _Período de enfriamiento_: duración del tiempo después de la presentación de una

  Transacción de "retiro". Durante este período se han eliminado las responsabilidades de

  validación y los fondos siguen siendo inaccesibles. Las Recompensas acumuladas

  debe entregarse al final de este período, junto con la devolución del

  depósito inicial.

El sentido sin confianza del tiempo y el ordenamiento de Solana proporcionado por su estructura de datos PoH, junto con su difusión y diseño de transmisión de datos de [turbina](https://www.youtube.com/watch?v=qt_gDRXHrHQ&t=1s) y su diseño de transmisión proporciona tiempos de confirmación de la transacción de sub-segundo que escalan con el registro del número de nodos en el cluster. Esto significa que no deberíamos tener que restringir el número de nodos de validación con un prohibitivo 'depósitos mínimos' y esperar que los nodos sean capaces de convertirse en validadores con cantidades nominales de SOL en stake. Al mismo tiempo, el enfoque de Solana en un alto rendimiento debería crear incentivos para que los clientes de validación proporcionen hardware confiable y de alto rendimiento. Combinados con el potencial de un umbral mínimo de velocidad de red para unirse como un cliente de validación, esperamos que surja un mercado de delegaciones de validación saludable. Con este fin, la red de pruebas de Solana conducirá a una competición de validación-cliente "Tour de SOL", centrándose en el rendimiento y tiempo de puesta en marcha para clasificarse y recompensar a los validadores de la red de pruebas.

## Multas

Como se explica en la sección [Diseño Económico](ed_overview/ed_overview.md), las tasas de interés anuales del validador deben especificarse como una función del porcentaje total de oferta circulante que ha sido puesto en stake. El clúster premia a los validadores que están en línea y participan activamente en el proceso de validación a lo largo de todo su _periodo de validación_. Para los validadores que no convaliden o no validen transacciones durante este período, su recompensa anual se reduce efectivamente.

Del mismo modo, podemos considerar una reducción algorítmica de la cantidad activa en stake de un validador en el caso de que estén fuera de línea. Es decir. si un validador está inactivo durante algún tiempo, ya sea debido a una partición o de otro modo, la cantidad de su stake que se considera "activo" \(elegible para ganar recompensas\) puede ser reducida. Este diseño estaría estructurado para ayudar a las particiones de larga duración a llegar finalmente a la finalidad en sus respectivas cadenas, ya que el % del stake total sin voto se reduce con el tiempo hasta que los validadores activos puedan lograr una supermayoría en cada partición. De la misma manera, una vez reactivado, la cantidad “activa” en stake volverá a estar en línea a una tasa definida. Se pueden considerar diferentes tasas de reducción de stake dependiendo del tamaño del conjunto de particiones/activos.
