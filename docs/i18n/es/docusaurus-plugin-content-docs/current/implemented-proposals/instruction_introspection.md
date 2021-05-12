---
title: introducción de instrucción
---

## Problema

Algunos programas de contratos inteligentes pueden querer verificar que otra Instrucción está presente en un mensaje dado, ya que esa Instrucción podría estar realizando una verificación de ciertos datos. en una función recompilada. (See secp256k1_instruction for an example).

## Solución

Añadir un nuevo sysvar Sysvar1nstructions111111111111111111111111111111111111111 al que un programa puede hacer referencia y recibir los datos de instrucciones del mensaje dentro de él, y también el índice de la instrucción actual.

Se pueden utilizar dos funciones de ayuda para extraer estos datos:

```
fn load_current_index(instruction_data: &[u8]) -> u16;
fn load_instruction_at(instruction_index: usize, instruction_data: &[u8]) -> Resultado<Instruction>;
```

El tiempo de ejecución reconocerá esta instrucción especial, serializa los datos de instrucciones del mensaje para él y también escribe el índice de instrucciones actual y luego el programa bpf puede extraer información necesaria desde allí.

Nota: la serialización personalizada de las instrucciones se utiliza porque el bincode es aproximadamente 10x más lento en el código nativo y excede los límites actuales de instrucciones BPF.
