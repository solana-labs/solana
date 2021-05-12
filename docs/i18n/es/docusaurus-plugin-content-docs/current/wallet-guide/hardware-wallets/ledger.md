---
title: Ledger Nano
---

Esta página describe cómo utilizar un Ledger Nano S o Nano X para interactuar con Solana usando las herramientas de línea de comandos. Para ver otras soluciones para interactuar con Solana con tu Nano, [haz clic aquí](../ledger-live.md#interact-with-the-solana-network).

## Antes de comenzar

- [Configura un Nano con la aplicación Solana](../ledger-live.md)
- [Instalar la suite de herramientas de línea de comandos Solana](../../cli/install-solana-cli-tools.md)

## Usar Nano Ledger con Solana CLI

1. Asegúrese de que la aplicación Ledger Live está cerrada
2. Conecta tu Nano al puerto USB de tu computadora
3. Introduce tu pin y inicia la aplicación Solana en el Nano
4. Asegúrate de que la pantalla dice "La aplicación está lista"

### Ver su ID de billetera

En tu ordenador, ejecutar:

```bash
pubkey solana-keygen usb://ledger
```

Esto confirma que tu dispositivo Ledger está conectado correctamente y en el estado correcto para interactuar con el CLI Solana. El comando devuelve el único _wallet ID_ de tu Ledger. Cuando tienes múltiples dispositivos Nano conectados al mismo ordenador, puede utilizar el ID de su billetera para especificar qué billetera hardware Ledger desea usar. Si solo planea usar un solo Nano en su computadora a la vez, no necesita incluir el ID de la billetera. Para obtener información sobre usando el ID de la billetera para usar un Ledger específico, consulte [Administrar Múltiples Carteras de hardware](#manage-multiple-hardware-wallets).

### Ver sus direcciones de billetera

Su Nano soporta un número arbitrario de direcciones y firmantes de billeteras válidos. Para ver cualquier dirección, utilice el comando `solana-keygen pubkey`, como se muestra a continuación, seguido de una [URL del keypair](../hardware-wallets.md#specify-a-keypair-url) válida.

Múltiples direcciones de billetera pueden ser útiles si desea transferir tokens entre sus propias cuentas para diferentes propósitos, o para utilizar diferentes keypairs en el dispositivo como autoridades firmantes de una cuenta de stake, por ejemplo.

Todos los siguientes comandos mostrarán diferentes direcciones, asociadas con la ruta del keypair dado. ¡Pruébalos!

```bash
solana-keygen pubkey usb://ledger
solana-keygen pubkey usb://ledger?key=0
solana-keygen pubkey usb://ledger?key=1
solana-keygen pubkey usb://ledger?key=2
```

- NOTA: los parámetros de la url del keypair se ignoran en **zsh** &nbsp;[Vea la solución de problemas para más información](#troubleshooting)

También puede utilizar otros valores para el número después de `key=`. Cualquiera de las direcciones mostradas por estos comandos son direcciones válidas del monedero Solana. La parte privada asociada a cada dirección se almacena de forma segura en el Nano, y se utiliza para firmar las transacciones desde esta dirección. Simplemente tenga en cuenta la URL del keypair que utilizó para derivar cualquier dirección que vaya a usar para recibir tokens.

Si solo planea utilizar una sola dirección/keypair en su dispositivo, una buena ruta fácil de recordar podría ser usar la dirección en `key=0`. Ver esta dirección con:

```bash
pubkey solana-keygen usb://ledger?key=0
```

Ahora tienes una dirección de billetera (o varias direcciones), puedes compartir cualquiera de estas direcciones públicamente para que actúe como dirección receptora, y puedes usar la URL del keypair asociado como firmante de las transacciones desde esa dirección.

### Ver su saldo

Para ver el saldo de cualquier cuenta, independientemente de la billetera que utilice, utiliza el comando `solana balance`:

```bash
solana balance SOME_WALLET_ADDRESS
```

Por ejemplo, si su dirección es `7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`, entonces introduzca el siguiente comando para ver el saldo:

```bash
solana balance 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri
```

También puede ver el saldo de cualquier dirección de cuenta en la pestaña de Cuentas en el [Explorador](https://explorer.solana.com/accounts) y pegar la dirección en el recuadro para ver el saldo en su navegador web.

Nota: Cualquier dirección con un saldo de 0 SOL, tal como una recién creada en su Ledger, se mostrará como "No Encontrado" en el explorador. Las cuentas vacías y las cuentas inexistentes son tratadas igual en Solana. Esto cambiará cuando la dirección de tu cuenta tenga algo de SOL en ella.

### Enviar SOL desde un Nano

Para enviar algunos tokens desde una dirección controlada por tu Nano, necesitarás usar el dispositivo para firmar una transacción, usando el mismo URL del keypair que usó para derivar la dirección. Para hacer esto, asegúrate de que tu Nano está conectado, desbloqueado con el PIN, Ledger Live no está funcionando, y la aplicación Solana está abierta en el dispositivo, mostrando "La aplicación está Lista".

El comando `solana transfer` se utiliza para especificar a qué dirección enviar tokens, cuántos tokens enviar, y utiliza el argumento `--keypair` para especificar cuál keypair está enviando los tokens, que firmará la transacción, y el saldo de la dirección asociada disminuirá.

```bash
solana transfer RECIPIENT_ADDRESS AMOUNT --keypair KEYPAIR_URL_OF_SENDER
```

A continuación se muestra un ejemplo completo. Primero, una dirección se ve en un determinado url keypair. En segundo lugar, el balance de la dirección tht está comprobado. Por último, se introduce una transacción de transferencia para enviar `1` SOL a la dirección del destinatario `7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`. Cuando presione Enter para un comando de transferencia, se le pedirá que apruebe los detalles de la transacción en su dispositivo Ledger. En el dispositivo, utilice los botones derecho e izquierdo para revisar los detalles de la transacción. Si se ven correctamente, haga clic en ambos botones de la pantalla "Aprobar", de lo contrario presione ambos botones en la pantalla "Rechazar".

```bash
~$ solana-keygen pubkey usb://ledger?key=42
CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV

~$ solana balance CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
1.000005 SOL

~$ solana transfer 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri 1 --keypair usb://ledger?key=42
Waiting for your approval on Ledger hardware wallet usb://ledger/2JT2Xvy6T8hSmT8g6WdeDbHUgoeGdj6bE2VueCZUJmyN
✅ Approved

Signature: kemu9jDEuPirKNRKiHan7ycybYsZp7pFefAdvWZRq5VRHCLgXTXaFVw3pfh87MQcWX4kQY4TjSBmESrwMApom1V
```

Después de aprobar la transacción en su dispositivo, el programa mostrará la firma de la transacción, y esperar al número máximo de confirmaciones (32) antes de regresar. Esto solo toma unos segundos, y entonces la transacción está finalizada en la red Solana. Puedes ver los detalles de esta o cualquier otra transacción yendo a la pestaña de Transacciones en el [Explorador](https://explorer.solana.com/transactions) y pegar en la firma de la transacción.

## Operaciones avanzadas

### Administrar Billeteras carteras hardware

A veces es útil firmar una transacción con claves de múltiples billeteras hardware. Iniciar sesión con múltiples billeteras requiere _URL completas del keypair_. Cuando la URL no está completamente cualificada, la CLI de Solana le pedirá las URLs completamente cualificadas de todas las billetera hardware conectadas, y pregunte qué billetera usar para cada firma.

En lugar de usar las instrucciones interactivas, puedes generar URLs cualificadas usando el comando Solana CLI `resolve-signner`. Por ejemplo, prueba conectando un Nano a USB, desbloquéalo con tu pin y ejecuta el comando siguiente:

```text
solana resolve-signer usb://ledger?key=0/0
```

Verás una salida similar a:

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

pero donde `BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK` es su `WALLET_ID`.

Con su URL completamente cualificada, puede conectar múltiples billeteras hardware a la misma computadora e identificar un keypair de cualquiera de ellas. Usa la salida del comando `resolve-signner` en cualquier lugar un comando `solana` espera una entrada `<KEYPAIR>` para usar esa ruta resuelta como el firmante para esa transacción dada.

## Solución de problemas

### Parámetros URL del keypair son ignorados en zsh

El carácter de signo de interrogación es un carácter especial en zsh. Si no es una función que utilices, añade la siguiente línea a tu `~/.zshrc` para tratarla como un carácter normal:

```bash
unsetopt nomatch
```

Luego reinicia tu ventana de shell o ejecuta `~/.zshrc`:

```bash
fuente ~/.zshrc
```

Si prefiere no desactivar el manejo especial de zsh del carácter del signo de interrogación, puede desactivarlo explicativamente con una barra invertida en las URLs del keypair. Por ejemplo:

```bash
pubkey solana-keygen usb://ledger\?key=0
```

## Soporte

Echa un vistazo a nuestra [Página de soporte al monedero](../support.md) para encontrar formas de obtener ayuda.

Lea más sobre [enviar y recibir tokens](../../cli/transfer-tokens.md) y [delegar stake](../../cli/delegate-stake.md). Puede utilizar su URL de keypair Ledger en cualquier lugar que vea una opción o argumento que acepte un `<KEYPAIR>`.
