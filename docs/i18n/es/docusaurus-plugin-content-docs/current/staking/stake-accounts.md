---
title: Estructura de Cuenta Stake
---

Una cuenta stake en Solana puede ser usada para delegar tokens a validadores en la red para potencialmente ganar recompensas por el dueño de la cuenta de stake. Stake accounts are created and managed differently than a traditional wallet address, known as a _system account_. Una cuenta del sistema sólo puede enviar y recibir SOL de otras cuentas de la red, mientras que una cuenta de stake apoya operaciones más complejas necesarias para gestionar una delegación de tokens.

Las cuentas de Stake en Solana también funcionan de forma diferente a las de otras redes de blockchain de Prueba-de-Stake con las que puede estar familiarizado. Este documento describe la estructura de alto nivel y las funciones de una cuenta stake de Solana.

#### Dirección de cuenta

Cada cuenta de stake tiene una dirección única que puede utilizarse para buscar la información de la cuenta en la línea de comandos o en cualquier herramienta de explorador de red. Sin embargo, a diferencia de una dirección de billetera en la que el titular del keypair de la dirección controla la billetera, el keypair asociado a una dirección de cuenta de stake no tiene necesariamente ningún control sobre la cuenta. De hecho, un keypair o clave privada puede ni siquiera existir para la dirección de una cuenta de stake.

La única vez que la dirección de una cuenta de stake tiene un archivo de par de claves es cuando [creas una cuenta de stake usando las herramientas de línea de comandos](../cli/delegate-stake.md#create-a-stake-account), un nuevo archivo keypair es creado primero para asegurarse de que la dirección de la cuenta de stake es nueva y única.

#### Entendiendo las autoridades de las cuentas

Certain types of accounts may have one or more _signing authorities_ associated with a given account. Se utiliza una autoridad de una cuenta para firmar ciertas transacciones para la cuenta que controla. Esto es diferente de otras redes de blockchain donde el titular del keypair asociado con la dirección de la cuenta controla toda la actividad de la cuenta.

Cada cuenta de stake tiene dos autoridades de firma especificadas por su respectiva dirección, cada una de las cuales está autorizada a realizar determinadas operaciones en la cuenta de stake.

The _stake authority_ is used to sign transactions for the following operations:

- Delegando Stake
- Desactivando la delegación de stake
- Dividiendo la cuenta del monto, creando una nueva cuenta con una porción de los fondos en la primera cuenta
- Merging two stake accounts into one
- Establecer una nueva autoridad de stake

The _withdraw authority_ signs transactions for the following:

- Retirar el stake no delegado en una dirección de billetera
- Establecer una nueva autoridad de retiro
- Establecer una nueva autoridad de stake

La autoridad de la stake y la autoridad de retiro se establecen cuando se crea la cuenta de stake, y se pueden cambiar para autorizar una nueva dirección de firma en cualquier momento. La autoridad de hacer stake y retirar pueden ser la misma dirección o dos direcciones diferentes.

El keypair de la autoridad de retiro tiene más control sobre la cuenta, ya que es necesaria para liquidar los tokens de la cuenta de stake, y puede utilizarse para restablecer la autoridad de stake si el keypair de la autoridad de stake se pierde o se ve comprometido.

Asegurar la autoridad de retiro contra pérdida o robo es de suma importancia al administrar una cuenta de stake.

#### Delegaciones múltiples

Cada cuenta de stake sólo se puede utilizar para delegar a un validador a la vez. Todos los tokens en la cuenta son delegados o des-delegados, o en el proceso de delegar o des-delegar. Para delegar una fracción de tus tokens a un validador, o para delegar a múltiples validadores, debes crear cuentas de stake múltiples.

Esto puede lograrse creando varias cuentas de stake a partir de una dirección de una billetera que contenga algunos tokens, o creando una única cuenta de stake grande y utilizando la autoridad de stake para dividir la cuenta en varias cuentas con saldos de tokens de su elección.

Las mismas autoridades de stake y retiro pueden asignarse a múltiples cuentas de stake.

#### Merging stake accounts

Two stake accounts that have the same authorities and lockup can be merged into a single resulting stake account. A merge is possible between two stakes in the following states with no additional conditions:

- two deactivated stakes
- an inactive stake into an activating stake during its activation epoch

For the following cases, the voter pubkey and vote credits observed must match:

- two activated stakes
- two activating accounts that share an activation epoch, during the activation epoch

All other combinations of stake states will fail to merge, including all "transient" states, where a stake is activating or deactivating with a non-zero effective stake.

#### Delegation Warmup and Cooldown

When a stake account is delegated, or a delegation is deactivated, the operation does not take effect immediately.

A delegation or deactivation takes several [epochs](../terminology.md#epoch) to complete, with a fraction of the delegation becoming active or inactive at each epoch boundary after the transaction containing the instructions has been submitted to the cluster.

There is also a limit on how much total stake can become delegated or deactivated in a single epoch, to prevent large sudden changes in stake across the network as a whole. Since warmup and cooldown are dependent on the behavior of other network participants, their exact duration is difficult to predict. Details on the warmup and cooldown timing can be found [here](../cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal).

#### Lockups

Stake accounts can have a lockup which prevents the tokens they hold from being withdrawn before a particular date or epoch has been reached. While locked up, the stake account can still be delegated, un-delegated, or split, and its stake and withdraw authorities can be changed as normal. Only withdrawal into a wallet address is not allowed.

A lockup can only be added when a stake account is first created, but it can be modified later, by the _lockup authority_ or _custodian_, the address of which is also set when the account is created.

#### Destroying a Stake Account

Like other types of accounts on the Solana network, a stake account that has a balance of 0 SOL is no longer tracked. If a stake account is not delegated and all of the tokens it contains are withdrawn to a wallet address, the account at that address is effectively destroyed, and will need to be manually re-created for the address to be used again.

#### Viewing Stake Accounts

Stake account details can be viewed on the Solana Explorer by copying and pasting an account address into the search bar.

- http://explorer.solana.com/accounts
