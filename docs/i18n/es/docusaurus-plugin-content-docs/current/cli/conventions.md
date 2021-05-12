---
title: Usando Solana CLI
---

Antes de ejecutar cualquier comando Solana CLI, vamos a pasar algunas convenciones que verás a través de todos los comandos. En primer lugar, la CLI de Solana es en realidad una colección de diferentes comandos para cada acción que desee realizar. Puede ver la lista de todos los comandos posibles ejecutando:

```bash
solana --help
```

Para hacer zoom sobre cómo usar un comando en particular, ejecute:

```bash
solana <COMMAND> --help
```

donde se sustituye el texto `<COMMAND>` por el nombre del comando que se desea aprender más sobre él.

El mensaje de uso del comando normalmente contendrá palabras como `<AMOUNT>`, `<ACCOUNT_ADDRESS>` o `<KEYPAIR>`. Cada palabra es un marcador de posición para el _tipo_ de texto con el que se puede ejecutar el comando. Por ejemplo, puedes reemplazar `<AMOUNT>` con un número como `42` o `100.42`. Puede reemplazar `<ACCOUNT_ADDRESS>` por la codificación base58 de su clave pública, como `9grmKMwTiZwUHSExjtbFzHLPTdWoXgcg1bZkhvwTrTww`.

## Convenciones Keypair

Muchos comandos que utilizan las herramientas CLI requieren un valor para un `<KEYPAIR>`. El valor que debe usar para el keypair depende del tipo de cartera de línea de comandos [que haya creado](../wallet-guide/cli.md).

Por ejemplo, la forma de mostrar la dirección de cualquier monedero (también conocida como pubkeys delkeypair), el documento de ayuda de CLI muestra:

```bash
pubkey de solana-keygen <KEYPAIR>
```

A continuación, mostramos cómo resolver lo que debe poner en `<KEYPAIR>` dependiendo de su tipo de billetera.

#### Billetera de papel

En una Billetera de papel, el keypair se deriva de forma segura de las palabras de semilla y contraseña opcional que introdujo cuando se creó la Billetera. To use a paper wallet keypair anywhere the `<KEYPAIR>` text is shown in examples or help documents, enter the uri scheme `prompt://` and the program will prompt you to enter your seed words when you run the command.

Para mostrar la dirección de la billetera de una billetera de papel:

```bash
solana-keygen pubkey prompt://
```

#### Cartera del Sistema de Archivos

Con una cartera de sistema de archivos, el keypair se almacena en un archivo en su computadora. Reemplaza `<KEYPAIR>` por la ruta completa del archivo al keypair.

Por ejemplo, si la ubicación del archivo del keypair del sistema de archivos es `/home/solana/my_wallet.json`, para mostrar la dirección, son:

```bash
pubkey solana-keygen /home/solana/my_wallet.json
```

#### Billetera Hardware

Si eligió una billetera de hardware, utilice su [URL de par de claves](../wallet-guide/hardware-wallets.md#specify-a-hardware-wallet-key), como `usb://ledger?key=0`.

```bash
pubkey solana-keygen usb://ledger?key=0
```
