---
title: Staking en Solana
---

_Nota antes de leer: Todas las referencias a incrementos de valores son en términos absolutos con respecto al balance de SOL. Este documento no hace ninguna sugerencia sobre el valor monetario de SOL en ningún momento._

Hacer stake de tus tokens SOL en Solana es la mejor manera en que puedes ayudar a asegurar la red de blockchain más eficiente del mundo, y [gana recompensas](implemented-proposals/staking-rewards.md) por hacerlo! Las recompensas de inflación y red _NO_ están activadas actualmente en la red Beta Mainnet de Solana, pero pueden estar habilitadas en el futuro.

Solana es una red de prueba de participación (PoS) con delegaciones, lo que significa que cualquiera que posea tokens SOL puede elegir delegar algunos de sus SOL a uno o más validadores, quienes procesan las transacciones y ejecutan la red.

Delegar la participación es un modelo financiero de riesgo compartido y recompensa compartida que puede proporcionar rendimientos a los titulares de tokens delegados durante un largo período. Esto se consigue alineando los incentivos financieros de los titulares de token (delegadores) y los validadores a los que delegan.

Cuanto más participación tenga un validador en su poder, más a menudo este validador es elegido para escribir nuevas transacciones en el contador. Cuantas más transacciones escriba el validador, más recompensas ganarán ellos y sus delegados. Los validadores que configuran sus sistemas para poder procesar más transacciones a la vez, no solo ganan proporcionalmente más recompensas por hacerlo, también mantienen la red funcionando lo más rápido y fluido posible.

Los validadores incurren en costos ejecutando y manteniendo sus sistemas, y esto se transmite a los delegados en forma de una comisión cobrada como un porcentaje de las recompensas ganadas. Esta comisión es conocida como una _comisión_. A medida que los validadores ganan más recompensas, se les delega más stake, pueden competir entre sí para ofrecer la comisión más baja para sus servicios, con el fin de atraer más participación delegada.

Hay un riesgo de pérdida de tokens al hacer stake, a través de un proceso conocido como _slashing_. El slashing _NO_ esta habilitado actualmente en la red Mainnet Beta de Solana pero es posible que se habilite en el futuro. El slashing involucra la eliminación y destrucción automática de una porción de la participación delegada de un validador en respuesta al comportamiento malicioso intencional, tales como crear transacciones no válidas o censurar ciertos tipos de transacciones o participantes en la red. Si un validador es Cortado, todos los poseedores de fichas que han delegado la participación en el validador perderán una porción de su delegación. Si bien esto significa una pérdida inmediata para el titular de tokens, también es una pérdida de recompensas futuras para el validador debido a su reducción total de delegación.

Este es el objetivo de las recompensas de la red y el slashing, alinear los incentivos financieros de ambos validadores y de los poseedores de token, lo que a su vez ayuda a mantener la red segura, robusta y con el mejor rendimiento posible.

_Nota: Las recompensas de red para los stakers y validadores no están habilitadas actualmente en la Mainnet Beta._

_Nota: El slashing no está activado en la Beta de Mainnet en este momento._

## ¿Cómo puedo hacer Stake de mis fichas SOL?

Para poder hacer stake de tokens en Solana, primero tendrás que transferir algo de SOL a una billetera que soporte el staking, luego siga los pasos o instrucciones proporcionados por la cartera para crear una cuenta de stake y delegar su stake. Los diferentes monederos varían ligeramente en su proceso para esto, pero la descripción general es la siguiente.

#### Carteras soportadas

Las operaciones de stake son soportadas por las siguientes soluciones de carteras:

- SolFlare.com junto con un archivo keystore o un Ledger Nano. Echa un vistazo a nuestra guía [para usar SolFlare](wallet-guide/solflare.md) para más detalles.

- Las herramientas de línea de comandos de Solana pueden realizar todas las operaciones de stake junto con un monedero de archivos de pares de claves generado por la CLI, un monedero de papel o con un Ledger Nano conectado. [comandos de staking utilizando las herramientas de línea de comandos de Solana](cli/delegate-stake.md).

#### Crear una cuenta Stake

Una cuenta stake es un tipo diferente de cuenta de una dirección de cartera que se utiliza para simplemente enviar y recibir fichas SOL a otras direcciones. Si usted ha recibido SOL en una dirección de cartera que controla, puedes usar alguno de estos tokens para crear y agregar fondos a una nueva cuenta de stake, que tendrá una dirección diferente a la que usó para crearla. Dependiendo de qué monedero esté utilizando los pasos para crear una cuenta stake pueden variar considerablemente. No todas las billeteras soportan cuentas Stake, consulte [Monederos soportados](#supported-wallets).

#### Seleccione un Validador

Después de crear una cuenta Stake, probablemente deseara delegar el SOL en un nodo de validador. A continuación encontrará algunos lugares donde puede obtener información sobre los validadores que actualmente están participando en el funcionamiento de la red. El equipo de Solana Labs y la Fundación Solana no recomiendan ningún validador en particular.

Los validadores de la Beta Mainnet se presentan a sí mismos y sus servicios en este hilo del Foro Solana:

- https://forums.solana.com/t/validator-information-thread

El sitio solanabeach.io es construido y mantenido por uno de nuestros validadores. Staking Facilities. Proporciona una información gráfica de alto nivel sobre la red en su conjunto, así como una lista de cada validador y algunas estadísticas de rendimiento de cada uno.

- https://solanabeach.io

Para ver las estadísticas de producción de bloques, utilice las herramientas de la línea de comandos de Solana:

- `validadores de solana`
- `producción de bloques de solana`

El equipo de Solana no hace recomendaciones sobre cómo interpretar esta información. Los posibles delegadores deben hacer su propia diligencia.

#### Delegar tu Stake

Una vez que haya decidido a qué validador o validadores delegará, utilice un monedero compatible para delegar su cuenta de Stake a la dirección de cuenta del validador.

## Detalles de la cuenta Stake

Para obtener más información acerca de las operaciones y permisos asociados con una cuenta Stake, por favor consulte [Cuentas Stake](staking/stake-accounts.md)
