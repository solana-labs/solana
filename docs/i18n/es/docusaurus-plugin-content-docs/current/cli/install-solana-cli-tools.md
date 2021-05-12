---
title: Instalar el paquete de herramientas de Solana
---

Hay varias maneras de instalar las herramientas Solana en tu ordenador dependiendo de tu flujo de trabajo preferido:

- [Usar la herramienta de instalación de Solana (opción más simple)](#use-solanas-install-tool)
- [Descargar Binarios Preconstruidos](#download-prebuilt-binaries)
- [Construir desde el origen](#build-from-source)

## Usar la herramienta de instalación de Solana

### MacOS & Linux

- Abra su aplicación de terminal favorita

- Instale la versión Solana [LATEST_SOLANA_RELEASE_VERSION](https://github.com/solana-labs/solana/releases/tag/LATEST_SOLANA_RELEASE_VERSION) en su máquina ejecutando:

```bash
sh -c "$(curl -sSfL https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/install)"
```

- Puede reemplazar `LATEST_SOLANA_RELEASE_VERSION` con la etiqueta de lanzamiento que coincida con la versión de software de su versión deseada, o utilice uno de los tres nombres simbólicos de canales: `estable`, `beta`, o `borde`.

- La siguiente salida indica una actualización exitosa:

```text
descargando el instalador LATEST_SOLANA_RELEASE_VERSION
Configuración: /home/solana/.config/solana/install/config.yml
Active release directory: /home/solana/.local/share/solana/install/active_release
* Release version: LATEST_SOLANA_RELEASE_VERSION
* Release URL: https://github.com/solana-labs/solana/releases/download/LATEST_SOLANA_RELEASE_VERSION/solana-release-x86_64-unknown-linux-gnu.tar.bz2
Actualización correcta
```

- Dependiendo de su sistema, el final de la mensajería del instalador puede pedirle que

```bash
Actualice su variable de entorno PATH para incluir los programas solana:
```

- Si obtienes el mensaje anterior, copia y pega el comando recomendado a continuación para actualizar `PATH`
- Confirma que tienes la versión deseada de `solana` instalada ejecutando:

```bash
solana --version
```

- Después de una instalación exitosa, `la actualización de solana-install` puede utilizarse para fácilmente actualizar el software de Solana a una versión más reciente en cualquier momento.

---

### Windows

- Abrir un formulario de comando (`cmd.exe`) como administrador

  - Buscar Comandos en la barra de búsqueda de Windows. Cuando aparezca el comando, haga clic con el botón derecho del ratón y seleccione “Abrir como administrador”. Si aparece una ventana emergente que pregunta "¿Quieres permitir que esta aplicación haga cambios en tu dispositivo?", haz clic en Sí.

- Copia y pega el siguiente comando, luego presiona Enter para descargar el instalador de Solana en un directorio temporal:

```bash
curl https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/solana-install-init-x86_64-pc-windows-msvc.exe --output C:\solana-install-tmp\solana-install-init.exe --create-dirs
```

- Copia y pega el siguiente comando, luego presiona Enter para instalar la última versión de Solana. Si ve una ventana emergente de seguridad por su sistema, seleccione permitir que el programa se ejecute.

```bash
C:\solana-install-tmp\solana-install-init.exe LATEST_SOLANA_RELEASE_VERSION
```

- Cuando haya terminado el instalador, presione Enter.

- Cierra la ventana del símbolo de espera de comandos y vuelve a abrir una nueva ventana del indicador de comandos como un usuario normal
  - Busca "Comando Prompt" en la barra de búsqueda, luego haz clic con el botón izquierdo en el icono de la aplicación de Comando, sin necesidad de ejecutar como Administrador)
- Confirma que tienes la versión deseada de `solana` instalada introduciendo:

```bash
solana --version
```

- Después de una instalación exitosa, `la actualización de solana-install` puede utilizarse para fácilmente actualizar el software de Solana a una versión más reciente en cualquier momento.

## Descargar Binarios Preconstruidos

Si prefiere no utilizar `solana-install` para gestionar la instalación, puede descargar e instalar manualmente los binarios.

### Linux

Descarga los binarios navegando a [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), descarga **solana-release-x86_64-unknown-linux-msvc.tar.bz2**, luego extrae el archivo:

```bash
tar jxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### MacOS

Descarga los binarios navegando a [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), descarga **solana-release-x86_64-apple-darwin.tar.bz2**, luego extrae el archivo:

```bash
tar jxf solana-release-x86_64-apple-darwin.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### Windows

- Descarga los binarios navegando a [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), descarga **solana-release-x86_64-pc-windows-msvc. ar.bz2**, luego extraiga el archivomusando WinZip o similar.

- Abra un comando Prompt y vaya al directorio en el que extrajiste los binarios y ejecuta:

```bash
cd solana-release/
set PATH=%cd%/bin;%PATH%
```

## Construir desde la fuente

Si no puedes usar los binarios preconstruidos o prefieres construirlo tú mismo desde el código fuente, navega a [https://github. om/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), y descarga el archivo **Código fuente**. Extrae el código y construye los binarios con:

```bash
./scripts/cargo-install-all.sh .
export PATH=$PWD/bin:$PATH
```

Luego puede ejecutar el siguiente comando para obtener el mismo resultado que con binarios preconstruidos:

```bash
init solana-install
```
