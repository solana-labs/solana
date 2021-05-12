---
title: Terminología
---

Los siguientes términos se utilizan a lo largo de la documentación.

## cuenta

Un archivo persistente dirigido por [clave pública](terminology.md#public-key) y con [lamports](terminology.md#lamport) de seguimiento de su vida.

## app

Una aplicación de front-end que interactúa con un clúster Solana.

## estado bancario

El resultado de interpretar todos los programas en el ledger a una altura de marca [](terminology.md#tick-height) determinada. Incluye al menos el conjunto de todas las [cuentas](terminology.md#account) que poseen [tokens nativos](terminology.md#native-tokens).

## bloque

Un conjunto contiguo de [entradas](terminology.md#entry) en el ledger cubierto por un [voto](terminology.md#ledger-vote). Un [líder](terminology.md#leader) produce como máximo un bloque por cada [espacio](terminology.md#slot).

## hash bloque

Un hash [resistente a la preimagen](terminology.md#hash) del ledger [](terminology.md#ledger) a una determinada [altura del bloque](terminology.md#block-height). Tomada desde la última [entrada id](terminology.md#entry-id) en la ranura

## altura del bloque

El número de [bloques](terminology.md#block) debajo del bloque actual. El primer bloque después de que el bloque [génesis](terminology.md#genesis-block) tenga una altura de uno.

## validador bootstrap

El primer [validador](terminology.md#validator) en producir un bloque [](terminology.md#block).

## Bloque CBC

El trozo cifrado más pequeño del ledger, un segmento de ledger cifrado estaría hecho de muchos bloques CBC. `ledger_segment_size / cbc_block_size` para ser exacto.

## cliente

Un nodo [](terminology.md#node) que utiliza el clúster [](terminology.md#cluster).

## clúster

Un conjunto de validadores [](terminology.md#validator) que mantienen un solo [ledger](terminology.md#ledger).

## tiempo de confirmación

La duración del reloj de pantalla entre un líder [](terminology.md#leader) creando una entrada de tick [](terminology.md#tick) y creando un [bloque confirmado](terminology.md#confirmed-block).

## bloque confirmado

Un bloque [](terminology.md#block) que ha recibido una [súper mayoría](terminology.md#supermajority) de [votos mayores](terminology.md#ledger-vote) con una interpretación del ledger que coincide con la del líder.

## plano de control

Una red gossip que conecta todos los [nodos](terminology.md#node) de un clúster [](terminology.md#cluster).

## período de enfriamiento

Algún número de [epochs](terminology.md#epoch) después de [stake](terminology.md#stake) se ha desactivado mientras está disponible progresivamente para su retiro. Durante este período, el stake se considera "desactivandose". Más información sobre: [warmup y enfriamiento](implemented-proposals/staking-rewards.md#stake-warmup-cooldown-withdrawal)

## crédito

Ver [crédito de voto](terminology.md#vote-credit).

## plano de datos

Una red multicast utilizada para validar eficientemente [entradas](terminology.md#entry) y obtener consenso.

## drone

Un servicio off-chain que actúa como un custodio para la clave privada de un usuario. Por lo general, sirve para validar y firmar transacciones.

## entrada

Una entrada en el [ledger](terminology.md#ledger) o un [tick](terminology.md#tick) o una [entrada de transacciones](terminology.md#transactions-entry).

## id de entrada

Una preimagen resistente [hash](terminology.md#hash) sobre el contenido final de una entrada, que actúa como el [identificador único global de la entrada](terminology.md#entry). El hash sirve como prueba de:

- La entrada que se está generando después de una duración del tiempo
- Las [transacciones](terminology.md#transaction) especificadas son las incluidas en la entrada
- La posición de la entrada con respecto a otras entradas en [ledger](terminology.md#ledger)

Ver [Prueba de Historia](terminology.md#proof-of-history).

## época

El tiempo, es decir, el número de [espacios](terminology.md#slot), para los cuales un [programa de líder](terminology.md#leader-schedule) es válido.

## cuenta de comisión

La cuenta de comisión en la transacción es la cuenta que paga el costo de incluir la transacción en el ledger. Esta es la primera cuenta en la transacción. Esta cuenta debe declararse como Lead-Write (escribible) en la transacción ya que el pago de la transacción reduce el saldo de la cuenta.

## finalidad

Cuando los nodos que representan 2/3 del [stake](terminology.md#stake) tienen un [raíz común](terminology.md#root).

## fork

Un [ledger](terminology.md#ledger) derivado de entradas comunes pero luego divergente.

## bloque génesis

El primer bloque [](terminology.md#block) de la cadena.

## configuración genesis

El archivo de configuración que prepara el [ledger](terminology.md#ledger) para el [bloque génesis](terminology.md#genesis-block).

## hash

Una huella digital de una secuencia de bytes.

## inflación

Un aumento de la oferta de tokens con el tiempo utilizado para financiar recompensas para la validación y para financiar el desarrollo continuo de Solana.

## instrucción

La unidad más pequeña de un [programa](terminology.md#program) que un [cliente](terminology.md#client) puede incluir en una [transacción](terminology.md#transaction).

## keypair

Una [clave pública](terminology.md#public-key) y la correspondiente [clave privada](terminology.md#private-key).

## lamport

Un [token nativo ](terminology.md#native-token) fraccional con el valor de 0.000000001 [sol](terminology.md#sol).

## líder

El rol de un [validador](terminology.md#validator) cuando está adjuntando [entradas](terminology.md#entry) al [ledger](terminology.md#ledger).

## programación de líder

A sequence of [validator](terminology.md#validator) [public keys](terminology.md#public-key) mapped to [slots](terminology.md#slot). El clúster utiliza el programa de líder para determinar qué validador es el [líder](terminology.md#leader) en cualquier momento.

## ledger

Una lista de [entradas](terminology.md#entry) que contienen [transacciones](terminology.md#transaction) firmadas por [clientes](terminology.md#client). Conceptualmente, esto puede rastrearse hasta el bloque [génesis](terminology.md#genesis-block), pero el valor agregado real de [validadores](terminology.md#validator)sólo puede tener [bloques más nuevos](terminology.md#block) para ahorrar uso de almacenamiento como los antiguos no necesarios para validar bloques futuros por diseño.

## voto del ledger

Un hash [](terminology.md#hash) del estado del validador [](terminology.md#bank-state) a una altura de [tick](terminology.md#tick-height) dada. Contiene la afirmación del [validador](terminology.md#validator) de que se ha verificado un bloque [](terminology.md#block) que ha recibido, así como una promesa de no votar por un bloque [en conflicto](terminology.md#block) \(i.. [bifurcar](terminology.md#fork)\) por una cantidad específica de tiempo, el [bloqueo](terminology.md#lockout) período.

## cliente ligero

Un tipo de [cliente](terminology.md#client) que puede verificar que apunta a un clúster [válido](terminology.md#cluster). Realiza más verificación de ledge que un [cliente delgado](terminology.md#thin-client) y menor que un [validador](terminology.md#validator).

## cargador

Un programa [](terminology.md#program) con la capacidad de interpretar la codificación binaria de otros programas en cadena.

## bloqueo

La duración del tiempo durante la cual un validador [](terminology.md#validator) no puede [votar](terminology.md#ledger-vote) en otro [fork](terminology.md#fork).

## token nativo

El [token](terminology.md#token) usado para rastrear el trabajo realizado por [nodos](terminology.md#node) en un clúster.

## nodo

Una computadora que participa en un clúster [](terminology.md#cluster).

## número de nodos

El número de validadores [](terminology.md#validator) participando en un clúster [](terminology.md#cluster).

## PoH

Ver [Prueba de Historia](terminology.md#proof-of-history).

## punto

Un [crédito ponderado](terminology.md#credit) en un régimen de recompensas. En el [validador](terminology.md#validator) [régimen de recompensas](cluster/stake-delegation-and-rewards.md), el número de puntos adeudados a una[stake](terminology.md#stake) durante el canje es el producto de los [créditos de voto](terminology.md#vote-credit) ganados y el número de lamports en stake.

## clave privada

La clave privada de un keypair [](terminology.md#keypair).

## programa

El código que interpreta [instrucciones](terminology.md#instruction).

## id del programa

La clave pública de la cuenta [](terminology.md#account) que contiene un programa [](terminology.md#program).

## Prueba de historia

Un montón de pruebas, cada uno de los cuales demuestra que existían algunos datos antes de que se creara la prueba y que una duración exacta del tiempo pasado antes de la prueba anterior. Al igual que [VDF](terminology.md#verifiable-delay-function), una prueba de historia puede ser verificada en menos tiempo del que tomó producir.

## clave pública

La clave pública de un keypair [](terminology.md#keypair).

## root

Un bloque [](terminology.md#block) o un espacio [](terminology.md#slot) que ha alcanzado el máximo [bloqueo](terminology.md#lockout) en un [validador](terminology.md#validator). El root es el bloque más alto que es un ancestro de todos los forks activos de un validador. Todos los bloques ancestrales del root son también transitoriamente un root. Los bloques que no son antepasados y no descendientes de un root están excluidos de la consideración de consenso y pueden ser descartados.

## tiempo de ejecución

El componente de un [validador](terminology.md#validator) responsable de la ejecución del [programa](terminology.md#program).

## trozo

Una fracción de un bloque [](terminology.md#block), la unidad más pequeña enviada entre [validadores](terminology.md#validator).

## firma

Una firma ed25519 de 64 bytes de R (32-bytes) y S (32-bytes). Con el requisito de que R es un punto Edwards empaquetado no de orden pequeño y S es un escalar en el rango de 0 <= S < L. Este requisito asegura que no haya maleabilidad de la firma. Cada transacción debe tener al menos una firma para [cuenta de comisión](terminology#fee-account). Por lo tanto, la primera firma en la transacción puede ser tratada como [Id de transactón](terminology.md#transaction-id)

## skipped slot

A past [slot](terminology.md#slot) that did not produce a [block](terminology.md#block), because the leader was offline or the [fork](terminology.md#fork) containing the slot was abandoned for a better alternative by cluster consensus. A skipped slot will not appear as an ancestor for blocks at subsequent slots, nor increment the [block height](terminology#block-height), nor expire the oldest `recent_blockhash`.

Whether a slot has been skipped can only be determined when it becomes older than the latest [rooted](terminology.md#root) (thus not-skipped) slot.

## slot

The period of time for which each [leader](terminology.md#leader) ingests transactions and produces a [block](terminology.md#block).

Collectively, slots create a logical clock. Slots are ordered sequentially and non-overlapping, comprising roughly equal real-world time as per [PoH](terminology.md#proof-of-history).

## smart contract

A set of constraints that once satisfied, signal to a program that some predefined account updates are permitted.

## sol

The [native token](terminology.md#native-token) tracked by a [cluster](terminology.md#cluster) recognized by the company Solana.

## stake

Tokens forfeit to the [cluster](terminology.md#cluster) if malicious [validator](terminology.md#validator) behavior can be proven.

## supermajority

2/3 of a [cluster](terminology.md#cluster).

## sysvar

A synthetic [account](terminology.md#account) provided by the runtime to allow programs to access network state such as current tick height, rewards [points](terminology.md#point) values, etc.

## thin client

A type of [client](terminology.md#client) that trusts it is communicating with a valid [cluster](terminology.md#cluster).

## tick

A ledger [entry](terminology.md#entry) that estimates wallclock duration.

## tick height

The Nth [tick](terminology.md#tick) in the [ledger](terminology.md#ledger).

## token

A scarce, fungible member of a set of tokens.

## tps

[Transactions](terminology.md#transaction) per second.

## transaction

One or more [instructions](terminology.md#instruction) signed by the [client](terminology.md#client) using one or more [keypairs](terminology.md#keypair) and executed atomically with only two possible outcomes: success or failure.

## transaction id

The first [signature](terminology.md#signature) in a [transaction](terminology.md#transaction), which can be used to uniquely identify the transaction across the complete [ledger](terminology.md#ledger).

## transaction confirmations

The number of [confirmed blocks](terminology.md#confirmed-block) since the transaction was accepted onto the [ledger](terminology.md#ledger). A transaction is finalized when its block becomes a [root](terminology.md#root).

## transactions entry

A set of [transactions](terminology.md#transaction) that may be executed in parallel.

## validator

A full participant in the [cluster](terminology.md#cluster) responsible for validating the [ledger](terminology.md#ledger) and producing new [blocks](terminology.md#block).

## VDF

See [verifiable delay function](terminology.md#verifiable-delay-function).

## verifiable delay function

A function that takes a fixed amount of time to execute that produces a proof that it ran, which can then be verified in less time than it took to produce.

## vote

See [ledger vote](terminology.md#ledger-vote).

## vote credit

A reward tally for [validators](terminology.md#validator). A vote credit is awarded to a validator in its vote account when the validator reaches a [root](terminology.md#root).

## wallet

A collection of [keypairs](terminology.md#keypair).

## warmup period

Some number of [epochs](terminology.md#epoch) after [stake](terminology.md#stake) has been delegated while it progressively becomes effective. During this period, the stake is considered to be "activating". More info about: [warmup and cooldown](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
