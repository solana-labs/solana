---
title: Instalación y actualizaciones de software del clúster
---

Actualmente se requiere a los usuarios que construyan el software de clúster de solana desde el repositorio de git y lo actualicen manualmente, que es propenso a errores e inconveniente.

Este documento propone un programa de instalación y actualización fácil de usar que se puede utilizar para desplegar binarios precompilados para plataformas soportadas. Los usuarios pueden optar por utilizar binarios suministrados por Solana o cualquier otra parte en la que confíen. El despliegue de actualizaciones se gestiona mediante un programa manifest de actualización en cadena.

## Motivando Ejemplos

### Obtener y ejecutar un instalador precompilado usando un script de arranque curl/shell

El método de instalación más fácil para las plataformas soportadas:

```bash
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh
```

Este script revisará github para ver la última versión etiquetada y descargará y ejecutará el binario `solana-install-init` desde allí.

Si se necesitan argumentos adicionales para ser especificados durante la instalación, se utiliza la siguiente sintaxis de shell:

```bash
$ init_args=.... # argumentos para `solana-install-init ...`
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh -s - ${init_args}
```

### Obtener y ejecutar un instalador precompilado desde una versión de Github

Con una URL de versión conocida, se puede obtener un binario precompilado para las plataformas soportadas:

```bash
$ curl -o solana-install-init https://github.com/solana-labs/solana/releases/download/v1.0.0/solana-install-init-x86_64-apple-darwin
$ chmod +x ./solana-install-init
$ ./solana-install-init --help
```

### Crear y ejecutar el instalador desde el código fuente

Si un binario precompilado no está disponible para una plataforma determinada, construir el instalador desde el código fuente es siempre una opción:

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana/install
$ cargo run -- --help
```

### Desplegar una nueva actualización a un clúster

Dado un tarball de lanzamiento de solana \ (creado por ` ci / publish-tarball.sh ` \) que ya se ha subido a una URL de acceso público, los siguientes comandos implementarán la actualización:

```bash
$ solana-keygen new -o update-manifest.json # <-- sólo se genera una vez, la clave pública se comparte con los usuarios
$ despliegue de solana-install http://example.com/path/to/solana-release.tar.bz2 update-manifest.json
```

### Ejecutar un nodo validador que se actualiza automáticamente

```bash
$ solana-install init --pubkey 92DMonmBYXwEMHJ99c9ceRSpAmk9v6i3RdvDdXaVcrfj # <-- pubkey se obtiene de quien despliegue las actualizaciones
$ export PATH=~/. ocal/share/solana-install/bin:$PATH
$ solana-keygen . . # <-- ejecuta la última solana-keygen
$ solana-install run solana-validator . . # <-- ejecuta un validador, reiniciándolo como necesario cuando se aplica una actualización
```

## Manifiesto de actualización en cadena

Se utiliza un manifiesto de actualización para anunciar el despliegue de nuevos tarballs de lanzamiento en un clúster solana. El manifiesto de actualización se almacena mediante el programa ` config `, y cada cuenta de manifiesto de actualización describe un canal de actualización lógico para un triple objetivo determinado \ (p. Ej., ` x86_64-apple-darwin ` \). La clave pública de la cuenta es bien conocida entre la entidad que implementa nuevas actualizaciones y los usuarios que las consumen.

El tarball de actualización en sí está alojado en otro lugar, fuera de la cadena y se puede obtener desde el `download_url` especificado.

```text
use solana_sdk::signature::Signature;

/// Información necesaria para descargar y aplicar una actualización determinada
pub struct UpdateManifest {
     pub timestamp_secs: u64, // Cuando se implementó la versión en segundos desde UNIX EPOCH
     pub download_url: String, // URL de descarga a la versión tar.bz2
     pub download_sha256: String, // resumen SHA256 del archivo tar.bz2 de la versión
}

/// Datos de una cuenta de programa de manifiesto de actualización.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct SignedUpdateManifest {
    pub manifest: UpdateManifest,
    pub manifest_signature: Firma,
}
```

Tenga en cuenta que el campo `manifiesto` contiene una firma correspondiente \(`manifest_signature`\) para proteger contra ataques man-in-the-middle entre la herramienta `solana-install` y la API RPC del clúster solana.

Para protegerse contra ataques de reversión, `solana-install` se negará a instalar una actualización con una antigua `timestamp_seg` de lo que está instalado.

## Publicar contenido del archivo

Se espera que un archivo de liberación sea un archivo rar comprimido con bzip2 con la siguiente estructura interna:

- `/version.yml` - un simple archivo YAML que contiene el campo `"target"` - el

  tupla de destino. Cualquier campo adicional es ignorado.

- `/bin/` -- directorio que contiene programas disponibles en la versión.

  `solana-install` enlazará este directorio a

  `~/.local/share/solana-install/bin` para su uso por el entorno `PATH`

  variable.

- `...` -- cualquier archivo y directorio adicionales están permitidos

## herramienta de instalación de solana

La herramienta `solana-install` es utilizada por el usuario para instalar y actualizar su software de clúster.

Administra los siguientes archivos y directorios en el directorio principal del usuario:

- `~/.config/solana/install/config.yml` - configuración de usuario e información sobre la versión del software actualmente instalada
- `~/.local/share/solana/install/bin` - un enlace simbólico a la versión actual. eg, `~/.local/share/solana-update/<update-pubkey>-<manifest_signature>/bin`
- `~/.local/share/solana/install/releases/<download_sha256>/` - contenido de una versión

### Interfaz de línea de comandos

```text
solana-install 0.16.
El instalador de software de clúster de solana

USAGE:
    solana-install [OPTIONS] <SUBCOMMAND>

FLAGS:
    -h, --help Imprime información de ayuda
    -V, --version Imprime información de versión

OPCIONES:
    -c, --config <PATH>    Archivo de configuración a usar [por defecto: . ./Library/Preferences/solana/install. ml]

SUBCOMMANDS:
    despliegue una nueva actualización
    ayuda Imprime este mensaje o la ayuda de los subcomando(s) dados
    información muestra información sobre la instalación actual
    init inicializa una nueva instalación
    ejecuta un programa mientras revisa periódicamente y aplica actualizaciones de software
    revisa la actualización, y si hay descargas disponibles y aplicarlo
```

```text
solana-install-init
inicializa una nueva instalación

USAGE:
    solana-install init [OPTIONS]

FLAGS:
    -h, --help Imprime información de ayuda

OPCIONES:
    -d, --data_dir <PATH>    Directorio para almacenar datos de instalación [por defecto: . ./Biblioteca/Soporte de Aplicaciones/solana]
    -u, --url <URL>          URL JSON RPC para el clúster solana [por defecto: http://devnet. olana.com]
    -p, --pubkey <PUBKEY>    Clave pública del manifiesto de actualización [por defecto: 9X329sPuskWhH4DQh6k16c87dHKhXLBZTL3Gxmve8Gp]
```

```text
información de solana-install
muestra información sobre la instalación actual

USAGE:
    información de solana-install [FLAGS]

FLAGS:
    -h, --help Imprime información de ayuda
    -l, --local sólo mostrar información local, no comprobar el clúster para nuevas actualizaciones
```

```text
solana-install deploy
implementa una nueva actualización

USAGE:
    despliegue solana-install <download_url> <update_manifest_keypair>

FLAGS:
    -h, --help Prints help information

ARGS:
    <download_url>               URL al archivo de solana release
    <update_manifest_keypair>    Keypair file for the update manifest (/path/to/keypair. hijo)
```

```text
la actualización de solana-install
busca una actualización, y si está disponible las descargas y lo aplica

USAGE:
    solana-install update

FLAGS:
    -h, --help Imprime información de ayuda
```

```text
solana-install run
Ejecuta un programa mientras verifica periódicamente y aplica actualizaciones de software

USAGE:
    solana-install run <program_name> [program_arguments]...

FLAGS:
    -h, --help Imprime información de ayuda

ARGS:
    <program_name>            programa para ejecutar
    <program_arguments>.. argumentos para proporcionar al programa

El programa se reiniciará con una actualización de software exitosa
```
