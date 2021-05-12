---
title: Recompensas y Delegación de Stake
---

Los participantes son recompensados por ayudar a validar el registro. Lo hacen delegando su stake en nodos validadores. Esos validadores hacen el trabajo preliminar de reproducir el ledger y enviar votos a una cuenta de voto por nodo en la que los participantes pueden delegar su stake. El resto del grupo utiliza esos votos ponderados por stake para seleccionar un bloque cuando surgen forks. Tanto el validador como el staker necesitan algún incentivo económico para desempeñar su papel. El validador debe ser compensado por su hardware y el staker debe ser compensado por el riesgo de que se reduzca su stake. Los aspectos económicos se tratan en [ recompensas de stake ](../implemented-proposals/staking-rewards.md). Esta sección, por otra parte, describe los mecanismos subyacentes de su implementación.

## Diseño básico

La idea general es que el validador posee una cuenta de voto. La cuenta de voto rastrea los votos del validador, cuenta los créditos generados por el validador, y proporciona cualquier estado específico del validador. La cuenta de voto no tiene conocimiento de ningun stake delegado en ella y no tiene peso de staking.

Una cuenta de stake separada \ (creada por un participante \) nombra una cuenta de votación a la que se delega el stake. Las recompensas generadas son proporcionales a la cantidad de lamports en stake. La cuenta Stake es propiedad del participante solamente. Parte de los lamports almacenados en esta cuenta son el stake.

## Delegación Pasiva

Cualquier número de cuentas Stake puede delegar a una sola cuenta de voto sin una acción interactiva de la identidad que controla la cuenta de voto o envía votos a la cuenta.

El stake total asignado a una cuenta de voto puede ser calculada por la suma de todas las cuentas de Stake que tienen la pubkey de la cuenta de Voto como `StakeState::Stake::voter_pubkey`.

## Cuentas de voto y Stake

El proceso de recompensas se divide en dos programas en cadena. El programa Vote resuelve el problema de hacer que los stakes sean cortados. El programa Stake actúa como custodio del grupo de recompensas y prevé la delegación pasiva. El programa Stake es responsable de pagar recompensas a los interesados y votantes cuando se muestra que el delegado de un participante ha participado en la validación del titular.

### Estado de la votación

Estado de voto es el estado actual de todos los votos que el validador ha enviado a la red. El Estado de voto contiene la siguiente información de estado:

- `votes` - La estructura de datos de los votos enviados.
- `credits` -El número total de recompensas que este programa de Voto ha generado a lo largo de su vida.
- `root_slot` - La última ranura para alcanzar el compromiso completo de bloqueo necesario para obtener recompensas.
- `comisión` - La comisión tomada por este Estado del Voto para cualquier recompensa reclamada por las cuentas de stake del participante. Este es el porcentaje máximo de la recompensa.
- Account::lamports: los lamports acumulados de la comisión. Estos no cuentan como participaciones.
- `authorized_voter` - solo esta identidad está autorizada para enviar votos. Este campo sólo puede ser modificado por esta identidad.
- `node_pubkey` - El nodo Solana que vota en esta cuenta.
- ` Authorized_withdrawer `: la identidad de la entidad a cargo de los importes de esta cuenta, separada de la dirección de la cuenta y del firmante de voto autorizado.

### VoteInstruction::Initialize\(VoteInit\)

- `account[0]` - RW - El Estado del voto.

  `VoteInit` lleva la nueva cuenta de voto `node_pubkey`, `authorized_voter`, `authorized_withdrawal`y `comisión`.

  otros miembros del Estado con Voto incumplieron.

### VoteInstruction::Authorize\(Pubkey, VoteAuthorize\)

Actualiza la cuenta con un nuevo votante o retiro autorizado, según el parámetro VoteAuthorize \(`Voter` o `Withdrawer`\). La transacción debe estar firmada por la cuenta de voto actual `authorized_voter` o `authorized_withdrawal` de la cuenta de voto.

- `account[0]` - RW - El Estado del voto. `VoteState::authorized_voter` o `authorized_withdrawal` está establecido en `Pubkey`.

### VoteInstruction::Vote\(Vote\)

- `account[0]` - RW - El estado del Voto. `VoteState::lockouts` y `VoteState::credits` se actualizan de acuerdo a las reglas de bloqueo de votación ver [Tower BFT](../implemented-proposals/tower-bft.md).
- `account[1]` - RO - `sysvar::slot_hashes` Una lista de algunas N ranuras más recientes y sus hash para que la votación se verifique en su lugar.
- `account[2]` - RO - `sysvar::clock` El tiempo actual de red, expresado en ranuras, epochs.

### Estado del Stake

Un StakeState toma uno de cuatro formularios, StakeState::Uninitialized, StakeState::Initialized, StakeState::StakeState::Stake, y StakeState::RewardsPool. Sólo las tres primeras formas se utilizan en e lstaking, pero sólo StakeState::Stake es interesante. Todas las RewardsPools se crean en génesis.

### StakeState::Stake

StakeState::Stake es la preferencia actual de la delegación de la **staker** y contiene la siguiente información de estado:

- Account::lamports - Los lamports disponibles para hacer staking.
- `stake` - la cantidad puesta en stake \(sujeto a warmup y enfriamiento\) para generar recompensas, siempre menor o igual a Account::lamports.
- `voter_pubkey` - La llave de la instancia de VoteState a la que se delegan los lamports.
- `credits_observed` - Los créditos totales reclamados durante la vida del programa.
- ` activated `: la época en la que se activó / delegó este stake. El stake total se contará después de warmup.
- `deactivated` -la época en la que se desactivó este stake, se requieren algunas épocas de enfriamiento antes de que la cuenta se desactive por completo, y el stake disponible para el retiro.
- `authorized_staker` - la pubkey de la entidad que debe firmar las transacciones de delegación, activación y desactivación.
- `authorized_wither` - la identidad de la entidad responsable de los lamports de esta cuenta, separados de la dirección de la cuenta y el participante autorizado.

### StakeState::Fondo de recompensas

Para evitar un único bloqueo o contención en toda la red, 256 RewardsPools son parte de la génesis bajo claves predeterminadas, cada uno con std::u64::MAX créditos para poder satisfacer canjeos de acuerdo al valor del punto.

Los Stakes y RewardsPool son cuentas que pertenecen al mismo programa `Stake`.

### Instrucción de Stake::Delegar Stake

La cuenta de stake se mueve de la forma Inicializada a la forma StakeState::Stake, o de una StakeState::Stake desactivada (es decir, totalmente enfriada) a una StakeState::Stake activada. Así es como los stakers eligen la cuenta de voto y el nodo validador al que se delegan los lamports de su cuenta de stake. La transacción debe ser firmada por el `authorized_staker` del stake.

- `account[0]` - RW - The StakeState::Stake instance. `StakeState::Stake::credits_observed` se inicializa en `VoteState::credits`, `StakeState::Stake::voter_pubkey` se inicializa en `cuenta[1]`. Si esta es la delegación inicial del stake, `El estado de stake::Stake::bet` se inicializa al saldo de la cuenta en lamports, `StakeState::Stake::activated` está inicializado en la época actual del Banco, y `StakeState::Stake::deactivated` está inicializado a std::u64::MAX
- `account[1]` - R - La instancia de VoteState.
- `cuenta[2]` - R - cuenta sysvar::clock porta información sobre epoch bancario actual.
- `cuenta[3]` - R - cuenta sysvar::stakehistory, lleva información sobre el historial de estaciones.
- `cuenta[4]` - R - stake::Cuenta de configuración, lleva configuración warmup, tiempo de recarga y configuración de Slashing.

### StakeInstruction::Authorize\(Pubkey, StakeAuthorize\)

Actualiza la cuenta con un nuevo staker o retiro autorizado, según el parámetro StakeAuthorize \(`Staker` o `Withdrawer`\). La transacción debe estar firmada por la cuenta actual de Stakee `authorized_staker` o `authorized_withdrawal` de la cuenta de participantes. Cualquier bloqueo de participación debe haber expirado, o el custodio del bloqueo también debe firmar la transacción.

- `account[0]` - RW - El estado de participación.

  `StakeState::authorized_staker` o `authorized_Remove` se establece en `Pubkey`.

### StakeInstruction::Desactivar

Un participante puede querer retirarse de la red. Para hacerlo, primero debe desactivar su stake y esperar a que se enfrie. La transacción debe estar firmada por la `authorized_staker` del stake.

- `account[0]` - RW - La instancia de StakeState::Stake que está desactivando.
- `account[1]` - R - cuenta sysvar::clock del Banco que lleva la época actual.

StakeState::Stake::deactivated está establecido en el epoch + enfriamiento actual. La apuesta de la cuenta se reducirá a cero por esa época, y Account::lamports estará disponible para retiro.

### StakeInstruction::Withdraw\(u64\)

Los lamports se acumulan a lo largo del tiempo en una cuenta de Stake y se puede retirar cualquier exceso de stake activado. La transacción debe estar firmada por el `authorized_withdrawal` de la participación.

- `cuenta[0]` - RW - El StakeState::Stake desde el cual retirarse.
- `cuenta[1]` - RW - Cuenta que debe ser acreditada con los lamports retirados.
- `cuenta[2]` - R - cuenta sysvar::clock del Banco que lleva la época actual, para calcular el stake.
- `cuenta[3]` - R - cuenta sysvar::stake_history del Banco que lleva la historia warmup/cooldown de la inversión.

## Beneficios del diseño

- Voto único para todos los participantes.
- La eliminación de la variable de crédito no es necesaria para reclamar recompensas.
- Cada stake delegada puede reclamar sus recompensas de forma independiente.
- La comisión para el trabajo se deposita cuando el stake delegado reclama una recompensa.

## Ejemplo de flujo de llamadas

![Flujo de llamada pasivo](/img/passive-staking-callflow.png)

## Recompensas de Staking

Aquí se describen los mecanismos específicos y las reglas del régimen de recompensas del validador. Las recompensas se ganan delegando el stake a un validador que vota correctamente. La votación expone incorrectamente ese stake del validador a [slashing](../proposals/slashing.md).

### Básicos

La red paga recompensas de una porción de la inflación [de la red](../terminology.md#inflation). El número de lamports disponibles para pagar las recompensas de una época es fijo y debe repartirse equitativamente entre todos los nodos puestos en stake en función de su peso relativo en stake y su participación. La unidad de pesaje se llama un [punto](../terminology.md#point).

Las recompensas por un epicentro no están disponibles hasta el final de esa época.

Al final de cada época, el número total de puntos ganados durante el epicentro se resume y se utiliza para dividir la porción de recompensas de la inflación de épocas para llegar a un valor punto. Este valor se registra en el banco en un [sysvar](../terminology.md#sysvar) que mapea épocas a valores puntuales.

Durante la redención, el programa de stake cuenta los puntos ganados por stake por cada época, multiplica ese valor por el valor de puntos de las épocas. y transfiere lamports en esa cantidad de una cuenta de premios a las cuentas de stake y votos de acuerdo a la configuración de la comisión de la cuenta de voto.

### Economía

El valor de punto para una época depende de la participación de la red agregada. Si la participación en una época cae, los valores de los puntos son más altos para aquellos que sí participan.

### Ganar créditos

Los validadores ganan un crédito de voto por cada voto correcto que supere el bloqueo máximo, es decir, cada vez que la cuenta de votos del validador retira una ranura de su lista de bloqueo, convirtiendo ese voto en raíz para el nodo.

Los participantes que han delegado a ese validador ganan puntos en proporción a su stake. Los puntos ganados son el producto de los créditos de voto y el stake.

### Calentamiento, enfriamiento y retiro de stake

Los stake, una vez delegados, no se hacen efectivas de inmediato. Primero deben pasar por un período de calentamiento. Durante este período, una parte del stake se considera "efectiva", el resto se considera "activando". Los cambios ocurren en los límites de épocas.

El programa de stake limita la tasa de cambio a la participación total en la red, reflejado en el programa de stake `config::warmup_rate` \\(establecido a 25% por época en la implementación actual\).

La cantidad de stake que se puede calentar en cada época es una función del stake efectivo total de la época anterior, total de activación de stake, y la tasa de calentamiento configurada por el programa de stake.

El tiempo de enfriamiento funciona de la misma forma. Una vez que un stake es desactivada, parte de el se considera "efectiva", y también "desactivando". A medida que el stake se enfría, sigue ganando recompensas y se expone a un slashing, pero también está disponible para retirarse.

Las apuestas de Bootstrap no están sujetas a calentamiento.

Las recompensas se pagan contra la porción "efectiva" del stake por esa época.

#### Ejemplo de Calentamiento

Consideremos la situación de un solo stake de 1.000 es activada en la época N, con velocidad warmup de red de 20%, y una quiescente participación total de red la época N de 2.000.

En la época N+1, la cantidad disponible para activar para la red es 400 \(20% de 2000\), y en la época N, este stake de ejemplo es el único que se activa, y por lo tanto tiene derecho a toda la sala warmup disponible.

| época | efectiva | activando | efectivo total | activación total |
|:----- | --------:| ---------:| --------------:| ----------------:|
| N-1   |          |           |          2,000 |                0 |
| N     |        0 |     1,000 |          2,000 |            1,000 |
| N+1   |      400 |       600 |          2,400 |              600 |
| N+2   |      880 |       120 |          2,880 |              120 |
| N+3   |     1000 |         0 |          3,000 |                0 |

Si se activaran 2 stakes \(X y Y\) en la época N, se les concedería una porción del 20% en proporción a su stake. En cada época efectiva y activada para cada stake es una función del estado anterior de época.

| época | X eff | X act | Y eff | Y act | efectivo total | activación total |
|:----- | -----:| -----:| -----:| -----:| --------------:| ----------------:|
| N-1   |       |       |       |       |          2,000 |                0 |
| N     |     0 | 1,000 |     0 |   200 |          2,000 |            1,200 |
| N+1   |   333 |   667 |    67 |   133 |          2,400 |              800 |
| N+2   |   733 |   267 |   146 |    54 |          2,880 |              321 |
| N+3   |  1000 |     0 |   200 |     0 |          3,200 |                0 |

### Retiro

Sólo se pueden retirar en cualquier momento los lamports que superen el stake activado. Esto significa que durante el warmup, en realidad no se puede retirar ninguna participación. Durante el tiempo de enfriamiento, cualquier token que supere el stake efectivo puede ser retirado \(activando == 0\). Debido a que las recompensas ganadas se añaden automáticamente al stake, el retiro sólo es posible después de la desactivación.

### Bloqueo

Las cuentas de stake apoyan la noción de bloqueo, en el que el saldo de la cuenta de stake no está disponible para retiro hasta un momento específico. El bloqueo se especifica como una altura de época es decir, la altura de época mínima que debe alcanzar la red antes de que el saldo de la cuenta de stake esté disponible para retiro, a menos que la transacción también esté firmada por un custodio especificado. Esta información se recopila cuando se crea la cuenta de stake y se almacena en el campo Bloqueo del estado de la cuenta de stake. Cambiar el participante o retiro autorizado también está sujeto a bloqueo, ya que dicha operación es efectivamente una transferencia.
