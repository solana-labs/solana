---
title: Guía de billetera de Solana
---

Este documento describe las diferentes opciones de monederos disponibles para los usuarios de Solana que quieren poder enviar. recibir e interactuar con tokens SOL en la cadena de bloques de Solana.

## ¿Qué es una billetera?

Una billetera de criptomonedas es un dispositivo o aplicación que almacena una colección de claves y puede utilizarse para enviar, recibir y hacer un seguimiento de la propiedad de las criptomonedas. Las billeteras pueden adoptar muchas formas. Una billetera puede ser un directorio o archivo en el sistema de archivos de su computadora, un pedazo de papel, o un dispositivo especializado llamado billetera _hardware_. También hay varias aplicaciones de smartphone y programas de computadora que proporcionan una manera sencilla de crear y gestionar billeteras.

Un par de claves \__ es una clave privada \_generada de forma segura_ y su clave pública _derivada criptográficamente_. Una clave privada y su correspondiente clave pública son conocidas como _keypair_. Una billetera contiene una colección de uno o más keypairs y proporciona algunos medios para interactuar con ellos.

La _clave pública_ (comúnmente acortada a _pubkey_) es conocida como _dirección receptora_ de la billetera o simplemente su _dirección_. La dirección de la billetera **puede ser compartida y mostrada libremente**. Cuando otra parte va a enviar alguna cantidad de criptomoneda a una billetera, necesitan saber la dirección de recepción de la billetera. Dependiendo de la implementación de un blockchain, la dirección también puede utilizarse para ver cierta información sobre unabilletera, tales como ver el saldo, pero no tiene la capacidad de cambiar nada sobre la billetera o retirar fichas.

La _private key_ es necesaria para firmar digitalmente cualquier transacción para enviar criptomonedas a otra dirección o para hacer cambios en la billetera. La clave privada **nunca debe ser compartida**. Si alguien obtiene acceso a la clave privada de una billetera, puede retirar todos los tokens que contiene. Si se pierde la clave privada de una billetera, cualquier tokens que se hayan enviado a la dirección de esa cartera son **perdidos permanentemente**.

Las diferentes soluciones de billetera ofrecen diferentes enfoques para la seguridad del keypair y la interacción con el keypair y la firma de transacciones para usar/gastar los tokens. Algunas son más fáciles de usar que otras. Algunos almacenan y hacen una copia de seguridad de las claves privadas con mayor seguridad. Solana soporta múltiples tipos de billeteras para que puedas elegir el balance adecuado de seguridad y conveniencia.

**Si quieres poder recibir fichas SOL en el blockchain de Solana, primero tendrás que crear una billetera.**

## Billeteras soportadas

Solana admite varios tipos de billetera en la aplicación nativa de línea de comandos de Solana así como billeteras de terceros.

Para la mayoría de los usuarios, recomendamos usar una [app de billetera](wallet-guide/apps.md) o una [billetera web](wallet-guide/web-wallets.md) basada en el navegador, la cual proporcionará una experiencia de usuario más familiar en lugar de necesitar aprender herramientas de línea de comandos.

Para usuarios avanzados o desarrolladores, las carteras de [línea de comandos](wallet-guide/cli.md) pueden ser más apropiadas, ya que las nuevas características en el blockchain de Solana siempre serán soportadas primero en la línea de comandos antes de ser integradas en soluciones de terceros.
