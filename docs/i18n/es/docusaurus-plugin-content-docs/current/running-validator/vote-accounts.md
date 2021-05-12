---
title: Gestión de cuentas de voto
---

Esta página describe cómo configurar una cuenta de voto _en cadena_. Se necesita crear una cuenta de voto si planeas ejecutar un nodo de validador en Solana.

## Crear una cuenta de voto

Se puede crear una cuenta de voto con el comando [create-vote-account](../cli/usage.md#solana-create-vote-account). La cuenta de voto puede configurarse cuando se crea por primera vez o después de que el validador esté en ejecución. Todos los aspectos de la cuenta de voto pueden ser cambiados, excepto la [vote account address](#vote-account-address), el cual es fijo para toda la vida de la cuenta.

### Configurar una cuenta de voto existente

- Para cambiar la identidad del validador [](#validator-identity), usa [vote-update-validator](../cli/usage.md#solana-vote-update-validator).
- Para cambiar la [autoridad de voto](#vote-authority), usa [vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter).
- Para cambiar la [autoridad de retiro de votos](#withdraw-authority), utilice [vote-authorize-withdrawer](../cli/usage.md#solana-vote-authorize-withdrawer).
- Para cambiar la [comisión](#commission), usa [vote-update-commission](../cli/usage.md#solana-vote-update-commission).

## Estructura de Cuenta de Voto

### Dirección de cuenta de voto

Una cuenta de voto se crea en una dirección que es la clave pública de un archivo keypair, o en una dirección derivada basada en la clave pública de un archivo keypair y una cadena de semillas.

La dirección de una cuenta de voto nunca es necesaria para firmar ninguna transacción, pero solo se utiliza para buscar la información de la cuenta.

Cuando alguien quiere [delegar tokens en una cuenta de stake](../staking.md), el comando de delegación está dirigido a la dirección de cuenta de voto del validador a quien el titular del token quiere delegar.

### Identidad del validador

La _identidad del validador_ es una cuenta del sistema que se utiliza para pagar por todas las comisiones de transacción de voto enviadas a la cuenta de voto. Debido a que se espera que el validador vote en la mayoría de los bloques válidos que recibe, la cuenta de identidad del validador esta frecuentemente (potencialmente múltiples veces por segundo) firmando transacciones y pagando comisiones. Por esta razón, el keypair de identidad del validador debe ser almacenado como una "billetera caliente" en un archivo keypair en el mismo sistema que el proceso de validación se está ejecutando.

Debido a que una cartera caliente es generalmente menos segura que una cartera sin conexión o "fría", el operador del validador puede elegir almacenar solo suficiente SOL en la cuenta de identidad para cubrir las comisiones de votación por un tiempo limitado. tales como unas pocas semanas o meses. La cuenta de identidad del validador podría ser recargada periódicamente desde una cartera más segura.

Esta práctica puede reducir el riesgo de pérdida de fondos si el disco o el sistema de archivos del nodo validador se ponen en peligro o están dañados.

La identidad del validador es necesaria cuando se crea una cuenta de voto. La identidad del validador también se puede cambiar después de crear una cuenta usando el comando [vote-update-validator](../cli/usage.md#solana-vote-update-validator).

### Autoridad de voto

El keypair de la _vote authority_ se utiliza para firmar cada transacción de voto que el nodo del validador desea enviar al clúster. Esto no necesariamente tiene que ser único de la identidad del validador, como verás más adelante en este documento. Como la autoridad de voto, al igual que la identidad del validador, firma transacciones con frecuencia, también debe ser un keypair en caliente en el mismo sistema de archivos que el proceso del validador.

La autoridad de voto puede establecerse en la misma dirección que la identidad del validador. Si la identidad del validador es también la autoridad de voto, solo una firma por transacción de voto es necesaria para firmar el voto y pagar la comisión de la transacción. Debido a que las comisiones de transacción en Solana se evalúan por firma, tener un firmante en lugar de dos resultará en la mitad de la comisión de transacción pagada en comparación con ajustar la autoridad de voto y la identidad del validador a dos cuentas diferentes.

La autoridad de voto puede establecerse cuando se crea la cuenta de voto. Si no es proporcionado, el comportamiento predeterminado es asignarle la misma que la identidad del validador. La autoridad de voto puede ser cambiada más tarde con el comando [autorizar votantes](../cli/usage.md#solana-vote-authorize-voter).

La autoridad de votación puede cambiarse como máximo una vez por época. Si la autoridad es cambiada con [vote-authorize-voter](../cli/usage.md#solana-vote-authorize-voter), esto no tendrá efecto hasta el comienzo del siguiente época. Para soportar una transición suave de la firma de votos, `solana-validator` permite que el argumento `--authorized-voter` sea especificado varias veces. Esto permite que el proceso de validación siga votando con éxito cuando la red alcance un límite de época en el que la cuenta de autoridad del validador del voto del validador cambie.

### Autoridad de Retiro

El keypair _withdraw authority_ se utiliza para retirar fondos de una cuenta de votos mediante el comando [withdraw-from-vote-account](../cli/usage.md#solana-withdraw-from-vote-account). Cualquier recompensa de la red que gane un validador se deposita en la cuenta de votos y sólo se puede recuperar firmando con el keypai de la autoridad de retiro.

La autoridad de retiro también es necesaria para firmar cualquier transacción para cambiar la [comisión](#commission), de una cuenta de votos, y para cambiar la identidad del validador en una cuenta de votos.

Dado que la cuenta de votos podría acumular un saldo importante, considere la posibilidad de mantener el keypair de autoridad de retirada en un monedero offline/frío, ya que no es necesario para firmar transacciones frecuentes.

La autoridad de retiro puede establecerse en la creación de una cuenta de voto con la opción `--authorized-withdrawer`. Si esto no se proporciona, la identidad del validador se establecerá como la autoridad de retiro por defecto.

La autoridad de retiro se puede cambiar más tarde con el comando [de retiro de autorización de voto](../cli/usage.md#solana-vote-authorize-withdrawer).

### Comisión

_Comisión_ es el porcentaje de recompensas de red obtenidas por un validador que se deposita en la cuenta de voto del validador. El resto de las recompensas se distribuyen a todas las cuentas de stake delegadas a esa cuenta de voto. proporcional al peso activo del stake de cada cuenta de stake.

Por ejemplo, si una cuenta de voto tiene una comisión del 10%, por todas las recompensas ganadas por ese validador en una época dada, 10% de estas recompensas serán depositadas en la cuenta de voto en el primer bloque de la siguiente época. El 90% restante se depositará en cuentas de stake delegadas como stake. activa inmediatamente.

Un validador puede elegir establecer una comisión baja para intentar atraer más delegaciones de stake como resultado de una comisión más baja en un porcentaje mayor de recompensas pasado al delegado. Como hay costos asociados a configurar y operar un nodo de validador, un validador idealmente establecería una comisión suficientemente alta para al menos cubrir sus gastos.

La comisión puede establecerse al crear una cuenta de voto con la opción `--commission`. Si no se proporciona, el valor predeterminado será del 100%, lo que resultará en todos las recompensas depositadas en la cuenta de voto, y ninguno se transmite a ninguna cuenta de stake delegada.

La comisión también se puede cambiar más tarde con el comando [vote-update-commission](../cli/usage.md#solana-vote-update-commission).

Al establecer la comisión, sólo se aceptan valores enteros en el conjunto [0-100]. El entero representa el número de puntos porcentuales para la comisión, así que crear una cuenta con `--commission 10` establecerá una comisión del 10%.

## Rotación de clave

Rotar las claves de autoridad de la cuenta de voto requiere un manejo especial al tratar con un validador en vivo.

### Identidad del validador de cuentas de voto

Necesitarás acceder al keypair _withdraw authority_ de la cuenta de voto para cambiar la identidad del validador. Los siguientes pasos asumen que `~/withdraw-authority.json` es ese keypair.

1. Crear el nuevo keypair de identidad del validador, `solana-keygen new -o ~/new-validator-keypair.json`.
2. Asegúrese de que la nueva cuenta de identidad ha sido financiada, `solana transfer ~/new-validator-keypair.json 500`.
3. Ejecute `solana vote-update-validator ~/vote-account-keypair.json ~/new-validator-keypair.json ~/withdraw-authority.json` para modificar la identidad del validador en su cuenta de voto
4. Reinicia tu validador con el nuevo keypair de identidad para el argumento `--identity`

### Cuenta de voto Votante autorizado

El par de claves _vote authority_ sólo puede cambiarse en los límites de la época y requiere algunos argumentos adicionales a `solana-validator` para una migración sin problemas.

1. Ejecuta `solana epoch-info`. Si no queda mucho tiempo en la época actual, considere la posibilidad de esperar a la siguiente época para que su validador tenga tiempo suficiente para reiniciar y ponerse al día.
2. Crear el nuevo keypair de autoridad de votación, `solana-keygen new -o ~/new-vote-authority.json`.
3. Determine the current _vote authority_ keypair by running `solana vote-account ~/vote-account-keypair.json`. Puede ser la cuenta de identidad del validador (por defecto) o algún otro keypair. Los siguientes pasos asumen que `~/validator-keypair.json` es ese keypair.
4. Ejecute `solana vote-authorize-voter ~/vote-account-keypair.json ~/validator-keypair.json ~/new-vote-authority.json`. Está previsto que la nueva autoridad de votación se active a partir de la próxima época.
5. `solana-validator` debe reiniciarse ahora con los pares de claves de autoridad de voto antiguos y nuevos, para que pueda realizar la transición sin problemas en la siguiente época. Add the two arguments on restart: `--authorized-voter ~/validator-keypair.json --authorized-voter ~/new-vote-authority.json`
6. Después de que el clúster alcance la siguiente época, elimine el argumento `--authorized-voter ~/validator-keypair.json` y reinicie `solana-validator`, ya que el antiguo keypair de autoridad de voto ya no es necesario.

### Cuenta de voto de Retirada autorizada

No se requiere ninguna manipulación especial. Utilice el comando `solana vote-authorize-withdrawer` según sea necesario.
