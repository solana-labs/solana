---
title: Publicando información del validador
---

Puedes publicar tu información de validador en la cadena para que sea visible públicamente para otros usuarios.

## Ejecutar información del validador de solana

Ejecutar el CLI de solana para rellenar una cuenta de información de validador:

```bash
solana validator-info publique --keypair ~/validator-keypair.json <VALIDATOR_INFO_ARGS> <VALIDATOR_NAME>
```

Para detalles sobre campos opcionales para VALIDATOR_INFO_ARGS:

```bash
solana validator- info publish --help
```

## Ejemplos de comandos

Ejemplo de comando de publicación:

```bash
solana validator-info publicar "Elvis Validator" -n elvis -w "https://elvis-validates.com"
```

Ejemplo de comando de consulta:

```bash
solana validator-info get
```

que produce

```text
Validator info from 8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFizayqziDc9ah
  Validator pubkey: 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTTyj4xmyDMWo
  Info: {"keybaseUsername":"elvis","name":"Elvis Validator","website":"https://elvis-validates.com"}
```

## Keybase

Incluyendo un nombre de usuario Keybase permite a las aplicaciones cliente \(como Solana Network Explorer\) extraer automáticamente el perfil público del validador, incluyendo pruebas criptográficas, identidad de marca, etc. Para conectar su validador pubkey con Keybase:

1. Únete a [https://keybase.io/](https://keybase.io/) y completa el perfil de tu validador
2. Añade tu validador **identidad pubkey** a Keybase:

   - Crea un archivo vacío en tu ordenador local llamado `validator-<PUBKEY>`
   - En la base de teclas, navega a la sección Archivos y sube tu archivo pubkey a

     `solana` subdirectory in your public folder: `/keybase/public/<KEYBASE_USERNAME>/solana`

   - Para comprobar tu pubkey, asegúrate de que puedes navegar con éxito

     `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<PUBKEY>`

3. Añade o actualiza tu información de validador de `solana` con tu nombre de usuario de Keybase. El

   CLI verificará el archivo `validator-<PUBKEY>`
