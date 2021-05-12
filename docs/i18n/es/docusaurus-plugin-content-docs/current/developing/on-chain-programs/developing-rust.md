---
title: "Desarrollando con Rust"
---

Solana soporta la escritura de programas en cadena usando el lenguaje de programación [Rust](https://www.rust-lang.org/).

## Diseño del proyecto

Los programas Solana Rust siguen el típico [ diseño de proyecto Rust](https://doc.rust-lang.org/cargo/guide/project-layout.html):

```
/inc/
/src/
/Cargo.toml
```

Pero también debe incluir:

```
/Xargo.toml
```

Que debe contener:

```
[target.bpfel-unknown-unknown.dependencies.std]
features = []
```

Solana Rust programs may depend directly on each other in order to gain access to instruction helpers when making [cross-program invocations](developing/programming-model/calling-between-programs.md#cross-program-invocations). Al hacerlo, es importante no tirar los símbolos del punto de entrada del programa dependiente porque pueden entrar en conflicto con el propio programa. To avoid this, programs should define an `exclude_entrypoint` feature in `Cargo.toml` and use to exclude the entrypoint.

- [Defina la función](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/Cargo.toml#L12)
- [Excluir el punto de entrada](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/src/lib.rs#L12)

Luego, cuando otros programas incluyan este programa como una dependencia, deberían hacerlo usando la función `exclude_entrypoint`.

- [Incluir sin punto de entrada](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token-swap/program/Cargo.toml#L19)

## Dependencias del proyecto

Como mínimo, los programas Solana Rust deben llevar la caja de [solana-programa](https://crates.io/crates/solana-program).

Los programas BPF de Solana tienen algunas [restricciones](#restrictions) que pueden prevenir la inclusión de algunas cajas como dependencias o requieren un manejo especial.

Por ejemplo:

- Los contenedores que requieren la arquitectura son un subconjunto de los soportados por la cadena de herramientas oficial. No hay ninguna solución para esto a menos que esa caja sea bifurcada y BPF añadido a esas comprobaciones de arquitectura.
- Las caudales pueden depender de `rand` el cual no está soportado en el entorno determinístico del programa de Solana. Para incluir una caja dependiente de `rand`, consulte [Dependiendo en Rand](#depending-on-rand).
- Los índices pueden desbordar la pila incluso si el código sobrecargado no está incluido en el programa. Para obtener más información, consulte [Pila](overview.md#stack).

## Cómo construir

Primero configura el entorno:

- Instalar la última versión estable de Rust desde https://rustup.rs/
- Instale las últimas herramientas de línea de comandos de Solana desde https://docs.solana.com/cli/install-solana-cli-tools

La compilación normal de cargo está disponible para construir programas contra su máquina host que puede ser utilizada para pruebas unitarias:

```bash
$ cargo build
```

Para construir un programa específico, como un SPL Token, para el objetivo Solana BPF que puede ser desplegado en el cluster:

```bash
$ cd <the program directory>
$ carga build-bpf
```

## Cómo probarlo

Los programas de Solana pueden ser probados por unidad a través del mecanismo tradicional de `cargo test` por funciones del programa de ejercicio directamente.

Para ayudar a facilitar las pruebas en un entorno que coincida más estrechamente con un clúster en vivo, los desarrolladores pueden usar la caja [`prueba de programa`](https://crates.io/crates/solana-program-test). El crate `program-test` inicia una instancia local del tiempo de ejecución y permite a las pruebas enviar múltiples transacciones manteniendo el estado durante la duración de la prueba.

Para más información el [ejemplo de prueba en sysvar](https://github.com/solana-labs/solana-program-library/blob/master/examples/rust/sysvar/tests/functional.rs) muestra cómo una instrucción que contiene cuenta sysvar es enviada y procesada por el programa.

## Entrypoint del programa

Los programas exportan un símbolo de punto de entrada conocido que el tiempo de ejecución de Solana busca y llama al invocar un programa. Solana soporta múltiples [versiones del cargador BPF](overview.md#versions) y los puntos de entrada pueden variar entre ellos. Los programas deben escribirse para el mismo cargador y desplegarse en él. Para más detalles vea el [resumen](overview#loaders).

Actualmente hay dos cargadores soportados [BPF cargador](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) y [cargador BPF obsoleto](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

Ambos tienen la misma definición de punto de entrada en bruto, el siguiente es el símbolo en bruto que el tiempo de ejecución busca y llama:

```rust
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64;
```

Este punto de entrada toma una matriz de bytes genérica que contiene los parámetros del programa (id de programa, cuentas, datos de instrucción, etc...). Para deserializar los parámetros que cada cargador contiene su propia macro envolvente que exporta el punto de entrada crudo, deserializa los parámetros, llama a la función de procesamiento de instrucción definida por el usuario y retorna los resultados.

Puedes encontrar las macros de punto de entrada aquí:

- [Macro de punto de entrada del Cargador BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L46)
- [El Cargador BPF desaprobó la macro de punto de entrada](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L37)

La función de procesamiento de instrucciones definida por el programa que las macros de punto de entrada deben ser de este formulario:

```rust
pub type ProcessInstruction =
    fn(program_id: &Pubkey, cuentas: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult;
```

Consulte [el uso del punto de entrada por parte de Helloworld](https://github.com/solana-labs/example-helloworld/blob/c1a7247d87cd045f574ed49aec5d160aefc45cf2/src/program-rust/src/lib.rs#L15) como ejemplo de cómo encajan las cosas.

### Deserialización de parámetros

Cada cargador proporciona una función de ayuda que deserializa los parámetros de entrada del programa en tipos Rust. Las macros del punto de entrada llaman automáticamente al ayudante de deserialización:

- [Deserialización del cargador BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L104)
- [Deserialización obsoleta de BPF Loader](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L56)

Algunos programas pueden querer realizar la deserialzaiton ellos mismos y pueden hacerlo proporcionando su propia implementación del [punto de entrada crudo](#program-entrypoint). Tenga en cuenta que las funciones de deserialización proporcionadas conservan las referencias a la matriz de bytes serializada para las variables que el programa está autorizado a modificar (lamports, datos de la cuenta). La razón de esto es que al devolver el cargador leerá esas modificaciones para que puedan ser confirmadas. Si un programa implementa su propia función de deserialización, debe asegurarse de que cualquier modificación que el programa desee realizar se escriba de nuevo en la matriz de bytes de entrada.

Los detalles sobre cómo el cargador serializa las entradas del programa se pueden encontrar en los documentos [Serialización de parámetros de entrada](overview.md#input-parameter-serialization).

### Tipos de datos

Las macros de punto de entrada del cargador llaman a la función del procesador de instrucciones definido con los siguientes parámetros:

```rust
program_id: &Pubkey,
cuentas: &[AccountInfo],
instruction_data: &[u8]
```

El program_id es la clave pública del programa en ejecución.

Las cuentas son una porción ordenada de las cuentas referenciadas por la instrucción y representadas como un [AccountInfo](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/account_info.rs#L10) estructuras. El lugar que ocupa una cuenta en la matriz significa su significado, por ejemplo, al transferir lamports una instrucción puede definir la primera cuenta como origen y la segunda como destino.

Los miembros de la estructura `AccountInfo` son de solo lectura excepto los `lamports` y `datos`. Ambos pueden ser modificados por el programa de acuerdo con la [política de ejecución](developing/programming-model/accounts.md#policy). Ambos de estos miembros están protegidos por la estructura de Rust `RefCell`, así que deben ser tomados prestados para leerlos o escribirlos. La razón de esto es que ambos apuntan a la matriz de bytes de entrada original, pero puede haber múltiples entradas en la porción de cuentas que apuntan a la misma cuenta. El uso de `RefCell` garantiza que el programa no realice accidentalmente lecturas/escrituras superpuestas en los mismos datos subyacentes a través de múltiples estructuras `AccountInfo`. Si un programa implementa su propia función de deserialización debe tomarse cuidado para manejar las cuentas duplicadas apropiadamente.

Los datos de la instrucción son la matriz de bytes de propósito general de la [datos de la instrucción](developing/programming model/transactions.md#instruction-data) que se está procesando.

## Heap

Los programas de Rust implementan el montón directamente definiendo un [`global_allocator`](https://github.com/solana-labs/solana/blob/8330123861a719cd7a79af0544617896e7f00ce3/sdk/program/src/entrypoint.rs#L50) personalizado

Los programas pueden implementar su propio `global_allocator` basado en sus necesidades específicas. Consulte el [ejemplo personalizado de heap](#examples) para más información.

## Restricciones

Los programas en cadena Rust soportan la mayor parte de Rust's libstd, libcore y liballoc, como así como muchas cajas de terceros.

Hay algunas limitaciones ya que estos programas se ejecutan en un entorno con recursos limitados, con un solo hilo y deben ser deterministas:

- Sin acceso a
  - `rand`
  - `std::fs`
  - `std::net`
  - `std::os`
  - `std::future`
  - `std::net`
  - `std::process`
  - `std::sync`
  - `std::task`
  - `std::thread`
  - `std::time`
- Acceso limitado a:
  - `std::hash`
  - `std::os`
- Bincode es extremadamente costoso computacionalmente en ambos ciclos y en la profundidad de llamada y debe evitarse
- El formato de cadena debe ser evitado, ya que también es computacionalmente caro.
- ¡No hay soporte para `println!`, `print!`[, los ayudantes de registro ](#logging) Solana deben ser usados en su lugar.
- El tiempo de ejecución impone un límite en el número de instrucciones que un programa puede ejecutar durante el procesamiento de una instrucción. See [computation budget](developing/programming-model/runtime.md#compute-budget) for more information.

## Dependiendo del Rand

Los programas están limitados a ejecutarse de forma determinista, por lo que no se dispone de números aleatorios. A veces un programa puede depender de un crate que a su vez depende de `rand` aunque el programa no utilice ninguna de las funcionalidades de números aleatorios. Si un programa depende de `rand`, la compilación fallará porque no hay soporte de `get-random` para Solana. El error normalmente se verá así:

```
error: el objetivo no está soportado, para más información ver: https://docs.rs/getrandom/#unsupported-targets
   --> /Users/jack/.cargo/registry/src/github.com-1ecc6299db9ec823/getrandom-0.1.14/src/lib.rs:257:9
    |
257 | /         compile_error!("\
258 | |             target is not supported, for more information see: \
259 | |             https://docs.rs/getrandom/#unsupported-targets\
260 | |         ");
    | |___________^
```

Para solucionar este problema de dependencias, agregue la siguiente dependencia al programa `Cargo.toml`:

```
getrandom = { versión = "0.1.14", features = ["dummy"] }
```

## Ingresando

¡La macro de Rust's `println!` es costosa para el cálculo y no es compatible. En su lugar, el macro ayudante [`msg!`](https://github.com/solana-labs/solana/blob/6705b5a98c076ac08f3991bb8a6f9fcb280bf51e/sdk/program/src/log.rs#L33) es proporcionado.

`msg!` tiene dos formas:

```rust
msg!("A string");
```

o

```rust
msg!(0_64, 1_64, 2_64, 3_64, 4_64);
```

Ambas formas muestran los resultados a los registros del programa. Si un programa lo desea puede emular `println!` by using `format!`:

```rust
msg!("Some variable: {:?}", variable);
```

La sección [depuración](debugging.md#logging) tiene más información sobre cómo trabajar con los registros del programa [ejemplos de Rust](#examples) contiene un ejemplo de registro.

## Pánico

¡El `panic!`, `assert!` y los resultados internos se imprimen en los [registros de programa](debugging.md#logging) por defecto.

```
INFO  solana_runtime::message_processor] Finalized account CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Call BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Program log: Panicked at: 'assertion failed: `(left == right)`
      left: `1`,
     right: `2`', rust/panic/src/lib.rs:22:5
INFO  solana_runtime::message_processor] BPF program consumed 5453 of 200000 units
INFO  solana_runtime::message_processor] BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ failed: BPF program panicked
```

### Manejador de pánico personalizado

Los programas pueden anular el manejador predeterminado proporcionando su propia implementación.

Primero defina la función `custom-panic` del programa `Cargo.toml`

```toml
[features]
default = ["custom-panic"]
custom-panic = []
```

Luego proporciona una implementación personalizada del manejador:

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    solana_program::msg!("program custom panic enabled");
    solana_program::msg!("{}", info);
}
```

En el fragmento anterior, se muestra la implementación por defecto, pero los desarrolladores pueden sustituirla por algo que se adapte mejor a sus necesidades.

Uno de los efectos secundarios de soportar mensajes de pánico completos por defecto es que los programas incurren en el costo de tirar de más de la implementación de `libstd` de Rust en el objeto compartido del programa. Los programas típicos ya estarán tirando de una buena cantidad de `libstd` y puede que no noten mucho el aumento del tamaño de los objetos compartidos. Pero los programas que explícitamente intentan ser muy pequeños evitando `libstd` pueden tener un impacto significativo (~25kb). Para eliminar ese impacto, los programas pueden proporcionar su propio manejador personalizado con una implementación vacía.

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    // Do nothing to save space
}
```

## Calcular presupuesto

Utilice la llamada del sistema [`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/program/src/log.rs#L102) para registrar un mensaje conteniendo el número restante de unidades de computación que el programa puede consumir antes de detener la ejecución

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## ELF Dump

Los internos de los objetos compartidos de BPF pueden volcarse a un archivo de texto para obtener más información sobre la composición de un programa y lo que puede estar haciendo en tiempo de ejecución. El dump contendrá tanto la información ELF como una lista de todos los símbolos y las instrucciones que los implementan. Algunos de los mensajes de registro de errores del cargador BPF harán referencia a los números de instrucción específicos en los que se produjo el error. Estas referencias pueden buscarse en el volcado del ELF para identificar la instrucción infractora y su contexto.

Para crear un archivo dump:

```bash
$ cd <program directory>
$ cargo build-bpf --dump
```

## Ejemplos

El repositorio de la biblioteca de programas de [Solana](https://github.com/solana-labs/solana-program-library/tree/master/examples/rust) contiene una colección de ejemplos de Rust.
