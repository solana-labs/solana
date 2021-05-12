---
title: Cartera de papel
---

Este documento describe cómo crear y utilizar una cartera de papel con las herramientas de Solana CLI.

> No pretendemos aconsejar cómo _crear o administrar de forma segura_ monederos de papel. Le ruego que investigue atentamente las cuestiones relativas a seguridad.

## Vista general

Solana proporciona una herramienta de generación de claves para obtener claves de frases compatibles con BIP39. Comandos de Solana CLI para ejecutar un validador y staking de tokens todos soportan entrada de keypair a través de frases de semilla.

Para obtener más información sobre el estándar BIP39, visite el repositorio de Gitcoin BI [aquí](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki).

## Uso de Cartera Papel

Los comandos Solana se pueden ejecutar sin guardar nunca un keypair en el disco de una máquina. Si evitar la escritura de una clave privada en el disco es una preocupación de seguridad para usted, ha llegado al lugar correcto.

> Incluso utilizando este método de entrada segura, sigue siendo posible que una clave privada se escriba en el disco mediante intercambios de memoria no cifrados. Es responsabilidad del usuario protegerse contra este escenario.

## Antes de comenzar

- [Instalar la suite de herramientas de línea de comandos Solana](../cli/install-solana-cli-tools.md)

### Revisa tu instalación

Comprueba que `solana-keygen` esté instalado correctamente ejecutando:

```bash
solana-keygen --version
```

## Creando una cartera en papel

Utilizando la herramienta `solana-keygen`, es posible generar nuevas frases semilla así como derivar un keypair a partir de una frase semilla existente y una frase de paso (opcional). La frase de semilla y la frase de contraseña pueden utilizarse juntos como una cartera de papel. Mientras mantenga su frase de semilla y contraseña almacenada de forma segura, puede utilizarlos para acceder a su cuenta.

> Para más información sobre cómo funcionan las frases de semillas, revisa esta [página de la wiki de Bitcoin](https://en.bitcoin.it/wiki/Seed_phrase).

### Generación de Frases de Semilla

Generar un nuevo keypair puede hacerse usando el comando `solana-keygen new`. El comando generará una frase de semilla aleatoria, te pedirá que introduzcas una contraseña opcional, y luego mostrará la clave pública derivada y la frase semilla generada para su cartera de papel.

Después de copiar tu frase de semilla, puede utilizar las instrucciones [derivación de clave pública](#public-key-derivation) para verificar que no ha cometido ningún error.

```bash
solana-keygen nuevo --no-outfile
```

> Si la bandera `--no-outfile` está **omitida**, el comportamiento por defecto es escribir el keypair en `~/.config/solana/id.json`, dando como resultado una [cartera de sistema de archivos](file-system-wallet.md)

La salida de este comando mostrará una línea como esta:

```bash
pubkey: 9ZNTfG4NyQgxy2SWjSiQoUyBPEvXT2xo7fKc5hPYYJ7b
```

El valor que aparece después de `pubkey:` es tu _dirección de la cartera_.

**Nota:** Al trabajar con carteras de papel y carteras de sistemas de archivos, los términos "pubkey" y "dirección del monedero" se utilizan a veces de forma intercambiable.

> Para mayor seguridad, incrementa el conteo de palabras de la frase semilla usando el argumento `--word-count`

Para detalles de uso completo ejecutar:

```bash
solana-keygen nuevo --help
```

### Derivación de la clave pública

Las claves públicas pueden derivarse de una frase semilla y una frase de contraseña si se decide utilizar una. Esto es útil para usar una frase semilla generada fuera de línea para derivar una clave pública válida. El comando `solana-keygen pubkey` te guiará para que introduzcas tu frase inicial y una frase de contraseña si decides utilizarla.

```bash
pubkey de solana-keygen
```

> Tenga en cuenta que podría utilizar diferentes frases de acceso para la misma frase de inicio. Cada contraseña única producirá un keypair diferente.

La herramienta `solana-keygen` utiliza la misma lista de palabras estándar en inglés BIP39 que para generar frases de semilla. Si su frase semilla fue generada con otra herramienta que utiliza una lista de palabras diferente, puede seguir utilizando `solana-keygen`, pero tendrá que pasar el argumento `--skip-seed-phrase-validation` y renunciar a esta validación.

```bash
pubkey de solana-keygen --skip-seed-phrase-validation
```

After entering your seed phrase with `solana-keygen pubkey ASK` the console will display a string of base-58 character. Esta es la _dirección de la billetera_ asociada a su frase semilla.

> Copiar la dirección derivada en una memoria USB para facilitar su uso en ordenadores en red

> Un paso siguiente habitual es [comprobar el saldo](#comprobar-saldo-de-cuenta) de la cuenta asociada a una clave pública

Para detalles de uso completo ejecutar:

```bash
solana-keygen nuevo --help
```

## Verificando el keypair

Para verificar que controlas la clave privada de una dirección de monedero de papel, utiliza `solana-keygen verify`:

```bash
verificación solana-keygen <PUBKEY> ASK
```

donde `<PUBKEY>` se reemplaza con la dirección del monedero y la palabra clave `ASK` le dice al comando que le pida la frase semilla del keypair. Tenga en cuenta que por razones de seguridad, su frase de semilla no se mostrará a medida que escribe. Después de introducir su frase de semilla, el comando mostrará "Éxito" si la clave pública dada coincide con el par de claves generado a partir de su frase de semilla, y "Failed" de otra manera.

## Comprobando el saldo de una cuenta

Todo lo que se necesita para comprobar el saldo de una cuenta es la clave pública de una cuenta. Para recuperar claves públicas de forma segura desde una cartera de papel, sigue las instrucciones de [Derivación de la clave pública](#public-key-derivation) en un [ordenador emparejado](<https://en.wikipedia.org/wiki/Air_gap_(networking)>). Las claves públicas pueden escribirse manualmente o transferirse a través de un dispositivo USB a una máquina conectada en red.

A continuación, configure la herramienta `solana` CLI para [conectarse a un clúster en particular](../cli/choose-a-cluster.md):

```bash
conjunto de configuración de solana --url <CLUSTER URL> # (es decir, https://api.mainnet-beta.solana.com)
```

Finalmente, para comprobar el saldo, ejecute el siguiente comando:

```bash
saldo de solana <PUBKEY>
```

## Creando varias direcciones de cartera de papel

Puede crear tantas direcciones de cartera como desee. Simplemente vuelva a ejecutar los pasos en [Generación de frases de semilla](#seed-phrase-generation) o [Derivación de clave pública](#public-key-derivation) para crear una nueva dirección. Múltiples direcciones del monedero pueden ser útiles si desea transferir tokens entre sus propias cuentas para diferentes propósitos.

## Soporte

Echa un vistazo a nuestra [Página de soporte al monedero](support.md) para encontrar formas de obtener ayuda.
