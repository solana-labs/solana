---
title: Añadir Solana a tu Exchange
---

Esta guía describe cómo agregar el token nativo SOL de Solana a su intercambio de criptomonedas.

## Configuración de Nodo

Recomendamos encarecidamente configurar al menos dos nodos en instancias de computadoras/nube de alto nivel, actualizando a versiones más recientes con rapidez, y vigilando las operaciones de servicio con una herramienta de monitoreo integrada.

Esta configuración le permite:
- para tener una puerta de enlace de confianza con el clúster Solana mainnet-beta para obtener datos y enviar transacciones de retiro
- para tener control total sobre la cantidad de datos históricos que se conservan
- para mantener la disponibilidad del servicio incluso si un nodo falla

Los nodos Solana requieren una potencia de computación relativamente alta para manejar nuestros bloques rápidos y TPS altos.  Para requisitos específicos, consulte [recomendaciones de hardware](../running-validator/validator-reqs.md).

Para ejecutar un nodo Api:

1. [Instalar la suite de herramientas de línea de comandos Solana](../cli/install-solana-cli-tools.md)
2. Iniciar el validador con al menos los siguientes parámetros:

```bash
solana-validator \
  --ledger <LEDGER_PATH> \
  --entrypoint <CLUSTER_ENTRYPOINT> \
  --expected-genesis-hash <EXPECTED_GENESIS_HASH> \
  --rpc-port 8899 \
  --no-voting \
  --enable-rpc-transaction-history \
  --limit-ledger-size \
  --trusted-validator <VALIDATOR_ADDRESS> \
  --no-untrusted-rpc
```

Personalice `--ledger` a la ubicación de almacenamiento deseada del edger y `--rpc-port` al puerto que desea exponer.

Los parámetros `--entrypoint` y `--expected-genesis-hash` son todos específicos del clúster al que se une. [Parámetros actuales para Mainnet Beta](../clusters.md#example-solana-validator-command-line-2)

El parámetro `--limit-ledger-size` le permite especificar cuántos ledger [shreds](../terminology.md#shred) su nodo retiene en el disco. Si no incluye este parámetro, el validador mantendrá el libro de valores entero hasta que se agote de espacio en disco.  El valor predeterminado intenta mantener el uso del disco ledger por debajo de 500GB.  Se puede solicitar más o menos uso del disco añadiendo un argumento a `--limit-ledger-size` si se desea. Compruebe `solana-validator --help` para conocer el valor límite por defecto utilizado por `--limit-ledger-size`.  Más información sobre seleccionar un valor límite personalizado está [disponible aquí](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

Especificar uno o más parámetros `--trusted-validator` puede protegerle de arrancar desde una captura maliciosa. [Más sobre el valor del arranque con validadores de confianza](../running-validator/validator-start.md#trusted-validators)

Parámetros opcionales a considerar:

- `--private-rpc` impide que su puerto RPC sea publicado para su uso por otros nodos
- `--rpc-bind-address` le permite especificar una dirección IP diferente para enlazar el puerto RPC

### Reinicios automáticos y monitorización

Recomendamos configurar cada uno de sus nodos para reiniciar automáticamente al salir, para asegurarse de que pierde la menor cantidad de datos posible. Ejecutar el software solana como un servicio de sistema es una gran opción.

Para monitorear, proporcionamos [`torre solana-vigilante`](https://github.com/solana-labs/solana/blob/master/watchtower/README.md), que puede monitorizar su validador y detectar con el `proceso de validación de solana` no es saludable. Se puede configurar directamente para alertarle a través de Slack, Telegram, Discord, o Twillio. Para más detalles, ejecuta `solana-watchtower --help`.

```bash
solana-watchtower --validator-identity <YOUR VALIDATOR IDENTITY>
```

#### Nuevos anuncios de lanzamiento de software

Lanzamos el nuevo software con frecuencia (alrededor de 1 versión / semana). A veces las versiones más recientes incluyen cambios incompatibles del protocolo, que necesita una actualización de software oportuna para evitar errores en el procesamiento de bloques.

Nuestros anuncios de lanzamiento oficiales para todo tipo de lanzamientos (normal y seguridad) se comunican a través de un canal de discord llamado [`#mb-announcement`](https://discord.com/channels/428295358100013066/669406841830244375) (`mb` significa `mainnet-beta`).

Como validadores con stake, esperamos que cualquier validador operado por el intercambio se actualice lo antes posible en un día hábil o dos después de un anuncio de lanzamiento normal. En el caso de lanzamientos relacionados con la seguridad, puede ser necesario tomar medidas más urgentes.

### Continuidad del ledger

Por defecto, cada uno de sus nodos arrancará desde una captura proporcionada por uno de sus validadores de confianza. Esta captura refleja el estado actual de la cadena, pero no contiene el contorno histórico completo. Si uno de los nodos sale y arranca desde una nueva captura, puede haber un hueco en el ledger en ese nodo. Para evitar este problema, añada el parámetro `--no-snapshot-fetch` a su comando `solana-validator` para recibir datos históricos del ledger en lugar de una captura.

No pase el parámetro `--no-snapshot-fetch` en su arranque inicial ya que no es posible arrancar el nodo desde el bloque genesis.  En su lugar arranque desde primero una captura y luego añadir el parámetro `--no-snapshot-fetch` para reiniciar.

Es importante tener en cuenta que la cantidad de contable histórico disponible para tus nodos del resto de la red está limitada en cualquier momento.  Una vez operacional si tus validadores experimentan un tiempo de inactividad significativo, pueden no ser capaces de ponerse al día de la red y necesitarán descargar una nueva captura de un validador de confianza.  Al hacerlo, tus validadores ahora tendrán un hueco en sus datos históricos de contabilidad que no se pueden rellenar.


### Minimizando exposición de puertos validadores

El validador requiere que varios puertos UDP y TCP estén abiertos para el tráfico entrante de todos los validadores de Solana.   Aunque este es el modo de funcionamiento más eficiente, y se recomienda encarecidamente, es posible restringir el validador para que sólo requiera tráfico entrante de otro validador Solana.

Primero añade el argumento `--restricted-repair-only-mode`.  Esto hará que el validador funcione en un modo restringido en el que no recibirá empujes del resto de validadores, y en su lugar tendrá que sondear continuamente a otros validadores en busca de bloques.  El validador solo transmitirá paquetes UDP a otros validadores usando los puertos *Gossip* y *ServeR* ("serve repair") y sólo recibir paquetes UDP en sus puertos *gossip* y *Reparación*.

El puerto de *Gossip* es bidireccional y permite a su validador permanecer en contacto con el resto del clúster.  Su validador transmite en el *ServeR* para hacer solicitudes de reparación para obtener nuevos bloques del resto de la red, ya que la Turbina está ahora deshabilitada.  Tu validador recibirá entonces reparaciones en el puerto *Reparar* de otros validadores.

Para restringir aún más el validador a solo solicitar bloques de uno o más validadores, primero determina el pubkey de identidad para ese validador y añade los argumentos `--gossip-pull-validator PUBKEY --repair-validator PUBKEY` para cada PUBKEY.  Esto hará que su validador sea un drenaje de recursos en cada validador que añada, así que por favor haga esto con moderación y sólo después de consultar con el validador de destino.

Tu validador sólo debería comunicarse ahora con los validadores listados explícitamente y sólo en el gossip **, *Reparar* y *ServeR* puertos.

## Configurar Cuentas de Depósitos

Las cuentas Solana no requieren ninguna inicialización en cadena; una vez que contienen algunos SOL, existen. Para configurar una cuenta de depósito para su intercambio, simplemente genere un par de claves Solana usando cualquiera de nuestras [herramientas de monedero](../wallet-guide/cli.md).

Recomendamos utilizar una cuenta de depósito única para cada uno de sus usuarios.

Las cuentas de Solana se cobran [alquiler](developing/programming-model/accounts.md#rent) al crearse y una vez por época, pero pueden quedar exentas de renta si contienen 2 años de renta en SOL. Para encontrar el saldo mínimo de rent-exempt para sus cuentas de depósito, consulte [`getMinimumBalanceForRentExemption` endpoint](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

### Cuentas fuera de línea

Es posible que desee mantener las claves de una o más cuentas de colección sin conexión para mayor seguridad. Si es así, necesitarás mover SOL a cuentas en caliente utilizando nuestros [métodos sin conexión](../offline-signing.md).

## Escuchando para los depósitos

Cuando un usuario desee depositar SOL en su intercambio, instruya que envíe una transferencia a la dirección de depósito apropiada.

### Encuesta para Bloques

Para rastrear todas las cuentas de depósito de tu intercambio, consulta por cada bloque confirmado e inspeccionar las direcciones de interés, usando el servicio JSON-RPC de su nodo Solana API.

- Para identificar qué bloques están disponibles, envía una solicitud de [`getConfirmedBlocks`](developing/clients/jsonrpc-api.md#getconfirmedblocks), pasando el último bloque que ya ha procesado como parámetro de ranura inicial:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5]}' localhost:8899

{"jsonrpc":"2.0","result":[5,6,8,9,11],"id":1}
```

No todas las ranuras producen un bloque, así que puede haber espacios vacíos en la secuencia de enteros.

- Para cada bloque, solicite su contenido con una solicitud [`getConfirmedBlock`](developing/clients/jsonrpc-api.md#getconfirmedblock):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[5, "json"]}' localhost:8899

{
  "jsonrpc": "2. ",
  "resultado": {
    "blockhash": "2WcrsKSVANoe6xQHKtCcqNdUpCQPQ3vb6QTgi1dcE2oL",
    "parentSlot": 4,
    "previousBlockhash": "7ZDoGW83nXgP14vnn9XhGSaGjbuLdLWkQAoUQ7pg6qDZ",
    "recompensas": [],
    "transacciones": [
      {
        "meta": {
          "err": null,
          "cuota": 5000,
          "postBalances": [
            2033973061360,
            218099990000,
            42000000003
          ],
          "preBalances": [
            2044973066360,
            207099990000,
            42000000003
          ],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "Bbqg1M4YVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
              "111111111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 1,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [
                  0,
                  1
                ],
                "data": "3Bxs3zyH82bhpB8j",
                "programIdIndex": 2
              }
            ],
            "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
          },
          "signatures": [
            "dhjhJp2V2ybQGVfELWM1aZy98guVVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
          ]
        }
      }
    ]
  }, 
 },
  "id": 1
}
```

Los campos `preBalances` y `postBalances` permiten seguir los cambios de saldo en cada cuenta sin tener que analizar toda la transacción. Ellos listan los balances iniciales y finales de cada cuenta en [lamports](../terminology.md#lamport), indexados a la lista `accountKeys`. Por ejemplo, si la dirección de depósito es `47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi`, esta transacción representa una transferencia de 218099990000 - 207099990000 = 11000000000 lamports = 11 SOL

Si necesita más información sobre el tipo de transacción u otros específicos, puede solicitar el bloque de RPC en formato binario, y lo interprete usando nuestro [Rust SDK](https://github.com/solana-labs/solana) o [Javascript SDK](https://github.com/solana-labs/solana-web3.js).

### Historial de direcciones

También puede consultar el historial de transacciones de una dirección específica. Este es generalmente *no* un método viable para rastrear todas sus direcciones de depósito en todas las ranuras, pero puede ser útil para examinar algunas cuentas por un período específico de tiempo.

- Enviar una solicitud [`getConfirmedSignaturesForAddress2`](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress2) al nodo api:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedSignaturesForAddress2","params":["6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC", {"limit": 3}]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
      "slot": 114
    },
    {
      "err": null,
      "memo": null,
      "signature": "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
      "slot": 112
    },
    {
      "err": null,
      "memo": null,
      "signature": "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6",
      "slot": 108
    }
  ],
  "id": 1
}
```

- Para cada firma devuelta, obtenga los detalles de la transacción enviando una solicitud de [`getConfirmedTransaction`](developing/clients/jsonrpc-api.md#getconfirmedtransaction):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedTransaction","params":["dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6", "json"]}' localhost:8899

// Result
{
  "jsonrpc": "2.0",
  "result": {
    "slot": 5,
    "transaction": {
      "message": {
        "accountKeys": [
          "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
          "47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi",
          "11111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 1,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [
              0,
              1
            ],
            "data": "3Bxs3zyH82bhpB8j",
            "programIdIndex": 2
          }
        ],
        "recentBlockhash": "7GytRgrWXncJWKhzovVoP9kjfLwoiuDb3cWjpXGnmxWh"
      },
      "signatures": [
        "dhjhJp2V2ybQGVfELWM1aZy98guVVsxRCB5KhNiXFjCBMK5KEyzV8smhkVvs3xwkAug31KnpzJpiNPtcD5bG1t6"
      ]
    },
    "meta": {
      "err": null,
      "fee": 5000,
      "postBalances": [
        2033973061360,
        218099990000,
        42000000003
      ],
      "preBalances": [
        2044973066360,
        207099990000,
        42000000003
      ],
      "status": {
        "Ok": null
      }
    }
  },
  "id": 1
}
```

## Enviando retiros

Para acomodar la solicitud de un usuario de retirar SOL, debes generar una transacción de transferencia de Solana y envíelo al nodo api para ser reenviado a tu clúster.

### Sincrónica

Enviar una transferencia sincrónica al clúster de Solana le permite asegurar fácilmente que una transferencia sea exitosa y finalizada por el clúster.

La herramienta de línea de comandos de Solana ofrece un comando simple, `transferencia solana`, para generar, enviar y confirmar transacciones de transferencia. Por defecto, este método esperará y rastreará el progreso en stderr hasta que la transacción haya sido finalizada por el clúster. Si la transacción falla, informará de cualquier error de transacción.

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --keypair <KEYPAIR> --url http://localhost:8899
```

La [Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js) ofrece un enfoque similar para el ecosistema JS. Utilice el `System Program` para construir una transacción de transferencia, y enviarla usando el método `sendAndConfirmTransaction`.

### Asincrónico

Para mayor flexibilidad, puede enviar transferencias de retiro de forma asincrónica. En estos casos, es su responsabilidad verificar que la transacción tuvo éxito y fue finalizada por el clúster.

**Nota:** Cada transacción contiene un [blockhash reciente](developing/programming-model/transactions.md#blockhash-format) para indicar su vida. Es **crítico** esperar hasta que este blockhash caduque antes de reintentar una transferencia de retiro que no parece haber sido confirmada o finalizada por el clúster. De lo contrario, corremos el riesgo de un doble gasto. Ver más en [blockhash expiración](#blockhash-expiration) a continuación.

Primero, obtén un blockhash reciente usando el endpoint [`getFees`](developing/clients/jsonrpc-api.md#getfees) o el comando CLI:

```bash
solana fees --url http://localhost:8899
```

En la herramienta de línea de comandos, pasa el argumento `--no-wait` para enviar una transferencia de forma asíncrona, e incluya su blockhash reciente con el argumento `--blockhash`:

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

También puedes construir, firmar y serializar la transacción manualmente, y dispararla a el clúster utilizando el punto final JSON-RPC [`sendTransaction`endpoint](developing/clients/jsonrpc-api.md#sendtransaction).

#### Confirmaciones de transacción & Finalidad

Obtener el estado de un lote de transacciones usando el extremo [`getSignatureStatuses` JSON-RPC](developing/clients/jsonrpc-api.md#getsignaturestatuses). Los campos de `confirmaciones` reportan cuántos [bloques confirmados](../terminology.md#confirmed-block) han transcurrido desde que la transacción fue procesada. Si `confirmaciones: null`, está [finalizado](../terminology.md#finality).

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0", "id":1, "method":"getSignatureStatuses", "params":[["5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW", "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"]]}' http://localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 72,
        "confirmations": 10,
        "err": null,
        "status": {
          "Ok": null
        }
      },
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        }
      }
    ]
  },
  "id": 1
}
```

#### Caducidad de Blockhash

Cuando solicite un blockhash reciente para su transacción de retiro usando el endpoint [`getFees`](developing/clients/jsonrpc-api.md#getfees) o `solana fees`, la respuesta incluirá el último espacio `ValidSlot`, el último espacio en el que el blockhash será válido. Puede comprobar el contenedor de clúster con una consulta [`getSlot`](developing/clients/jsonrpc-api.md#getslot); una vez que la ranura de clúster sea mayor que `último Slot`, la transacción de retiro usando ese blockhash nunca debería tener éxito.

También puede verificar si un blockhash en particular sigue siendo válido enviando un [`getFeeCalculatorForBlockhash`](developing/clients/jsonrpc-api.md#getfeecalculatorforblockhash) solicitud con el blockhash como parámetro. Si el valor de la respuesta es nulo, el blockhash caduca y la transacción de retiro nunca debería tener éxito.

### Validar las direcciones de la cuenta suministradas por el usuario para los Retiros

Como los retiros son irreversibles, puede ser una buena práctica validar una dirección de cuenta proporcionada por el usuario antes de autorizar un retiro para prevenir la pérdida accidental de fondos del usuario.

La dirección de una cuenta normal en Solana es una cadena codificada en Base58 de una clave pública de ed25519 de 256 bits. No todos los patrones de bits son claves públicas válidas para la curva ed25519, por lo que es posible asegurar que las direcciones de la cuenta proporcionadas por el usuario sean al menos correctas ed25519 claves públicas.

#### Java

Aquí está un ejemplo Java de validar una dirección proporcionada por el usuario como una clave pública válida ed25519:

La siguiente muestra de código asume que está utilizando el Maven.

`pom.xml`:

```xml
<repositories>
  ...
  <repository>
    <id>primavera</id>
    <url>https://repo.spring.io/libs-release/</url>
  </repository>
</repositories>

...

<dependencies>
  ...
  <dependency>
      <groupId>io.github.novacrypto</groupId>
      <artifactId>Base58</artifactId>
      <version>0.1.3</version>
  </dependency>
  <dependency>
      <groupId>cafe.cryptography</groupId>
      <artifactId>curve25519-elisabeth</artifactId>
      <version>0.1.0</version>
  </dependency>
<dependencies>
```

```java
import io.github.novacrypto.base58.Base58;
import cafe.cryptography.curve25519.CompressedEdwardsY;

public class PubkeyValidator
{
    public static boolean verifyPubkey(String userProvidedPubkey)
    {
        try {
            return _verifyPubkeyInternal(userProvidedPubkey);
        } catch (Exception e) {
            return false;
        }
    }

    public static boolean _verifyPubkeyInternal(String maybePubkey) throws Exception
    {
        byte[] bytes = Base58.base58Decode(maybePubkey);
        return !(new CompressedEdwardsY(bytes)).decompress().isSmallOrder();
    }
}
```

## Soportando el estándar SPL Token

[SPL Token](https://spl.solana.com/token) es el estándar para la creación de tokens envuelto/sintético e intercambio en la cadena de bloques Solana.

El flujo de trabajo SPL Token es similar al de los token SOL nativos, pero hay unas diferencias que se discutirán en esta sección.

### Token Mints

Cada *tipo* de token SPL se declara creando una cuenta de *menta*.  Esta cuenta almacena metadatos que describen características como la suministración, el número de decimales y varias autoridades con control sobre la mint.  Cada cuenta SPL Token hace referencia a su menta asociada y solo puede interactuar con las fichas SPL de ese tipo.

### Instalando la herramienta CLI `spl-token`

Las cuentas de token SPL son consultadas y modificadas usando la utilidad `spl-token` línea de comandos. Los ejemplos que se ofrecen en esta sección dependen de que esté instalado en el sistema local.

`spl-token` se distribuye desde Rust [crates.io](https://crates.io/crates/spl-token) a través de la utilidad de línea de comandos Rust `cargo`. La última versión de `cargo` puede ser instalar utilizando un práctico one-liner para su plataforma en [rustup.rs](https://rustup.rs). Una vez que `carga` está instalada, `spl-token` se puede obtener con el siguiente comando:

```
carga instalar spl-token-cli
```

A continuación, puede comprobar la versión instalada para verificar

```
spl-token --version
```

Lo que debería resultar en algo como

```text
spl-token-cli 2.0.1
```

### Creación de cuenta

Las cuentas SPL Token llevan requisitos adicionales que las cuentas nativas del Programa de Sistema no:

1. Las cuentas SPL Token deben ser creadas antes de que se pueda depositar una cantidad de tokens.   Las cuentas de token se pueden crear explícitamente con el comando `spl-token create-account` , o implícitamente por la `transferencia spl-token --fund-recipient . .` comando.
1. Las cuentas de token SPL deben permanecer [exentas de alquiler](developing/programming-model/accounts.md#rent-exemption) durante su existencia y, por lo tanto, requieren que se deposite una pequeña cantidad de tokens SOL nativos sean depositados en la creación de la cuenta. Para cuentas SPL Token v2, esta cantidad es 0.00203928 SOL (2,039,280 lamports).

#### Línea de comando
Para crear una cuenta SPL Token con las siguientes propiedades:
1. Asociado con la mint dada
1. Propiedad del keypair de la cuenta de fondos

```
crear-token spl-token <TOKEN_MINT_ADDRESS>
```

#### Ejemplo
```
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir
Creación de la cuenta 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Firma: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVZZPiu5XxyJwS73Vi5WsZL88D7
```

O para crear una cuenta SPL Token con un keypair específico:
```
$ solana-keygen new -o token-account.json
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir token-account.json
Creación de la cuenta 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Firma: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVZZPiu5XxyJwS73Vi5WsZL88D7
```

### Comprobando el saldo de una cuenta

#### Línea de comando
```
saldo de spl-token <TOKEN_ACCOUNT_ADDRESS>
```

#### Ejemplo
```
$ solana balance 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV0
```

### Transferencias de Token

La cuenta de origen de una transferencia es la cuenta real del token que contiene la cantidad.

Sin embargo, la dirección del destinatario puede ser una cuenta normal de cartera.  Si todavía no existe una cuenta de tokens asociada para el monedero dado, la transferencia la creará siempre que el argumento `--fund-recipient` como se ha previsto.

#### Línea de comando
```
transferencia spl-token <SENDER_ACCOUNT_ADDRESS> <AMOUNT> <RECIPIENT_WALLET_ADDRESS> --fund-recipient
```

#### Ejemplo
```
Transferencia de spl-token 6B199xxzw3PkAm25hGJpj3Wj3WNYNHzDAnt1tEqg5BN 1 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Transferencia de 1 tokens
  Remitente: 6B199xxzw3PkAm25hGJpj3Wj3WNYNHzDAnt1tEqg5BN
  Destinatario: 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Firma: 3R6tsog17QM8KfzbcbdP4aoMfwgo6hBggJDVy7dZPVmH2xbCWjEj31JKD53NzMrf25ChFjY7Uv2dfCDq4mGFFyAj
```

### Depositando
Dado que cada par de `(usuario, mint)` requiere una cuenta separada en cadena, es recomendable que un intercambio cree lotes de cuentas de token de antemano y asigne a los usuarios si lo solicitan. Todas estas cuentas deben ser propiedad de keypairs controlados por el intercambio.

El seguimiento de las transacciones de depósito debe seguir el método [de votación de bloques](#poll-for-blocks) descrito arriba. Cada nuevo bloque debe escanearse para transacciones exitosas emisoras de SPL Token [Transferencia](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L92) o [Transferencia](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L252) instrucciones referenciando cuentas de usuario, luego consultar el saldo de la cuenta de tokens [](developing/clients/jsonrpc-api.md#gettokenaccountbalance) actualizaciones.

[Consideraciones](https://github.com/solana-labs/solana/issues/12318) se hacen para exender los campos `preBalance` y `metadatos de estado de la transacción` postBalance para incluir transferencias de saldo SPL Token.

### Retirando
La dirección de retiro de fondos que proporcione el usuario debe ser la misma que se utiliza para la retirada de fondos regular de SOL.

Antes de ejecutar una transferencia de retiro [](#token-transfers), el intercambio debería verificar la dirección [descrita anteriormente](#validating-user-supplied-account-addresses-for-withdrawals).

Desde la dirección de retiro, la cuenta de token asociada para la moneda correcta determinada y la transferencia emitida a esa cuenta.  Tenga en cuenta que es posible que la cuenta de token asociada aún no existe, en cuyo punto el intercambio debe financiar la cuenta en nombre del usuario.  Para cuentas SPL Token v2, el dinero de la cuenta de retiro requerirá 0.00203928 SOL (2.039,280 lamports).

Plantilla `comando de transferencia de spl-token` para un retiro:
```
$ transferencia spl-token --fund-recipient <exchange token account> <withdrawal amount> <withdrawal address>
```

### Otras consideraciones

#### Congelar Autoridad
Por razones de cumplimiento normativo, una entidad emisora de Fichas SPL puede optar por mantener la "Autoridad de Congelación" sobre todas las cuentas creadas en asociación con su ceca.  Esto les permite [congelar](https://spl.solana.com/token#freezing-accounts) los activos de una cuenta determinada a voluntad, haciendo que la cuenta no se pueda utilizar hasta que se descongele. Si esta función está en uso, el pubkey de la autoridad congelada se registrará en la cuenta del SPL Token.

## Probando la integración

Asegúrese de probar su flujo de trabajo completo en Solana devnet y testnet [clusters](../clusters.md) antes de pasar a producción en mainnet-beta. Devnet es el más abierto y flexible, e ideal para el desarrollo inicial, mientras que testnet ofrece una configuración de clúster más realista. Tanto devnet como testnet soportan una faucet, ejecute `solana airdrop 10` para obtener algunos SOL devnet o testnet para el desarrollo y las pruebas.
