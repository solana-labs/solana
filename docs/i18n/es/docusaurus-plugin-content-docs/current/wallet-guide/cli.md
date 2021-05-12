---
title: Carteras de línea de comandos
---

Solana soporta varios tipos de monederos que pueden utilizarse para interactuar directamente con las herramientas de línea de comandos de Solana.

**Si no estás familiarizado con el uso de programas de línea de comandos y sólo quieres poder enviar y recibir tokens SOL, te recomendamos que configures una aplicación de terceros [Billetera de aplicaciones](apps.md)**.

Para utilizar una cartera de línea de comandos, primero debe [instalar las herramientas de Solana CLI](../cli/install-solana-cli-tools.md)

## Cartera del Sistema de Archivos

Una billetera del sistema de archivos __, conocida como una billetera FS, es un directorio en el sistema de archivos de su computadora. Cada archivo en el directorio contiene un keypair.

### Seguridad de la cartera del sistema de archivos

Una cartera de sistema de archivos es la forma de cartera más conveniente y menos segura. Es conveniente porque el keypair se almacena en un archivo simple. Puedes generar tantas claves como quieras y hacer una copia de seguridad trivial copiando los archivos. Es inseguro porque los archivos del par de claves son **sin cifrar**. Si usted es el único usuario de su computadora y está seguro de que está libre de malware, una cartera FS es una buena solución para pequeñas cantidades de criptomonedas. Sin embargo, si su computadora contiene malware y está conectada a Internet, ese malware puede subir sus claves y usarlas para tomar sus tokens. Del mismo modo, debido a que los keypair se almacenan en su computadora como archivos, un hacker experto con acceso físico a tu ordenador puede ser capaz de acceder a él. Usar un disco encriptado duro, como FileVault en MacOS, minimiza ese riesgo.

[Cartera del Sistema de Archivos](file-system-wallet.md)

## Cartera de papel

Una cartera de papel __ es una colección de _frases de semilla_ escritas en papel. Una semilla es un número de palabras (normalmente 12 o 24) que pueden ser usadas para regenerar un par de claves bajo demanda.

### Seguridad de cartera de papel

En términos de conveniencia frente a seguridad, una cartera de papel se encuentra al lado opuesto del espectro de una cartera de FS. Es terriblemente incómodo de usar, pero ofrece una seguridad excelente. Esa alta seguridad se amplifica aún más cuando se utilizan carteras de papel junto con una [firma sin conexión](../offline-signing.md). Los servicios de Custodia como [Coinbase Custody](https://custody.coinbase.com/) usan esta combinación. Las carteras de papel y los servicios de custodia son una excelente manera de asegurar un gran número de tokens durante un largo período de tiempo.

[Carteras de papel](paper-wallet.md)

## Cartera Hardware

Una cartera de hardware es un pequeño dispositivo de mano que almacena keypairs y proporciona alguna interfaz para firmar transacciones.

### Seguridad de billetera Hardware

Una cartera de hardware, como la [cartera de hardware Ledger](https://www.ledger.com/), ofrece una gran mezcla de seguridad y comodidad para criptomonedas. Automatiza efectivamente el proceso de firma sin conexión mientras conserva casi toda la comodidad de una cartera de sistema de archivo.

[Carteras Hardware](hardware-wallets.md)
