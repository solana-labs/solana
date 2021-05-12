---
title: Utilizar carteras de hardware en el Solana CLI
---

Firmar una transacción requiere una clave privada, pero almacenar una clave privada en su computadora personal o teléfono la deja sujeta a robo. Añadir una contraseña a la clave añade seguridad, pero mucha gente prefiere ir un paso más allá y mover sus claves privadas a un dispositivo físico separado llamado _monedero de hardware_. Una cartera de hardware es un pequeño dispositivo portátil que almacena claves privadas y proporciona cierta interfaz para firmar transacciones.

La Solana CLI tiene soporte de primera clase para carteras de hardware. En cualquier lugar se utiliza una ruta de archivo keypair (denotada como `<KEYPAIR>` en la documentación de uso), puede pasar una _URL del par de claves_ que identifica de forma única un keypair en una cartera de hardware.

## Carteras Hardware soportadas

El CLI de Solana soporta las siguientes carteras hardware:

- [Ledger Nano S y Ledger Nano X](hardware-wallets/ledger.md)

## Especifique una URL de keyypair

Solana define un formato URL de keypair para localizar de forma única cualquier keypair de Solana en una cartera de hardware conectada a su ordenador.

La URL keypair tiene la siguiente forma, donde los corchetes denotan campos opcionales:

```text
usb://<MANUFACTURER>[/<WALLET_ID>][?key=<DERIVATION_PATH>]
```

`WALLET_ID` es una clave única a nivel global usada para desambiguar múltiples dispositivos.

`DERVIATION_PATH` se utiliza para navegar a las claves Solana dentro de su cartera de hardware. La ruta tiene la forma `<ACCOUNT>[/<CHANGE>]`, donde cada `CUENTA` y `CAMBIO` son enteros positivos.

Por ejemplo, una URL completa para un dispositivo Ledger podría ser:

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

Todas las rutas de derivación incluyen implícitamente el prefijo `44'/501'`, que indica que la ruta sigue las especificaciones de [BIP44](https://github.com/bitcoin/bips/blob/master/bip0044.mediawiki) y que cualquier clave derivada es una clave de Solana (tipo Coin 501). La comilla simple indica una derivación "reforzada". Dado que Solana utiliza keypairs Ed25519, todas las derivaciones están reforzadas y, por tanto, añadir la comilla es opcional e innecesario.
