---
title: "Transacciones"
---

La ejecución del programa comienza con una [transacción](terminology.md#transaction) siendo enviada al clúster. El tiempo de ejecución de Solana ejecutará un programa para procesar cada una de las [instrucciones](terminology.md#instruction) contenidas en la transacción en orden y atómicamente.

## Anatomía de una transacción

Esta sección cubre el formato binario de una transacción.

### Formato de la transacción

Una transacción contiene un [compact-array](#compact-array-format) de firmas, seguido de un [mensaje](#message-format). Cada elemento en el arreglo de firmas es una [firma digital](#signature-format) del mensaje dado. El tiempo de ejecución de Solana verifica que el número de firmas coincida con el número en los primeros 8 bits del encabezado de mensaje [](#message-header-format). También verifica que cada firma fue firmada por la clave privada correspondiente a la clave pública en el mismo índice en el arreglo de direcciones de cuenta del mensaje.

#### Formato de firma

Cada firma digital está en el formato binario ed25519 y consume 64 bytes.

### Formato del mensaje

Un mensaje contiene un [encabezado](#message-header-format), seguido de una matriz compacta de [direcciones de cuenta](#account-addresses-format), seguida de un reciente [blockhash](#blockhash-format), seguido de una matriz compacta de [instrucciones](#instruction-format).

#### Formato del encabezado del mensaje

El encabezado del mensaje contiene tres valores no firmados de 8 bits. El primer valor es el número de firmas requeridas en la transacción que contiene. El segundo valor es el número de las direcciones correspondientes de la cuenta que son de solo lectura. El tercer valor de en el encabezado del mensaje es el número de direcciones de cuenta de solo lectura que no requieren firmas.

#### Formato de direcciones de cuenta

Las direcciones que requieren firmas aparecen al principio del arreglo de direcciones de la cuenta, con direcciones que solicitan acceso de escritura primero y cuentas de sólo lectura que siguen. Las direcciones que no requieren firmas siguen las direcciones que siguen, de nuevo con las cuentas de read-write primero y las cuentas de solo lectura.

#### Formato Blockhash

Un blockhash contiene un hash SHA-256 de 32 bytes. Se utiliza para indicar cuando un cliente observó por última vez el ledger. Los validadores rechazarán las transacciones cuando el blockhash sea demasiado antiguo.

### Formato de Instrucción

Una instrucción contiene un índice de id del programa, seguido de una matriz compacto de índices de dirección de cuenta, seguido de una matriz compacto de datos opacos de 8 bits. El índice de id de programa se utiliza para identificar un programa en cadena que puede interpretar los datos opacos. El índice de identificación del programa es un índice de 8 bits sin signo a una dirección de cuenta en la matriz de direcciones de cuenta del mensaje. Los índices de la dirección de la cuenta son cada uno un índice de 8 bits sin signo en ese mismo array.

### Formato de matriz compacta

Una matriz compacta se serializa como la longitud de la matriz, seguida de cada elemento de la matriz. La longitud de la matriz es una codificación especial de múltiples bytes llamada compact-u16.

#### Formato Compact-u16

Un compact-u16 es una codificación multibyte de 16 bits. El primer byte contiene de 7 bits más bajos del valor en sus 7 bits más bajos. Si el valor está por encima de 0x7f, el bit alto se establece y los siguientes 7 bits del valor se colocan en los 7 bits inferiores de un segundo byte. Si el valor está por encima de 0x3fff, el bit alto se establece y los 2 bits restantes del valor se colocan en los 2 bits inferiores de un tercer byte.

### Formato de direcciones de cuenta

Una dirección de cuenta es de 32 bytes de datos arbitrarios. Cuando la dirección requiere una firma digital, el tiempo de ejecución lo interpreta como la clave pública de un keypair ed25519.

## Instrucciones

Cada instrucción [](terminology.md#instruction) especifica un solo programa, un subconjunto de las cuentas de la transacción que debe ser pasado al programa, y una matriz de bytes de datos que se pasa al programa. El programa interpreta la matriz de datos y opera con las cuentas especificadas por las instrucciones. El programa puede retornar con éxito, o con un código de error. Un retorno de error provoca que la transacción entera falle inmediatamente.

Normalmente el programa proporciona funciones de ayuda para construir instrucciones que soportan. Por ejemplo, el programa de sistema proporciona el siguiente ayudante de Rust para construir un [`SystemInstruction::CreateAccount`](https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L63) instrucción:

```rust
pub fn create_account(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, true),
    ];
    Instruction::new(
        system_program::id(),
        &SystemInstruction::CreateAccount {
            lamports,
            space,
            owner: *owner,
        },
        account_metas,
    )
}
```

El cual se puede encontrar aquí:

https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L220

### Id del programa

El [identificador de programa](terminology.md#program-id) de la instrucción especifica qué programa procesará esta instrucción. El propietario de la cuenta del programa especifica qué cargador debe usarse para cargar y ejecutar el programa y los datos contienen información sobre cómo el tiempo de ejecución debe ejecutar el programa.

En el caso de [programas desplegados](developing/deployed-programs/overview.md), el propietario es el cargador BPF y los datos de la cuenta guardan el bytecode BPF. Las cuentas del programa son permanentemente marcadas como ejecutables por el cargador una vez que estén desplegadas correctamente. El tiempo de ejecución rechazará las transacciones que especifican programas que no son ejecutables.

A diferencia de los programas desplegados, [las construcciones](developing/builtins/programs.md) son manejadas de forma diferente en que están construidas directamente en el tiempo de ejecución de Solana.

### Cuentas

Las cuentas referenciadas por una instrucción representan el estado en cadena y sirven como tanto las entradas como las salidas de un programa. Puede encontrar más información sobre clientes en la sección [Cuentas](accounts.md).

### Datos de instrucción

Cada instrucción contiene una matriz de bytes de propósito general que se pasa al programa junto con las cuentas. El contenido de los datos de las instrucciones es específico del programa y normalmente se utiliza para transmitir qué operaciones debe realizar el programa, y cualquier información adicional que esas operaciones puedan necesitar por encima de lo que contienen las cuentas.

Los programas son libres de especificar cómo se codifica la información en la matriz de bytes de datos de instrucción. La elección de cómo se codifican los datos debe tener en cuenta la sobrecarga de la decodificación, ya que ese paso lo realiza el programa en la cadena. Se ha observado que algunas codificaciones comunes (bincode de Rust, por ejemplo) son muy ineficientes.

El programa [Solana Program Library's Token ](https://github.com/solana-labs/solana-program-library/tree/master/token) da un ejemplo de cómo los datos de instrucciones pueden codificarse eficientemente, pero tenga en cuenta que este método sólo soporta tipos de tamaño fijo. Token utiliza el rasgo [Pack](https://github.com/solana-labs/solana/blob/master/sdk/program/src/program_pack.rs) para codificar/decodificar los datos de las instrucciones de los tokens, así como los estados de las cuentas de los tokens.

## Firmas

Cada transacción enumera explícitamente todas las claves públicas de la cuenta referenciadas por las instrucciones de la transacción. Un subconjunto de esas claves públicas van acompañadas por una firma de transacción. Esas firmas señalan a los programas en cadena que el titular de la cuenta ha autorizado la transacción. Normalmente, el programa utiliza la autorización para permitir debitar la cuenta o modificar sus datos. Puede conseguir mas información de cómo se comunica la autorización a un programa en [Cuentas](accounts.md#signers)

## Blockhash reciente

Una transacción incluye un [blockhash](terminology.md#blockhash) reciente para evitar la duplicación y dar vida a las transacciones. Cualquier transacción que sea completamente idéntica a una anterior es rechazada, así que añadir un blockhash nuevo permite a múltiples transacciones repetir exactamente la misma acción. Las transacciones también tienen tiempos de vida definidos por el blockhash, como cualquier transacción cuya blockhash sea demasiado vieja será rechazada.
