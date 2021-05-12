---
title: Billetera web SolFlare
---

## Introducción

[SolFlare.com](https://solflare.com/) es una Billetera web creada por la comunidad específicamente para Solana. SolFlare soporta el envío y la recepción de tokens SOL nativos, así como el envío y recepción de tokens SPL (equivalente en ERC de Solana). SolFlare también soporta staking de tokens SOL.

Como monedero _no custodial_, sus claves privadas no son almacenadas por el sitio SolFlare en sí. pero más bien se almacenan en un archivo encriptado [almacén de claves](#using-a-keystore-file) o en una cartera de hardware [Ledger Nano S o X](#using-a-ledger-nano-hardware-wallet).

Esta guía describe cómo configurar una cartera usando SolFlare, cómo enviar y recibir tokens SOL, y cómo crear y administrar una cuenta de stake.

## Comenzando

Vaya a https://www.solflare.com en un navegador compatible. Los navegadores web más populares deberían funcionar al interactuar con un archivo de almacén de claves, pero actualmente sólo Chrome y Brave son compatibles cuando interactúan con un Ledger Nano.

### Usando un archivo Keystore

#### Crear un nuevo archivo Keystore

Para crear una billetera con un archivo Keystore haga clic en "Crear una billetera" y seleccione "Usar el archivo Keystore". Siga las instrucciones para crear una contraseña que se utilizará para cifrar su archivo Keystore y luego para descargar el nuevo archivo en tu ordenador. Se le pedirá que vuelva a subir el archivo Keystore al sitio para verificar que la descarga se ha guardado correctamente.

**NOTA: Si pierdes el archivo de tu almacén de claves o la contraseña utilizada para cifrarlo, cualquier dinero en esa cartera se perderá permanentemente. Ni el equipo de Solana ni los desarrolladores de SolFlare pueden ayudarle a recuperar las claves perdidas.**

Puede considerar guardar una copia de seguridad de su almacén de claves en una unidad externa separada de su computadora principal, y almacenar su contraseña en una ubicación separada.

#### Acceder a su cartera con un archivo de Keystore

Para utilizar SolFlare con un archivo creado previamente, haga clic en "Acceder a una billetera" y seleccione "Usar archivo Keystore". Si acaba de crear un nuevo archivo Keystore, será dirigido a la página de acceso directamente. Se le pedirá que ingrese la contraseña y suba el archivo Keystore, luego se le llevará a la página principal de la interfaz de la billetera.

### Usando una Billetera hardware Ledger Nano

_NOTA: Por favor vea [problemas conocidos](ledger-live.md#known-issues) para cualquier limitación actual en el uso del Nano._

#### Configuración inicial del dispositivo

Para usar un Ledger Nano con SolFlare, primero asegúrate de que tienes [configurado tu Nano](ledger-live.md) y tienes [instalada la última versión de la aplicación Solana](ledger-live.md#upgrade-to-the-latest-version-of-the-solana-app) en tu dispositivo.

#### Seleccione una dirección Ledger para acceder

Conéctate a tu Nano y abre la aplicación Solana para que la pantalla del dispositivo muestre "Aplicación está Lista".

En la página de inicio de SolFlare, haga clic en "Acceder a una billetera" y luego seleccione "Usar el Ledger Nano S | Ledger Nano X". Bajo "Seleccionar ruta de derivación", seleccione la única opción:

`` Solana - 44`/501`/ ``

Nota: Su navegador puede preguntarle si SolFlare puede comunicarse con su dispositivo Ledger. Haga clic en permitir.

Seleccione una dirección para interactuar con el cuadro desplegable inferior y haga clic en "Access".

El dispositivo Ledger puede derivar un gran número de claves privadas y direcciones públicas asociadas a ellas. Esto le permite administrar e interactuar con un número arbitrario de cuentas diferentes del mismo dispositivo.

Si deposita fondos a una dirección derivada de su dispositivo Ledger, asegúrese de acceder a la misma dirección cuando utilice SolFlare para poder acceder a esos fondos. Si se conecta a la dirección incorrecta, simplemente haga clic en Cerrar sesión y vuelva a conectar con la dirección correcta.

## Seleccione una red

Solana mantiene [tres redes distintas](../clusters), cada una de las cuales tiene su propio propósito en el soporte del ecosistema de Solana. La Beta de Mainnet se selecciona por defecto en SolFlare, ya que esta es la red permanente donde se implementan intercambios y otras aplicaciones de producción. Para seleccionar una red diferente, haga clic en el nombre de la red seleccionada actualmente en la parte superior del panel de control de la billetera, o Mainnet, Testnet o Devnet, luego haga clic en el nombre de la red que desea utilizar.

## Enviando y Recibiendo tokens SOL

### Recibiendo

Para recibir tokens en tu billetera, alguien debe transferir algo a la dirección de tu billetera. La dirección se muestra en la parte superior izquierda de la pantalla, y puedes hacer clic en el icono Copiar para copiar la dirección y proporcionarla a quien te esté enviando tokens. Si mantiene tokens en una billetera diferente o en un exchange, también puede retirar a esta dirección. Una vez realizada la transferencia, el balance mostrado en SolFlare se actualizará en unos segundos.

### Enviando

Una vez que tenga algunos tokens en su dirección de billetera, puedes enviarlos a cualquier otra dirección de billetera o a una dirección de depósito de un exchange haciendo clic en "Transfer SOL" en la esquina superior derecha. Introduzca la dirección del destinatario y la cantidad de SOL para transferir y haga clic en "Enviar". Se le pedirá que confirme los detalles de la transacción antes de [utilizar su clave para firmar la transacción](#signing-a-transaction) y luego se enviará a la red.

## Staking de Tokens SOL

SolFlare soporta la creación y gestión de cuentas de stake y delegaciones. Para aprender acerca de cómo funciona el staking de Solana en general, revisa nuestra [Guía de Staking](../staking).

### Crear una cuenta Stake

Puede utilizar algunos de los tokens SOL de su billetera para crear una nueva cuenta de stake. Desde la página principal de la billetera haga clic en "Staking" en la parte superior de la página. En la parte superior-, haga clic en "Crear cuenta". Ingrese la cantidad de SOL que desea utilizar para depositar fondos en su nueva cuenta de stake. Esta cantidad se retirará de su billetera y se transferirá a la cuenta de stake. No transfiera todo el saldo de su billetera a una cuenta de stake, ya que el monedero todavía se utiliza para pagar cualquier comisión de transacción asociada con su cuenta de stake. Considere dejar al menos 1 SOL en su billetera.

Después de enviar y [firmar la transacción](#signing-a-transaction) verás tu nueva cuenta de stake aparecerá en la caja etiquetada "Tus cuentas de staking".

Las cuentas de Stake creadas en SolFlare establecen su dirección de billetera como la [autoridad de staking y retiro](../staking/stake-accounts#understanding-account-authorities) para su nueva cuenta. que da a la clave de su cartera la autoridad para firmar para cualquier transacción relacionada con la nueva cuenta de stake.

### Ver tus cuentas Stake

En la página principal del panel de control de la billetera o en la página del panel de control, las cuentas de stake serán visibles en el recuadro "Tus cuentas de staking". Las cuentas Stake existen en una dirección diferente de su cartera.

SolFlare localizará cualquier visualización de todas las cuentas de stake en la [red seleccionada](#select-a-network) para la cual su dirección de billetera está asignada como [autoridad de participación](../staking/stake-accounts#understanding-account-authorities). Las cuentas Stake que fueron creadas fuera de SolFlare también se mostrarán y podrán ser administradas mientras el monedero con el que iniciaste sesión se asigne como la autoridad del monedero.

### Delegar tokens en una cuenta stake

Una vez que hayas [seleccionado un validador](../staking#select-a-validator), puedes delegar los tokens en una de tus cuentas de stake. Desde el panel de control, haga clic en "Delegate" en el lado derecho de una cuenta de stake mostrada. Seleccione el validador al que desea delegar en la lista desplegable y haga clic en Delegar.

Para desdelegar tus tokens puestos en stake (también dicho, desactivar tu stake), el proceso es similar. En la página de Staking, en el lado derecho de una cuenta delegada, haga clic en el botón "Desdelegar" y siga las indicaciones.

### Dividir una cuenta Stake

Puedes dividir una cuenta de stake existente en dos cuentas. Haga clic en la dirección de una cuenta de stake controlada por su billetera y, debajo de la barra de Acciones, haga clic en "Dividir". Especifique la cantidad de tokens SOL que desea dividir. Esta será la cantidad de tokens en tu nueva cuenta de stake y tu saldo actual de la cuenta de stake se reducirá en la misma cantidad. Dividir tu cuenta de stake te permite delegar a varios validadores diferentes con diferentes cantidades de tokens. Puedes dividir una cuenta de stake tantas veces como quieras, para crear tantas cuentas de stake como desees.

## Firmando una transacción

Cada vez que envíes una transacción como enviar tokens a otra cartera o delegando stake, necesita usar su clave privada para firmar la transacción para que sea aceptado por la red.

### Usando un archivo Keystore

Si accedió a su cartera usando un archivo Keystore se le pedirá que ingrese su contraseña en cualquier momento en que se necesite la clave para firmar una transacción.

### Usando un Ledger Nano

Si accedió a su cartera con un Ledger Nano, se le pedirá confirmar los detalles de la transacción pendiente en su dispositivo cada vez que se necesite la clave para firmar. En el Nano, usa los botones izquierdo y derecho para ver y confirmar todos los detalles de la transacción. Si todo parece correcto, sigue haciendo clic en el botón derecho hasta que la pantalla muestre "Aprobar". Haga clic en ambos botones para aprobar la transacción. Si algo parece incorrecto, pulsa el botón derecho una vez más para que la pantalla muestre "Rechazar" y pulsa ambos botones para rechazar la transacción. Después de aprobar o rechazar una transacción, verás esto reflejado en la página de SolFlare.
