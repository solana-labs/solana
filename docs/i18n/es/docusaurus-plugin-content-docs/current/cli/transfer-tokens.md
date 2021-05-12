---
title: Enviar y recibir tokens
---

Esta página describe cómo recibir y enviar tokens SOL usando las herramientas de la línea de comandos con una cartera de línea de comandos como [cartera de papel](../wallet-guide/paper-wallet.md), una [cartera de sistema de archivos](../wallet-guide/file-system-wallet.md)o una [cartera hardware](../wallet-guide/hardware-wallets.md). Antes de empezar, asegúrate de haber creado un monedero y de tener acceso a su dirección (pubkey) y al keypair de ingreso. Echa un vistazo a nuestras [convenciones para introducir keypairs para diferentes tipos de monederos](../cli/conventions.md#keypair-conventions).

## Probando tu cartera

Antes de compartir tu clave pública con otros, primero debería asegurarse de que la clave es válida y de que posee la clave privada correspondiente.

En este ejemplo, crearemos una segunda cartera además de tu primera cartera, y luego transferiremos algunos tokens a ella. Esto confirmará que puede enviar y recibir tokens en su tipo de monedero elegido.

Este ejemplo de prueba utiliza nuestro desarrollador Testnet, llamado devnet. Las fichas emitidas en devnet **no** tienen valor, así que no te preocupes si las pierdes.

#### Airdrop algunos tokens para comenzar

Primero, _airdrop_ a ti mismo algunos tokens de juego en el devnet.

```bash
solana airdrop 1 <RECIPIENT_ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

donde se sustituye el texto `<RECIPIENT_ACCOUNT_ADDRESS>` por su clave pública/dirección de cartera codificada en base58 clave pública/dirección del monedero.

#### Compruebe su saldo

Confirme que el airdrop fue exitoso comprobando el saldo de la cuenta. Debería mostrar`1 SOL`:

```bash
saldo de solana <ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

#### Crear una segunda dirección de cartera

Necesitaremos una nueva dirección para recibir nuestros tokens. Crea un segundo keypair y graba su pubkey:

```bash
nuevo keygen de solana --no-passphrase --no-outfile
```

La salida contendrá la dirección después del texto `pubkey:`. Copiar la dirección. Lo utilizaremos en el siguiente paso.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

También puede crear un segundo (o más) monedero de cualquier tipo: [papel](../wallet-guide/paper-wallet#creating-multiple-paper-wallet-addresses), [sistema de archivos](../wallet-guide/file-system-wallet.md#creating-multiple-file-system-wallet-addresses), o [hardware](../wallet-guide/hardware-wallets.md#multiple-addresses-on-a-single-hardware-wallet).

#### Transferir tokens de su primera cartera a la segunda dirección

Después, comprueba que eres dueño de los tokens recibidos del airdrop transfiriéndolos. El clúster de Solana solo aceptará la transferencia si firma la transacción con el keypair correspondiente a la clave pública del remitente en la transacción.

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> 0.5 --allow-unfunded-recipient --url https://devnet.solana.com --fee-payer <KEYPAIR>
```

donde reemplaza `<KEYPAIR>` con la ruta de acceso a un keypair en su primera cartera, y sustituye `<RECIPIENT_ACCOUNT_ADDRESS>` con la dirección de tu segunda cartera.

Confirma el saldo actualizado con `saldo solana`:

```bash
saldo de solana <ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

donde `<ACCOUNT_ADDRESS>` es la clave pública de su keypair o la clave pública del destinatario.

#### Ejemplo completo de la transferencia de prueba

```bash
$ solana-keygen new --outfile my_solana_wallet.json # Creando mi primer monedero, un monedero de sistema de archivos
Generando un nuevo Keypair
Para mayor seguridad, introduzca una frase de contraseña (vacía para no tener frase de contraseña):
Escribir el nuevo keypair en my_solana_wallet.json
==========================================================================
pubkey: DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK # Aquí está la dirección del primer monedero
==========================================================================
Guarda esta frase semilla para recuperar tu nuevo keypair:
width enhance concert vacant ketchup eternal spy craft spy guard tag punch # ¡Si esto fuera un monedero real, nunca compartiría estas palabras en internet de esta manera!
==========================================================================

$ solana airdrop 1 DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com  # Airdropping 1 SOL to my wallet's address/pubkey
Requesting airdrop of 1 SOL from 35.233.193.70:9900
1 SOL

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com # Check the address's balance
1 SOL

$ solana-keygen new --no-outfile  # Creating a second wallet, a paper wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
====================================================================
pubkey: 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv                   # Here is the address of the second, paper, wallet.
====================================================================
Guarde esta frase semilla para recuperar su nuevo keypair:
clump panic cousin hurt coast charge engage eager urge win love # Si esto fuera una cartera real, ¡nunca compartas estas palabras en internet de esta manera!
====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 0.5 --allow-unfunded-recipient --url https://devnet.solana.com --fee-payer my_solana_wallet.json  # Transferring tokens to the public address of the paper wallet
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # This is the transaction signature

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com
0.499995 SOL  # The sending account has slightly less than 0.5 SOL remaining due to the 0.000005 SOL transaction fee payment

$ solana balance 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv --url https://devnet.solana.com
0.5 SOL  # The second wallet has now received the 0.5 SOL transfer from the first wallet

```

## Recibir tokens

Para recibir tokens, necesitarás una dirección para que otros envíen tokens. En Solana, la dirección del monedero es la clave pública de un keypair. Hay una variedadde técnicas para generar keypairs. El método que elija dependerá de cómo elija almacenar keypairs. Los keypairs se almacenan en carteras. Antes de recibir tokens, necesitará [crear una cartera](../wallet-guide/cli.md). Una vez completado, debería tener una clave pública para cada keypair que haya generado. La clave pública es una larga cadena de caracteres base58. Su longitud varía de 32 a 44 caracteres.

## Enviar Tokens

Si ya posee SOL y desea enviar tokens a alguien, necesitará una ruta a su keypair, su clave pública codificada en base58, y un número de tokens para transferir. Una vez recolectado, puedes transferir tokens con el comando `solana transfer`:

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> <AMOUNT> --fee-payer <KEYPAIR>
```

Confirma el saldo actualizado con `saldo solana`:

```bash
saldo de solana <ACCOUNT_ADDRESS>
```
