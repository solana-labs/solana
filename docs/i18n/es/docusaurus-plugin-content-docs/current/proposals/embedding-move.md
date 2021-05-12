---
title: Incorporación del lenguaje Mover
---

## Problema

Solana permite a los desarrolladores escribir programas en cadena en lenguajes de programación generales como C o Rust, pero estos programas contienen mecanismos específicos de Solana. Por ejemplo, no hay otra cadena que le pida a los desarrolladores crear un módulo Rust con una función `process_instruction(KeyedAccounts)`. Siempre que sea práctico, Solana debería ofrecer a los desarrolladores de aplicaciones opciones más portátiles.

Hasta hace poco, ningún blockchain popular ofrecía un lenguaje que pudiera exponer el valor del [tiempo de ejecución](../validator/runtime.md) masivo paralelo de Solana. Los contratos de solidez, por ejemplo, no separan las referencias a los datos compartidos del código del contrato, y por lo tanto necesitan ser ejecutados en serio, para asegurar un comportamiento determinista. En la práctica, vemos que todas las cadenas de bloques basadas en EVM más optimizadas parecen alcanzar un máximo de 1.200 TPS, una pequeña fracción de lo que puede hacer Solana. El proyecto Libra, por su parte, diseñó un lenguaje de programación en cadena llamado Move que es más adecuado para la ejecución en paralelo. Al igual que el tiempo de ejecución de Solana, los programas Move dependen de cuentas para todo el estado compartido.

La mayor diferencia de diseño entre el runtime de Solana y el Move VM de Libra es cómo gestionan las invocaciones seguras entre módulos. Solana adoptó un enfoque de sistemas operativos y Libra un enfoque de lenguaje específico del dominio. En el tiempo de ejecución, un módulo debe hacer una trampa en el tiempo de ejecución para asegurarse de que el módulo de la persona que llama no ha escrito en los datos que pertenecen a la persona que llama. Del mismo modo, cuando el llamador finaliza, debe volver a atrapar al tiempo de ejecución para asegurarse de que el llamador no ha escrito en datos propiedad del llamador. Move, por otro lado, incluye un sistema de tipos avanzado que permite que estas comprobaciones sean ejecutadas por su verificador de código de bytes. Debido a que Move bytecode puede ser verificado, el costo de la verificación se paga una sola vez, en el momento en que el módulo se carga en cadena. En el tiempo de ejecución, el costo se paga cada vez que una transacción cruza entre módulos. La diferencia es similar en espíritu a la diferencia entre un lenguaje de tipo dinámico como Python y un lenguaje de tipo estático como Java. El tiempo de ejecución de Solana permite que las aplicaciones se escriban en lenguajes de programación de propósito general, pero eso viene con el costo de las comprobaciones de tiempo de ejecución al saltar entre programas.

Esta propuesta intenta definir una forma de incrustar la MV Mover tal que:

- las invocaciones entre módulos dentro de Move no requieren las comprobaciones

  entre programas del tiempo de ejecución

- Mover programas puede aprovechar la funcionalidad en otros programas Solana y vice

  versa

- El paralelismo en tiempo de ejecución de Solana se expone a lotes de Move y no Move

  transacciones

## Solución propuesta

### Mover MV como cargador de Solana

La VM de Move se incrustará como un cargador de Solana bajo el identificador `MOVE_PROGRAM_ID`, de modo que los módulos de Move puedan marcarse como `ejecutables` con la VM como su `propietario`. Esto permitirá que los módulos carguen las dependencias de los módulos, así como la ejecución en paralelo de los scripts de Move.

Todas las cuentas de datos propiedad de los módulos Move deben establecer sus propietarios en el cargador, `MOVE_PROGRAM_ID`. Dado que los módulos Move encapsulan sus datos de cuenta del mismo modo que los programas Solana encapsulan los suyos, el propietario del módulo Move debería estar incrustado en los datos de cuenta. El tiempo de ejecución concederá acceso de escritura a la VM de Move, y Move concede acceso a las cuentas del módulo.

### Interactuando con programas de Solana

Para invocar instrucciones en programas que no son de Move, Solana necesitaría extender la MV de Move con una llamada al sistema `process_instruction()`. Funcionaría igual que `process_instruction()` Rust BPF programs.
