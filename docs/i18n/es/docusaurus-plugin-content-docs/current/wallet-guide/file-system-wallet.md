---
title: Billetera del Sistema de Archivos
---

Este documento describe cómo crear y utilizar una billetera del sistema de archivos con las herramientas de CLI de Solana. Existe una billetera del sistema de archivos como un archivo de keypair sin cifrar en el sistema de archivos de su ordenador.

> Las billeteras del sistema de archivos son el método **menos seguro** de almacenar los tokens SOL. Almacenar grandes cantidades de tokens en una billetera del sistema de archivos **no es recomendable**.

## Antes de comenzar

Asegúrese de que tiene [instalado las herramientas de línea de comandos Solana](../cli/install-solana-cli-tools.md)

## Generar un File System Wallet Keypair

Utilice la herramienta de línea de comandos de Solana `solana-keygen` para generar archivos keypair. Por ejemplo, ejecute lo siguiente desde una línea de comandos shell:

```bash
mkdir ~/my-solana-wallet
solana-keygen new --outfile ~/my-solana-wallet/my-keypair.json
```

Este archivo contiene tu keypair **sin cifrar**. De hecho, incluso si especifica una contraseña, esa contraseña se aplica a la frase de semilla de recuperación, no al archivo. No compartas este archivo con otros. Cualquiera que tenga acceso a este archivo tendrá acceso a todos los tokens enviados a su clave pública. En su lugar, solo se debe compartir su clave pública. Para mostrar su clave pública, ejecute:

```bash
pubkey solana-keygen ~/my-solana-wallet/my-keypair.json
```

Le mostrará una cadena de caracteres, como:

```text
ErRr1caKzK8L8nn4xmEWtimYRiTCAZXjBtVphuZ5vMKy
```

Esta es la clave pública correspondiente al keypair en `~/my-solana-wallet/my-keypair.json`. La clave pública del archivo del keypair es su _dirección de billetera_.

## Verifique su dirección con su archivo Keypair

Para verificar que posee la clave privada para una dirección determinada, utilice `verificación de solana-keygen`:

```bash
solana-keygen verify <PUBKEY> ~/my-solana-wallet/my-keypair.json
```

donde `<PUBKEY>` es reemplazado con su dirección de billetera. El comando mostrará "Éxito" si la dirección dada coincide con la del archivo del keypair y "Failed" en caso contrario.

## Creando múltiples direcciones del sistema de archivos

Puede crear tantas direcciones de cartera como desee. Simplemente vuelva a ejecutar los pasos en [Genere una billetera del sistema de archivos](#generate-a-file-system-wallet-keypair) y asegúrese de usar un nuevo nombre de archivo o ruta con el argumento `--outfile`. Múltiples direcciones del monedero pueden ser útiles si desea transferir tokens entre sus propias cuentas para diferentes propósitos.
