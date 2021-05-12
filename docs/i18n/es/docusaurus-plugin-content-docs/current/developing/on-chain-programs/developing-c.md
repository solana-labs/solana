---
title: "Desarrollando con C"
---

Solana soporta la escritura de programas en cadena utilizando los lenguajes de programación C y C++.

## Diseño del proyecto

Los proyectos C están diseñados de la siguiente manera:

```
/src/<program name>
/makefile
```

El `makefile` debe contener lo siguiente:

```bash
OUT_DIR := <path to place to resulting shared object>
include ~/.local/share/solana/install/active_release/bin/sdk/bpf/c/bpf.mk
```

El bpf-sdk puede no estar en el lugar exacto especificado arriba, pero si configuras tu entorno por [Cómo construir](#how-to-build) entonces debería ser.

Echa un vistazo a [helloworld](https://github.com/solana-labs/example-helloworld/tree/master/src/program-c) para un ejemplo de un programa C.

## Cómo construir

Primero configura el entorno:

- Instalar la última versión estable de Rust desde https://rustup.rs
- Instale las últimas herramientas de línea de comandos de Solana desde https://docs.solana.com/cli/install-solana-cli-tools

Luego construye usando make:

```bash
make -C <program directory>
```

## Cómo probarlo

Solana utiliza el [framework de pruebas](https://github.com/Snaipe/Criterion) Criterión y las pruebas se ejecutan cada vez que el programa se construye [Cómo Construir](#how-to-build)].

Para añadir pruebas, crea un nuevo archivo junto a tu archivo de origen llamado `test_<program name>.c` y rellenalo con casos de prueba de criterio. Para un ejemplo, vea la [helloworld C pruebas](https://github.com/solana-labs/example-helloworld/blob/master/src/program-c/src/helloworld/test_helloworld.c) o la [documentación Criterion](https://criterion.readthedocs.io/en/master) para información sobre cómo escribir un caso de prueba.

## Entrypoint del programa

Los programas exportan un símbolo de punto de entrada conocido que el tiempo de ejecución de Solana busca y llama al invocar un programa. Solana soporta múltiples [versiones del cargador BPF](overview.md#versions) y los puntos de entrada pueden variar entre ellos. Los programas deben escribirse para el mismo cargador y desplegarse en él. Para más detalles vea el [resumen](overview#loaders).

Actualmente hay dos cargadores soportados [BPF cargador](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) y [cargador BPF obsoleto](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

Ambos tienen la misma definición de punto de entrada en bruto, el siguiente es el símbolo en bruto que el tiempo de ejecución busca y llama:

```c
extern uint64_t entrypoint(const uint8_t *input)
```

Este punto de entrada toma una matriz de bytes genérica que contiene los parámetros del programa (id de programa, cuentas, datos de instrucción, etc...). Para deserializar los parámetros que cada cargador contiene su propia función [helper](#Serialization).

Consulte [el uso del punto de entrada por parte de Helloworld](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L37) como ejemplo de cómo encajan las cosas.

### Serialización

Consulte [el uso de la función de deserialización por parte de Helloworld de deserialización](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L43).

Cada cargador proporciona una función de ayuda que deserializa los parámetros de entrada del programa en tipos C:

- [Deserialización del cargador BPF](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L304)
- [Deserialización obsoleta de BPF Loader](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/deserialize_deprecated.h#L25)

Algunos programas pueden querer realizar la deserialzaiton ellos mismos y pueden hacerlo proporcionando su propia implementación del [punto de entrada crudo](#program-entrypoint). Tenga en cuenta que las funciones de deserialización proporcionadas conservan las referencias a la matriz de bytes serializada para las variables que el programa está autorizado a modificar (lamports, datos de la cuenta). La razón de esto es que al devolver el cargador leerá esas modificaciones para que puedan ser confirmadas. Si un programa implementa su propia función de deserialización necesita asegurarse de que cualquier modificación que el programa desee realizar debe ser escrita de nuevo en la matriz de bytes de entrada.

Los detalles sobre cómo el cargador serializa las entradas del programa se pueden encontrar en los documentos [Serialización de parámetros de entrada](overview.md#input-parameter-serialization).

## Tipos de datos

La función de ayuda de deserialización del cargador llena la estructura [SolParámetros](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L276):

```c
/**
 * Structure that the program's entrypoint input data is deserialized into.
 /
typedef struct {
  SolAccountInfo* ka; /** Pointer to an array of SolAccountInfo, must already
                          point to an array of SolAccountInfos */
  uint64_t ka_num; /** Number of SolAccountInfo entries in `ka` */
  const uint8_t *data; /** pointer to the instruction data */
  uint64_t data_len; /** Length in bytes of the instruction data */
  const SolPubkey *program_id; /** program_id of the currently executing program */
} SolParameters;
```

'ka' es una matriz ordenada de las cuentas referenciadas por la instrucción y representada como un [SolAccountInfo](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L173) estructuras. El lugar que ocupa una cuenta en la matriz significa su significado, por ejemplo, al transferir lamports una instrucción puede definir la primera cuenta como origen y la segunda como destino.

Los miembros de la estructura `SolAccountInfo` son de solo lectura excepto los `lamports` y `datos`. Ambos pueden ser modificados por el programa de acuerdo con la [política de ejecución](developing/programming-model/accounts.md#policy). Cuando una instrucción hace referencia a la misma cuenta varias veces puede haber entradas `SolAccountInfo` duplicadas en la matriz pero ambas apuntan de nuevo a la matriz de bytes de entrada original. Un programa debería manejar estos casos con delicadeza para evitar que se superpongan lecturas/escrituras al mismo búfer. Si un programa implementa su propia función de deserialización debe tomarse cuidado para manejar las cuentas duplicadas apropiadamente.

`data` es la matriz de bytes de propósito general de la [datos de la instrucción](developing/programming-model/transactions.md#instruction-data) que se está procesando.

`program_id` es la clave pública del programa en ejecución.

## Heap

Los programas C pueden asignar memoria a través de la llamada al sistema [`calloc`](https://github.com/solana-labs/solana/blob/c3d2d2134c93001566e1e56f691582f379b5ae55/sdk/bpf/c/inc/solana_sdk.h#L245) o implementar su propio heap sobre la región del heap de 32KB comenzando en la dirección virtual x3000000. La región del heap también es utilizada por `calloc` por lo que si un programa implementa su propio heap no debería llamar también a `calloc`.

## Ingresando

El tiempo de ejecución proporciona dos llamadas del sistema que toman datos y lo registran en los registros del programa.

- [`sol_log(const char*)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L128)
- [`sol_log_64(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L134)

La sección [depuración](debugging.md#logging) tiene más información sobre cómo trabajar con los registros del programa.

## Calcular presupuesto

Utilice la llamada del sistema [`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/bpf/c/inc/solana_sdk.h#L140) para registrar un mensaje conteniendo el número restante de unidades de computación que el programa puede consumir antes de detener la ejecución

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## ELF Dump

Los internos de los objetos compartidos de BPF pueden volcarse a un archivo de texto para obtener más información sobre la composición de un programa y lo que puede estar haciendo en tiempo de ejecución. El dump contendrá tanto la información ELF como una lista de todos los símbolos y las instrucciones que los implementan. Algunos de los mensajes de registro de errores del cargador BPF harán referencia a los números de instrucción específicos en los que se produjo el error. Estas referencias pueden buscarse en el volcado del ELF para identificar la instrucción infractora y su contexto.

Para crear un archivo dump:

```bash
$ cd <program directory>
$ make dump_<program name>
```

## Ejemplos

El repositorio de la biblioteca de programas de [Solana](https://github.com/solana-labs/solana-program-library/tree/master/examples/c) contiene una colección de ejemplos de C
