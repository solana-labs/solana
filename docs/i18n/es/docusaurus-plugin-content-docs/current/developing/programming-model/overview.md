---
title: "Vista general"
---

Una aplicación [](terminology.md#app) interactúa con un clúster Solana enviando [transacciones](transactions.md) con una o más [instrucciones](transactions.md#instructions). El Solana [runtime](runtime.md) pasa esas instrucciones a [programas](terminology.md#program) implementados por desarrolladores de aplicaciones de antemano. Una instrucción podría, por ejemplo, decirle a un programa que transfiera [lamports](terminology.md#lamports) de una [cuenta](accounts.md) a otra o cree un contrato interactivo que regule cómo se transfieren los lamports. Las instrucciones se ejecutan secuencialmente y atómicamente para cada transacción. Si cualquier instrucción no es válida, todos los cambios en la cuenta son descartados.

Para empezar a desarrollar inmediatamente puedes construir, desplegar y ejecutar uno de los [ejemplos](developing/deployed-programs/examples.md).
