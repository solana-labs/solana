---
title: Crear una clave pública de validador
---

Para poder participar necesitas registrarte primero. Vea [Información de registro](../registration/how-to-register.md).

Para obtener su asignación de SOL, necesita publicar la clave pública de identidad de su validador bajo su cuenta keybase.io.

## **Generar Keypair**

1. Si aún no lo ha hecho, genere el keypair de identidad de su validador ejecutando:

   ```bash
     keygen nuevo -o ~/validator-keypair.json
   ```

2. La clave pública de identidad ahora se puede ver ejecutando:

   ```bash
     pubkey de solana-keygen ~/validator-keypair.json
   ```

> Nota: El archivo "validator-keypair.json" también es su clave privada \(ed25519\).

El keypair de identificación del validador identifica a su validador dentro de la red. **Es crucial respaldar esta información.**

Si no hace una copia de seguridad de esta información, NO PODRÁ RECUPERAR SU VALIDADOR, si pierde el acceso a él. Si esto ocurre, USTED PERDERE SU ASIGNACIÓN DE SOL.

Para hacer una copia de seguridad de su keypair de identificación del validador, **haga una copia de seguridad de su archivo "validator-keypair.json" en una ubicación segura.**

## Vincula tu pubkey de Solana con una cuenta de Keybase

Debes vincular tu pubkey de Solana a una cuenta de Keybase.io. Las siguientes instrucciones describen cómo hacerlo instalando Keybase en su servidor.

1. Instala [Keybase](https://keybase.io/download) en tu equipo.
2. Inicie sesión en su cuenta de Keybase en su servidor. Crea primero una cuenta de Keybase si aún no la tienes. Aquí hay una [lista de comandos básicos de CLI de Keybase](https://keybase.io/docs/command_line/basics).
3. Crea un directorio de Solana en tu carpeta pública de archivos: `mkdir /keybase/public/<KEYBASE_USERNAME>/solana`
4. Publica la clave pública de identidad de tu validador creando un archivo vacío en la carpeta del archivo de tu base de datos pública en el siguiente formato: `/keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`. Por ejemplo:

   ```bash
     touch /keybase/public/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>
   ```

5. Para comprobar que tu clave pública ha sido publicada, asegúrate de que puedes navegar correctamente a `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<BASE58_PUBKEY>`
