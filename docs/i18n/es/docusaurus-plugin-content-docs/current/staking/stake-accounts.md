---
title: Estructura de Cuenta Stake
---

Una cuenta stake en Solana puede ser usada para delegar tokens a validadores en la red para potencialmente ganar recompensas por el dueño de la cuenta de stake. Las cuentas de stake se crean y gestionan de forma diferente a una dirección de billetera tradicional, conocida como *cuenta del sistema*.  Una cuenta del sistema sólo puede enviar y recibir SOL de otras cuentas de la red, mientras que una cuenta de stake apoya operaciones más complejas necesarias para gestionar una delegación de tokens.

Las cuentas de Stake en Solana también funcionan de forma diferente a las de otras redes de blockchain de Prueba-de-Stake con las que puede estar familiarizado.  Este documento describe la estructura de alto nivel y las funciones de una cuenta stake de Solana.

#### Dirección de cuenta
Cada cuenta de stake tiene una dirección única que puede utilizarse para buscar la información de la cuenta en la línea de comandos o en cualquier herramienta de explorador de red.  Sin embargo, a diferencia de una dirección de billetera en la que el titular del keypair de la dirección controla la billetera, el keypair asociado a una dirección de cuenta de stake no tiene necesariamente ningún control sobre la cuenta.  De hecho, un keypair o clave privada puede ni siquiera existir para la dirección de una cuenta de stake.

La única vez que la dirección de una cuenta de stake tiene un archivo de par de claves es cuando [creas una cuenta de stake usando las herramientas de línea de comandos](../cli/delegate-stake.md#create-a-stake-account), un nuevo archivo keypair es creado primero para asegurarse de que la dirección de la cuenta de stake es nueva y única.

#### Entendiendo las autoridades de las cuentas
Ciertos tipos de cuentas pueden tener una o más *autoridades de firma* asociadas con una cuenta determinada. Se utiliza una autoridad de una cuenta para firmar ciertas transacciones para la cuenta que controla.  Esto es diferente de otras redes de blockchain donde el titular del keypair asociado con la dirección de la cuenta controla toda la actividad de la cuenta.

Cada cuenta de stake tiene dos autoridades de firma especificadas por su respectiva dirección, cada una de las cuales está autorizada a realizar determinadas operaciones en la cuenta de stake.

La *autoridad de participación* se utiliza para firmar transacciones para las siguientes operaciones:
 - Delegando Stake
 - Desactivando la delegación de stake
 - Dividiendo la cuenta del monto, creando una nueva cuenta con una porción de los fondos en la primera cuenta
 - Combinar dos cuentas de stake no delegadas en una
 - Establecer una nueva autoridad de stake

La autoridad de retiro ** firma transacciones para lo siguiente:
 - Retirar el stake no delegado en una dirección de billetera
 - Establecer una nueva autoridad de retiro
 - Establecer una nueva autoridad de stake

La autoridad de la stake y la autoridad de retiro se establecen cuando se crea la cuenta de stake, y se pueden cambiar para autorizar una nueva dirección de firma en cualquier momento. La autoridad de hacer stake y retirar pueden ser la misma dirección o dos direcciones diferentes.

El keypair de la autoridad de retiro tiene más control sobre la cuenta, ya que es necesaria para liquidar los tokens de la cuenta de stake, y puede utilizarse para restablecer la autoridad de stake si el keypair de la autoridad de stake se pierde o se ve comprometido.

Asegurar la autoridad de retiro contra pérdida o robo es de suma importancia al administrar una cuenta de stake.

#### Delegaciones múltiples
Cada cuenta de stake sólo se puede utilizar para delegar a un validador a la vez. Todos los tokens en la cuenta son delegados o des-delegados, o en el proceso de delegar o des-delegar.  Para delegar una fracción de tus tokens a un validador, o para delegar a múltiples validadores, debes crear cuentas de stake múltiples.

Esto puede lograrse creando varias cuentas de stake a partir de una dirección de una billetera que contenga algunos tokens, o creando una única cuenta de stake grande y utilizando la autoridad de stake para dividir la cuenta en varias cuentas con saldos de tokens de su elección.

Las mismas autoridades de stake y retiro pueden asignarse a múltiples cuentas de stake.

Dos cuentas de stake que no están delegadas y que tienen las mismas autoridades y que bloquean pueden fusionarse en una sola cuenta de stake resultante.

#### Calentamiento y enfriamiento de la delegación
Cuando se delega una cuenta de stake o se desactiva una delegación, la operación no tiene efecto inmediatamente.

Una delegación o desactivación tarda varias [épocas](../terminology.md#epoch) para completar, con una fracción de la delegación volviéndose activa o inactiva en cada límite de época después de que la transacción que contiene las instrucciones haya sido enviada al clúster.

También hay un límite en la cantidad total de stake que puede delegarse o desactivarse en una sola época, para evitar grandes cambios repentinos en el stake en el conjunto de la red. Como el calentamiento y el enfriamiento dependen del comportamiento de otros participantes en la red, su duración exacta es difícil de predecir. Los detalles sobre el tiempo de calentamiento y enfriamiento se pueden encontrar [aquí](../cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal).

#### Bloqueos
Las cuentas de stake pueden tener un bloqueo que impida retirar los tokens que poseen antes de que se alcance una fecha o época determinada.  Mientras se bloquea, la cuenta de stake aún puede ser delegada, no delegada, o dividida, y las autoridades de su stake y retiro pueden cambiarse normalmente.  Sólo retiro a una dirección de billetera no está permitido.

Un bloqueo solo puede ser añadido cuando se crea por primera vez una cuenta de stake, pero puede ser modificado más tarde por la *autoridad de bloqueo* o *custodio*, la dirección de la cual también se establece cuando se crea la cuenta.

#### Destruyendo una cuenta Stake
Al igual que otros tipos de cuentas en la red Solana, ya no se rastrea una cuenta de stake que tiene un saldo de 0 SOL.  Si una cuenta de stake no está delegada y todos los tokens que contiene se retiran a una dirección de billetera, la cuenta en esa dirección es efectivamente destruida, y tendrá que ser recreada manualmente para que la dirección sea utilizada de nuevo.

#### Viendo cuentas Stake
Los detalles de la cuenta pueden verse en el Explorador Solana copiando y pegando una dirección de cuenta en la barra de búsqueda.
 - http://explorer.solana.com/accounts
