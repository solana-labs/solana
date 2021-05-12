---
title: Política de retrocompatibilidad
---

A medida que el ecosistema de desarrolladores de Solana crece, también lo hace la necesidad de expectativas claras en torno a los cambios de comportamiento y de la API que afectan a las aplicaciones y las herramientas creadas para Solana. En un mundo perfecto, el desarrollo de Solana podría continuar a un ritmo muy rápido sin causar nunca problemas a los desarrolladores existentes. Sin embargo, habrá que hacer algunas concesiones por lo que este documento intenta aclarar y codificar el proceso para las nuevas versiones.

### Expectativas

- Las versiones de software de Solana incluyen APIs, SDKs y herramientas CLI (con algunas [excepciones](#exceptions)).
- Las versiones de software de Solana siguen las versiones semánticas, más detalles a continuación.
- El software de una versión `MINOR` será compatible con todo el software de la misma versión `MAJOR`.

### Proceso de eliminación

1. En cualquier versión `PATCH` o `MINOR`, una característica, una API, un endpoint etc. podrían marcarse como obsoletos.
2. Según la dificultad de actualización del código, algunas características quedarán obsoletas durante algunos ciclos de lanzamiento.
3. En una futura versión de `MAJOR`, las funciones obsoletas se eliminarán de forma incompatible.

### Cadencia de liberación

La API RPC de Solana, el SDK de Rust, las herramientas CLI y el SDK del programa BPF se actualizan y se envían junto con cada versión del software de Solana y siempre deberían ser compatibles entre las actualizaciones de `PATCH` de una versión particular de `MINOR`.

#### Canales de liberación

- Software `edge` que contiene funciones de última generación sin política de retrocompatibilidad
- software `beta` que se ejecuta en el clúster Solana Tour de SOL testnet
- software `stable` que se ejecutan en los clusters Solana Mainnet Beta y Devnet

#### Lanzamientos Mayores (x.0.0)

Los lanzamientos de versiones `MAJOR` (por ejemplo, 2.0.0) pueden contener cambios de ruptura y la eliminación de características previamente obsoletas. Los SDK y las herramientas del cliente comenzarán a utilizar las nuevas funciones y puntos finales que se habilitaron en la versión anterior de `MAJOR`.

#### Lanzamientos menores (1.x.0)

Las nuevas funcionalidades y las implementaciones de las propuestas se añaden a _nuevos_ `MINOR` lanzamientos de versiones (por ejemplo, 1.4.0) y se ejecutan primero en el clúster de la red de pruebas de Solana. Mientras se ejecuta en la red de pruebas, las versiones `MINOR` se consideran en el canal de lanzamiento `beta`. Después de que esos cambios se hayan parcheado según sea necesario y se haya demostrado que son fiables, la versión `MINOR` se actualizará al canal de lanzamiento `stable` y se desplegará en el clúster Beta de la Mainnet.

#### Lanzamientos de parches (1.0.x)

Las funciones de bajo riesgo, los cambios que no rompen el sistema y las correcciones de seguridad y de errores se envían como parte de las versiones `PATCH` (por ejemplo, 1.0.11). Los parches pueden aplicarse tanto a los canales de liberación `beta` como `stable`.

### RPC API

Lanzamientos de parches:
- Corrección de errores
- Correcciones de seguridad
- Endpoint / eliminación de funciones

Lanzamientos menores:
- Nuevos endpoints y funciones de RPC

Lanzamientos Mayores:
- Eliminación de características obsoletas

### Cajas Rust

* [`solana-sdk`](https://docs.rs/solana-sdk/) - SDK de Rust para crear transacciones y analizar estados de cuentas
* [`solana-programa`](https://docs.rs/solana-program/) - Rust SDK para programas de escritura
* [`solana-client`](https://docs.rs/solana-client/) - Cliente de Rust para conectar a la API RPC
* [`solana-cli-config`](https://docs.rs/solana-cli-config/) - Cliente de Rust para administrar archivos de configuración del Solana CLI

Lanzamientos de parches:
- Corrección de errores
- Correcciones de seguridad
- Mejoras de rendimiento

Lanzamientos menores:
- Nuevas APIs

Lanzamientos Mayores
- Eliminación de APIs obsoletas
- Cambios de comportamiento incompatibles con el pasado

### Herramientas CLI

Lanzamientos de parches:
- Corrección de errores y seguridad
- Mejoras de rendimiento
- Subcomando / argumento obsoleto

Lanzamientos menores:
- Nuevos subcomandos

Lanzamientos Mayores:
- Cambio a los nuevos endpoints / configuración de la API RPC introducidos en la versión principal anterior.
- Eliminación de características obsoletas

### Características del tiempo de ejecución

Las nuevas funciones del tiempo de ejecución de Solana se activan manualmente. Las características del tiempo de ejecución incluyen: la introducción de nuevos programas nativos, sysvars y syscalls; y cambios en su comportamiento. La activación de las funciones es independiente del clúster, lo que permite crear confianza en Testnet antes de la activación en Mainnet-beta.

El proceso de liberación es el siguiente:

1. La nueva función de tiempo de ejecución se incluye en una nueva versión, desactivada por defecto
2. Una vez que un número suficiente de validadores con stake se actualizan a la nueva versión, el interruptor de funciones en tiempo de ejecución se activa manualmente con una instrucción
3. La característica tiene efecto al comienzo de la siguiente época

### Cambios de infraestructura

#### Nodos públicos de la API

Solana proporciona nodos RPC API disponibles públicamente para todos los desarrolladores. El equipo de Solana hará todo lo posible para comunicar cualquier cambio en el host, el puerto, la disponibilidad, etc. Sin embargo, recomendamos que los desarrolladores confíen en sus propios nodos validadores para desalentar la dependencia de los nodos operados por Solana.

#### Scripts de clúster locales e imágenes Docker

Los cambios de ruptura se limitarán a las actualizaciones de versión `MAJOR`. Las actualizaciones `MINOR` y `PATCH` deben ser siempre compatibles con el pasado.

### Excepciones

#### SDK de JavaScript Web3

El SDK de Web3.JS también sigue las especificaciones de versiones semánticas, pero se envía por separado de las versiones de software de Solana.

#### Vectores de Ataque

Si se descubre un nuevo vector de ataque en el código existente, se pueden eludir los procesos anteriores para desplegar rápidamente una solución, dependiendo de la gravedad del problema.
