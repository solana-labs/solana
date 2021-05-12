---
title: Proceso de gestión de Solana ABI
---

Este documento propone el proceso de gestión de Solana ABI. El proceso de gestión de ABI es una práctica de ingeniería y un marco técnico de apoyo para evitar introducir cambios no deseados incompatibles con ABI.

# Problemas

La Solana ABI (interfaz binaria para el clúster) actualmente sólo está definida implícitamente por la implementación y requiere una atención muy cuidadosa para notar cambios de ruptura. Esto hace extremadamente difícil actualizar el software en un clúster existente sin reiniciar el ledger.

# Requerimientos y objetivos

- Los cambios no intencionados de ABI pueden detectarse como fallos mecánicos de CI.
- Una implementación más reciente debe ser capaz de procesar los datos más antiguos (desde genesis) una vez que vamos mainnet.
- El objetivo de esta propuesta es proteger el ABI al mismo tiempo que sostiene un desarrollo rápido optando por un proceso mecánico en lugar de un muy largo proceso de auditoría impulsado por el hombre.
- Una vez firmado criptográficamente, el blob de datos debe ser idéntico, por lo que no es posible actualizar el formato de datos en el lugar sin importar la entrada y salida de el sistema en línea. Además, teniendo en cuenta el gran volumen de transacciones que estamos tratando de manejar, la actualización retrospectiva en el lugar no es deseable en el mejor de los casos.

# Solución

En lugar de la diligencia debida del ojo del hombre natural, se debe asumir que falla regularmente, necesitamos una garantía sistemática de no romper el clúster cuando cambia el código fuente.

Para ese fin, introducimos un mecanismo para marcar todas las cosas relacionadas con ABI en el código fuente (`struct`s, `enum`s) con el nuevo atributo `#[frozen_abi]`. Este toma un valor digest de código duro derivado de tipos de sus campos a través de `ser::Serialize`. Y el atributo genera automáticamente una prueba unitaria para intentar detectar cualquier cambio no autorizado en las cosas marcadas relacionadas con el ABI.

Sin embargo, la detección no puede completarse; no importa lo difícil que analicemos estáticamente el código fuente, todavía es posible romper la ABI. Por ejemplo, esto incluye no-`derive`d escrito a mano `ser::Serialize`, la implementación de la biblioteca subyacente cambia (por ejemplo `bincode`), diferencias de arquitectura del CPU. La detección de estas posibles incompatibilidades ABI está fuera de alcance para esta gestión de ABI.

# Definiciones

Artículo/tipo ABI: varios tipos que se utilizarán para la serialización, que colectivamente incluye el ABI completo para cualquier componente del sistema. Por ejemplo, esos tipos incluyen `struct`s y `enum`s.

Resumen del elemento ABI: Algún hash fijo derivado de la información del tipo de los campos del elemento ABI.

# Ejemplos

```patch
+#[frozen_abi(digest="eXSMM7b89VY72V...")]
 #[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
 pub struct Vote {
     /// Una pila de votos que comienza con el voto más antiguo.
     pub slots: Vec<Slot>,
     ///firma del estado del banco en la última ranura
     pub hash: Hash,
 }
```

# Flujo de trabajo del desarrollador

Conocer el resumen de nuevos objetos ABI, los desarrolladores pueden añadir `frozen_abi` con un valor de resumen aleatorio y ejecutar las pruebas unitarias y reemplazarlo con el resumen correcto del mensaje de error de la prueba de afirmación.

En general, una vez que agregamos `frozen_abi` y su cambio se publica en el canal de lanzamiento estable, su resumen nunca debería cambiar. Si ese cambio es necesario, optaremos por definir una nueva estructura ``como`FooV1`. Y flujo de liberación especial como bifurcaciones duras.

# Comentarios de la implementación

Usamos cierto grado de maquinaria macro para generar automáticamente pruebas unitarias. y calcular un resumen de los elementos ABI. Esto se puede hacer mediante el uso inteligente de `serde::Serialize` (`[1]`) y `any::type_name` (`[2]`). Para un precedente similar de implementación, `ink` de las tecnologías de Parity `[3]` podría ser informativa.

# Detalles de implementación

El objetivo de la implementación es detectar los cambios no deseados en ABI automáticamente como sea posible. Para ese fin, el digest de información estructural de ABI se calcula con la mejor precisión y estabilidad del esfuerzo.

Cuando se ejecuta la comprobación de resumen de ABI, calcula dinámicamente un resumen de ABI digeriendo recursivamente el ABI de los campos del artículo ABI, al reutilizar la funcionalidad de serialización de `serde`, macro proc y especialización genérica. ¡Y entonces, la comprobación `assert!`s que su valor digest finalizado es idéntico como lo que se especifica en el atributo `frozen_abi`.

Para darse cuenta de eso, crea una instancia de ejemplo del tipo y un personalizado `Serializer` instancia para `serde` para recorrer recursivamente sus campos como si serializando el ejemplo de verdad. Este recorrido debe realizarse a través de `serde` para capturar realmente qué tipo de datos realmente serían serializados por `serde`, incluso considerando no personalizado-`derivive`d `Serialize` implementaciones de características.

# El proceso de digestión de ABI

Esta parte es un poco compleja. Hay tres partes interdependientes: `AbiExample`, `AbiDigester` y `AbiEnumVisitor`.

Primero, la prueba generada crea una instancia de ejemplo del tipo digerido con un rasgo llamado `AbiExample`, que debe implementarse para todos los tipos digeridos como el `Serialize` y devolver `Self` como el `default` rasgo. Generalmente, se proporciona mediante especialización genérica de rasgos para la mayoría de los tipos comunes. También es posible `derive` para `struct` y `enum` y puede ser escrito a mano si es necesario.

El `Serializer` personalizado se llama `AbiDigester`. Y cuando es llamado por `serde` para serializar algunos datos, recolecta recursivamente información ABI tanto como sea posible. `AbiDigester` El estado interno para el resumen ABI se actualiza de forma diferente dependiendo del tipo de datos. Esta lógica es específicamente redirigida a través de un trait llamado `AbiEnumVisitor` por cada tipo `enum`. Como sugiere el nombre de, no hay necesidad de implementar `AbiEnumVisitor` para otros tipos.

Para resumir esta interacción, `serde` gestiona el flujo del control de serialización recursivo junto con `AbiDigester`. El punto de entrada inicial en pruebas e hijo `AbiDigester`s utilizan `AbiExample` recursivamente para crear un gráfico jerárquico objeto de ejemplo. Y `AbiDigester` usa `AbiEnumVisitor` para consultar la información real ABI usando la muestra construida.

`Default` no es suficiente para `AbiExample`. La colección variada `::default()` está vacía, sin embargo, queremos digerirlos con elementos reales. Y, la digestión de ABI no puede realizarse solo con `AbiEnumVisitor`. `AbiExample` es requerido porque se necesita una instancia real de tipo para recorrer realmente los datos a través de `serde`.

Por otro lado, la digerción ABI no puede hacerse solo con `AbiEjemplo`, tampoco. `AbiEnumVisitor` es requerido porque todas las variantes de un `enum` no pueden ser atravesadas solo con una sola variante como ejemplo de ABI.

Información digestible:

- nombre del tipo de oxidación
- Nombre del tipo de datos de `serde`
- todos los campos en `struct`
- todas las variantes en `enum`
- `struct`: normal(`struct {...}`) y estilo tuple (`struct(...)`)
- `enum`: variantes normales y `struct`- y `tupla`- estilos.
- atributos: `serde(serialize_with=...)` y `serde(skip)`

Información no digerible:

- Cualquier ruta de serialización personalizada no tocada por la muestra proporcionada por `AbiExample`. (técnicamente no es posible)
- genéricos (debe ser un tipo de hormigón; use `frozen_abi` en alias del tipo de hormigón)

# Referencias

1. [(De)Serialización con información de tipo · Problema #1095 · serde-rs/serde](https://github.com/serde-rs/serde/issues/1095#issuecomment-345483479)
2. [`std::any::type_name` - Rust](https://doc.rust-lang.org/std/any/fn.type_name.html)
3. [La tinta de Paridad para escribir contratos inteligentes](https://github.com/paritytech/ink)
