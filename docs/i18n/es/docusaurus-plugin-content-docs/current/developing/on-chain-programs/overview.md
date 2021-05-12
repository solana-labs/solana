---
title: "Vista general"
---

Los desarrolladores pueden escribir y desplegar sus propios programas en la cadena de bloques Solana.

El [ejemplo de Helloworld](examples.md#helloworld) es un buen punto de partida para ver cómo se escribe un programa, construido, desplegado e interactuado con on-chain.

## Filtro de embalaje de Berkley (BPF)

Los programas en cadena de Solana se compilan a través del [compilador LLVM infraestructura](https://llvm.org/) a un [Formato Ejecutable y Enlazable (ELF)](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format) que contiene una variación del [Filtro de paquetes Berkley (BPF)](https://en.wikipedia.org/wiki/Berkeley_Packet_Filter) bytecode.

Dado que Solana utiliza la infraestructura del compilador LLVM, un programa puede escribirse en cualquier lenguaje de programación que pueda dirigirse al backend BPF de LLVM. Solana actualmente soporta programas de escritura en Rust y C/C++.

BPF proporciona un eficiente [conjunto de instrucciones](https://github.com/iovisor/bpf-docs/blob/master/eBPF.md) que puede ser ejecutado en una máquina virtual interpretada o como eficientes instrucciones nativas compiladas justo a tiempo.

## Mapa de memoria

El mapa de memoria de direcciones virtuales utilizado por los programas de Solana BPF es fijo y está dispuesto de la siguiente manera

- El código del programa comienza a 0x100000000
- La pila de datos comienza al 0x2000000
- Los datos del montón comienzan a las 0x3000000
- Los parámetros de entrada del programa comienzan a 0x400000000

Las direcciones virtuales anteriores son direcciones de inicio, pero los programas tienen acceso a un subconjunto del mapa de memoria. El programa entrará en pánico si intenta leer o escribir en una dirección virtual a la que no se le ha concedido acceso, y se devolverá un error `AccessViolation` que contiene la dirección y el tamaño de la violación intentada.

## Pila

BPF utiliza marcos de pila en lugar de un puntero de pila variable. Cada marco de pila tiene 4KB de tamaño.

Si un programa viola el tamaño del marco de pila, el compilador reportará la sobreescritura como advertencia.

For example: `Error: Function _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E Stack offset of -30728 exceeded max offset of -4096 by 26632 bytes, please minimize large stack variables`

El mensaje identifica qué símbolo excede su marco de pila, pero el nombre puede ser manipulado si es un símbolo Rust o C++. Para desglosar un símbolo de Rust usa [roustfilt](https://github.com/luser/rustfilt). La advertencia anterior proviene de un programa de Rust, por lo que el nombre del símbolo desangulado es:

```bash
$ rustfilt _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E
curve25519_dalek::edwards::EdwardsBasepointTable::create
```

Para desglosar un símbolo C++ usa `c++filt` de binutils.

La razón por la que se informa de una advertencia en lugar de un error es porque algunos crates dependientes pueden incluir funcionalidad que viola las restricciones del marco de la pila, incluso si el programa no utiliza esa funcionalidad. Si el programa viola el tamaño de la pila en tiempo de ejecución, se reportará un error `AccessViolation`.

Los marcos de pila de BPF ocupan un rango de direcciones virtual a partir de 0x200000000.

## Profundidad de llamada

Los programas están limitados a ejecutar rápidamente, y para facilitar esto, la pila de llamadas del programa está limitada a una profundidad máxima de 64 marcos.

## Pila

Los programas tienen acceso a una pila de tiempo de ejecución directamente en C o a través de las APIs de Rust `alloc`. Para facilitar las asignaciones rápidas, se utiliza un simple bump heap de 32KB. La pila no soporta `free` o `realloc` así que úsalo con prudencia.

Internamente, los programas tienen acceso a la región de memoria de 32KB comenzando en la dirección virtual 0x3000000 y pueden implementar una pila personalizada basada en las necesidades específicas del programa.

- [Uso en heap del programa Rust](developing-rust.md#heap)
- [Uso en heap del programa C](developing-c.md#heap)

## Soporte de float

Programs support a limited subset of Rust's float operations, if a program attempts to use a float operation that is not supported, the runtime will report an unresolved symbol error.

Float operations are performed via software libraries, specifically LLVM's float builtins. Due to be software emulated they consume more compute units than integer operations. In general, fixed point operations are recommended where possible.

The Solana Program Library math tests will report the performance of some math operations: https://github.com/solana-labs/solana-program-library/tree/master/libraries/math

To run the test, sync the repo, and run:

`$ cargo test-bpf -- --nocapture --test-threads=1`

Recent results show the float operations take more instructions compared to integers equivalents. Fixed point implementations may vary but will also be less then the float equivalents:

```
         u64   f32
Multipy    8   176
Divide     9   219
```

## Datos Estáticos Escribibles

Los objetos compartidos del programa no soportan datos compartidos escribibles. Los programas se comparten entre múltiples ejecuciones paralelas utilizando el mismo código y datos compartidos de sólo lectura. Esto significa que los desarrolladores no deben incluir ninguna variable estática escribible o global en los programas. En el futuro se podría añadir un mecanismo de copia sobre escritura para soportar datos escribibles.

## División firmada

El conjunto de instrucciones BPF no soporta [la división firmada](https://www.kernel.org/doc/html/latest/bpf/bpf_design_QA.html#q-why-there-is-no-bpf-sdiv-for-signed-divide-operation). Añadir una instrucción de división firmada es una consideración.

## Cargadores

Los programas son desplegados y ejecutados por los cargadores de tiempo de ejecución, actualmente hay dos cargadores soportados [BPF cargador](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) y [cargador BPFdesaprobado](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

Los cargadores pueden soportar diferentes interfaces binarias de aplicación, por lo que los desarrolladores deben escribir sus programas e implementarlos en el mismo cargador. Si un programa escrito para un cargador es desplegado a otro diferente, el resultado suele ser un error `AccessViolation` debido a la deserialización incorrecta de los parámetros de entrada del programa.

Para todos los fines prácticos, el programa siempre debe escribirse para dirigirse al último cargador BPF y el último cargador es el predeterminado para la interfaz de línea de comandos y las API de javascript.

Para obtener información específica sobre la implementación de un programa para un cargador en particular, consulte:

- [Puntos de entrada del programa Rust](developing-rust.md#program-entrypoint)
- [Puntos de entrada del programa C](developing-c.md#program-entrypoint)

### Implementación

La implementación del programa BPF es el proceso de subir un objeto compartido BPF a los datos de una cuenta de programa y marcar el ejecutable de la cuenta. Un cliente divide el objeto compartido BPF en piezas más pequeñas y los envía como los datos de instrucción de [`Escribe`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L13) instrucciones al cargador donde el cargador escribe esos datos en los datos de la cuenta del programa. Una vez recibidas todas las piezas, el cliente envía una instrucción [`Finalizar`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L30) al cargador, el cargador entonces valida que los datos BPF sean válidos y marca la cuenta del programa como _ejecutable_. Una vez que la cuenta del programa se marca como ejecutable, las transacciones posteriores pueden emitir instrucciones para que ese programa sea procesado.

Cuando una instrucción se dirige a un programa BPF ejecutable, el cargador configura el entorno de ejecución del programa, serializa los parámetros de entrada del programa, llama al punto de entrada del programa e informa de los errores encontrados.

Para más información consulte [despliegue](deploying.md)

### Serialización de parámetros de entrada

Los cargadores de BPF serializan los parámetros de entrada del programa en una matriz de bytes que luego se pasa al punto de entrada del programa, donde éste se encarga de deserializarla en la cadena. Uno de los cambios entre el cargador obsoleto y el cargador actual es que los parámetros de entrada se serializan de una manera que resulta en varios parámetros que caen en offsets alineados dentro de la matriz de bytes alineados. Esto permite a las implementaciones de deserialización referenciar directamente la matriz de bytes y proporcionar punteros alineados al programa.

Para información específica sobre el idioma acerca de la serialización, ver:

- [Deserialización de parámetros de programas en Rust](developing-rust.md#parameter-deserialization)
- [Deserialización de parámetros de programas en C](developing-c.md#parameter-deserialization)

El último cargador serializa los parámetros de entrada del programa como sigue (toda la codificación es little endian):

- Número de cuentas de 8 bytes sin signo
- Para cada cuenta
  - 1 byte que indica si se trata de una cuenta duplicada, si no es un duplicado el valor es 0xff, en caso contrario el valor es el índice de la cuenta de la que es un duplicado.
  - 7 bytes de relleno
    - si no es duplicado
      - 1 byte padding
      - 1 byte boolean, verdadero si la cuenta es un firmante
      - 1 byte boolean, verdadero si la cuenta es escribible
      - 1 byte boolean, verdadero si la cuenta es ejecutable
      - 4 bytes de relleno
      - 32 bytes de la clave pública de la cuenta
      - 32 bytes de la clave pública del propietario de la cuenta
      - Número sin signo de 8 bytes de lamports propiedad de la cuenta
      - 8 bytes sin signo de número de bytes de datos de cuenta
      - x bytes de datos de la cuenta
      - 10k bytes de relleno, usados para realloc
      - suficiente relleno para alinear el desplazamiento a 8 bytes.
      - 8 bytes de alquiler de época
- 8 bytes de datos de número de instrucción sin signo
- x bytes de datos de instrucciones
- 32 bytes del id del programa
