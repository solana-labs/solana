---
title: API RPC JSON
---

Los nodos Solana aceptan peticiones HTTP utilizando la [especificación JSON-RPC 2.0](https://www.jsonrpc.org/specification).

Para interactuar con un nodo Solana dentro de una aplicación JavaScript, utiliza la [solana-web3. s](https://github.com/solana-labs/solana-web3.js) biblioteca, que proporciona una interfaz conveniente para los métodos RPC.

## Endpoint HTTP RPC

**Default port:** 8899 eg. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## RPC PubSub-WebSocket Endpoint

**Default port:** 8900 eg. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## Métodos

- [obtener información de cuenta](jsonrpc-api.md#getaccountinfo)
- [obtener Balance](jsonrpc-api.md#getbalance)
- [obtener el Compromiso de Bloque](jsonrpc-api.md#getblockcommitment)
- [obtener Tiempo de bloque](jsonrpc-api.md#getblocktime)
- [obtener ClusterNodes](jsonrpc-api.md#getclusternodes)
- [obtener bloque confirmado](jsonrpc-api.md#getconfirmedblock)
- [obtener bloques confirmados](jsonrpc-api.md#getconfirmedblocks)
- [obtener bloques confirmados con límite](jsonrpc-api.md#getconfirmedblockswithlimit)
- [obtener firmas confirmadas para la dirección](jsonrpc-api.md#getconfirmedsignaturesforaddress)
- [obtener firmas confirmadas para la dirección](jsonrpc-api.md#getconfirmedsignaturesforaddress2)
- [obtener Transacción confirmada](jsonrpc-api.md#getconfirmedtransaction)
- [obtener información de la época](jsonrpc-api.md#getepochinfo)
- [obtener calendario de la época](jsonrpc-api.md#getepochschedule)
- [obtener la calculadora de tarifas para Blockhash](jsonrpc-api.md#getfeecalculatorforblockhash)
- [obtener una tasa de gobernanza](jsonrpc-api.md#getfeerategovernor)
- [obtener tasas](jsonrpc-api.md#getfees)
- [obtener el primer bloque disponible](jsonrpc-api.md#getfirstavailableblock)
- [obtener Genesis Hash](jsonrpc-api.md#getgenesishash)
- [obtener Salud](jsonrpc-api.md#gethealth)
- [obtener Identidad](jsonrpc-api.md#getidentity)
- [obtener inflación de governanza](jsonrpc-api.md#getinflationgovernor)
- [obtener Tasa de inflación](jsonrpc-api.md#getinflationrate)
- [obtener las mayores cuentas](jsonrpc-api.md#getlargestaccounts)
- [obtener el horario del líder](jsonrpc-api.md#getleaderschedule)
- [obtener el saldo mínimo para la exención del alquiler](jsonrpc-api.md#getminimumbalanceforrentexemption)
- [obtener cuentas múltiples](jsonrpc-api.md#getmultipleaccounts)
- [obtener Programar cuentas](jsonrpc-api.md#getprogramaccounts)
- [obtener Blockhash reciente](jsonrpc-api.md#getrecentblockhash)
- [obtener muestras de rendimiento recientes](jsonrpc-api.md#getrecentperformancesamples)
- [obtener Estados de las firmas](jsonrpc-api.md#getsignaturestatuses)
- [obtener ranura](jsonrpc-api.md#getslot)
- [obtener ranura lider](jsonrpc-api.md#getslotleader)
- [obtener Activación de stake](jsonrpc-api.md#getstakeactivation)
- [obtener suministro](jsonrpc-api.md#getsupply)
- [obtener el número de transacciones](jsonrpc-api.md#gettransactioncount)
- [obtener Version](jsonrpc-api.md#getversion)
- [obtener Cuentas de voto](jsonrpc-api.md#getvoteaccounts)
- [espacio mínimo](jsonrpc-api.md#minimumledgerslot)
- [solicitar Airdrop](jsonrpc-api.md#requestairdrop)
- [enviar Transacción](jsonrpc-api.md#sendtransaction)
- [simular Transacción](jsonrpc-api.md#simulatetransaction)
- [establecer el filtro de registro](jsonrpc-api.md#setlogfilter)
- [salida de Validador](jsonrpc-api.md#validatorexit)
- [Suscripción Websocket](jsonrpc-api.md#subscription-websocket)
  - [cuenta Suscripción](jsonrpc-api.md#accountsubscribe)
  - [cancelar suscripción](jsonrpc-api.md#accountunsubscribe)
  - [registros Suscribirse](jsonrpc-api.md#logssubscribe)
  - [registros anular suscripción](jsonrpc-api.md#logsunsubscribe)
  - [programa suscribirse](jsonrpc-api.md#programsubscribe)
  - [programa Anular suscripción](jsonrpc-api.md#programunsubscribe)
  - [suscribirse a firma](jsonrpc-api.md#signaturesubscribe)
  - [dejar de suscribirse](jsonrpc-api.md#signatureunsubscribe)
  - [suscribirse Ranura](jsonrpc-api.md#slotsubscribe)
  - [ranura Anular suscripción](jsonrpc-api.md#slotunsubscribe)

## Métodos inestables

Los métodos inestables pueden ver cambios de ruptura en los lanzamientos de parches y pueden no ser soportados en la autoridad.

- [obtener el saldo de la cuenta deToken](jsonrpc-api.md#gettokenaccountbalance)
- [obtener cuentas de tokens por delegado](jsonrpc-api.md#gettokenaccountsbydelegate)
- [obtener cuentas de tokens por propietario](jsonrpc-api.md#gettokenaccountsbyowner)
- [obtener cuentas más grandes](jsonrpc-api.md#gettokenlargestaccounts)
- [obtener suministro](jsonrpc-api.md#gettokensupply)

## Formato de Solicitud

Para realizar una solicitud JSON-RPC, envíe una solicitud POST HTTP con un `Tipo de contenido: encabezado aplicación/json`. Los datos de la solicitud JSON deben contener 4 campos:

- `jsonrpc: <string>`, establecer a `"2.0"`
- `id: <number>`, un único entero de identificación generado por el cliente
- `método: <string>`, una cadena que contiene el método a invocar
- `params: <array>`, un array JSON de valores de parámetros ordenados

Ejemplo usando curl:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2. ",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"
    ]
  }
'
```

La salida de respuesta será un objeto JSON con los siguientes campos:

- `jsonrpc: <string>`, coincidiendo con la especificación de la solicitud
- `id: <number>`, coincidiendo con el identificador de solicitud
- `resultado: <array|number|object|string>`, datos solicitados o confirmación de éxito

Las peticiones pueden ser enviadas en lotes enviando una matriz de objetos JSON-RPC como los datos de un POST único.

## Definiciones

- Hash: Un hash SHA-256 de un fragmento de datos.
- Pubkey: La clave pública de un par de claves Ed25519.
- Transacción: Lista de instrucciones de Solana firmadas por un keypair del cliente para autorizar dichas acciones.
- Firma: Firma Ed25519 de datos de carga útil de la transacción incluyendo instrucciones. Esto se puede utilizar para identificar las transacciones.

## Configurando Comité de Estado

Para comprobaciones de prevuelo y procesamiento de transacciones, los nodos Solana eligen el estado del banco a consultar basado en un requisito de compromiso establecido por el cliente. El compromiso describe cómo está finalizado un bloque en ese momento. Cuando consulta el estado del ledger, se recomienda usar niveles inferiores de compromiso para reportar el progreso y niveles más altos para asegurar que el estado no será revertido.

En orden descendente de compromiso (más finalizado hasta menos finalizado), los clientes pueden especificar:

- `"max"` - el nodo preguntará al bloque más reciente confirmado por la supermayoría del clúster por haber alcanzado el máximo de bloque, significando que el clúster ha reconocido este bloque como finalizado
- `"root"` - el nodo consultará el bloque más reciente habiendo alcanzado el máximo de bloque en este nodo, significando que el nodo ha reconocido este bloque como finalizado
- `"singleGossip"` - el nodo preguntará sobre el bloque más reciente que ha sido votado por la supermayoría del clúster.
  - Incorpora votos de gossip y repeticiones.
  - No cuenta los votos sobre los descendientes de un bloque, sólo los votos directos sobre ese bloque.
  - Este nivel de confirmación también mantiene las garantías de "confirmación optimista" en lanzamiento 1.3 y posteriores.
- `"reciente"` - el nodo preguntará su bloque más reciente. Tenga en cuenta que el bloque puede no estar completo.

Para procesar muchas transacciones dependientes en serie, se recomienda utilizar `"singleGossip"` de compromiso, el cual equilibra la velocidad con la seguridad de cancelación. Para total seguridad, se recomienda usar`"max"` compromiso.

#### Ejemplo

El parámetro de compromiso debe incluirse como el último elemento de la matriz `params`:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri",
      {
        "commitment": "max"
      }
    ]
  }
'
```

#### Por defecto:

Si no se proporciona la configuración del compromiso, el nodo tendrá por defecto el compromiso `"max"`

Sólo los métodos que consultan el estado del banco aceptan el parámetro de compromiso. Están indicados en la referencia de la API de abajo.

#### Estructura RpcResponse

Muchos métodos que toman un parámetro de compromiso devuelven un objeto RpcResponse JSON compuesto por dos partes:

- `contexto` : Una estructura JSON RpcResponseContext incluyendo un campo `slot` en el que la operación fue evaluada.
- `valor` : El valor devuelto por la operación misma.

## Revisión de Salud

Aunque no es una API RPC JSON, un `GET /health` en el punto final HTTP RPC proporciona un mecanismo de comprobación de la salud para su uso por los equilibradores de carga u otra infraestructura de red. Esta solicitud siempre devolverá una respuesta HTTP 200 OK con un cuerpo de "ok" o "detrás" basado en las siguientes condiciones:

1. Si se proporcionan uno o más argumentos `--validador de confianza` a `validador de solana`, se devuelve "ok" cuando el nodo está dentro de `HEALTH_CHECK_SLOT_DISTANCE` ranuras del validador de mayor confianza, en caso contrario, se devuelve "detrás".
2. Siempre se devuelve "ok" si no se proporcionan validadores de confianza.

## Referencia de API JSON RPC

### obtener información de cuenta

Devuelve toda la información asociada con la cuenta del Pubkey proporcionado

#### Parámetros:

- `<string>` - Bloque de cuenta a consultar, como cadena codificada en base-58
- `<object>` - (opcional) objeto de configuración que contiene los siguientes campos opcionales:
  - (opcional) [compromiso](jsonrpc-api.md#configuring-state-commitment)
  - `codificación: <string>` - codificación para datos de cuenta, ya sea "base58" (_slow_), "base64", "base64+zstd", o "jsonParsed". "base58" se limita a los datos de la cuenta de menos de 128 bytes. "base64" devolverá datos codificados en base64 para datos de Cuenta de cualquier tamaño. "base64+zstd" comprime los datos de la cuenta usando [Zstandard](https://facebook.github.io/zstd/) y base64 codifica el resultado. La codificación "jsonParsed" intenta usar analizadores de estado específicos del programa para devolver datos de estado más legibles y explícitos de la cuenta. Si se solicita "jsonParsed" pero no se puede encontrar un analizador, el campo vuelve a la codificación "base64", detectable cuando el campo `data` es de tipo `<string>`.
  - (opcional) `datalice: <object>` - limitar los datos de la cuenta devuelta usando el `offset proporcionado: <usize>` y `longitud: <usize>` campos; sólo disponible para codificaciones "base58", "base64" o "base64+zstd".

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a:

- `<null>` - si la cuenta solicitada no existe
- `<object>` - de lo contrario, un objeto JSON que contiene:
  - `lamports: <u64>`, número de lamports asignados a esta cuenta, como u64
  - `propietario: <string>`, código base-58 del programa que esta cuenta ha sido asignada a
  - `datos: <[string, encoding]|object>`, datos asociados con la cuenta, ya sea como datos binarios codificados o formato JSON `{<program>: <state>}`, dependiendo del parámetro de codificación
  - `ejecutable: <bool>`, boolean indicando si la cuenta contiene un programa \(y es estrictamente de solo lectura\)
  - `rentEpoch: <u64>`, el epicentro en el que se alquilará esta cuenta, como u64

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
      {
        "encoding": "base58"
      }
    ]
  }
'
```

Respuesta:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": [
        "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
        "base58"
      ],
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getAccountInfo",
    "params": [
      "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA",
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

Respuesta:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "data": {
        "nonce": {
          "initialized": {
            "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
            "blockhash": "3xLP3jK6dVJwpeGeTDYTwdDK3TKchUf1gYYGHa4sF3XJ",
            "feeCalculator": {
              "lamportsPerSignature": 5000
            }
          }
        }
      },
      "executable": false,
      "lamports": 1000000000,
      "owner": "11111111111111111111111111111111",
      "rentEpoch": 2
    }
  },
  "id": 1
}
```

### obtener Balance

Devuelve el saldo de la cuenta del Pubkey proporcionado

#### Parámetros:

- `<string>` - Bloque de cuenta a consultar, como cadena codificada en base-58
- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `RpcResponse<u64>` - RpcResponse objeto JSON con `valor` campo establecido al balance

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": { "context": { "slot": 1 }, "value": 0 },
  "id": 1
}
```

### obtener el Compromiso de Bloque

Devuelve compromiso para un bloque particular

#### Parámetros:

- `<u64>` - bloque, identificado por Ranura

#### Resultados:

El campo resultado será un objeto JSON que contiene:

- `compromiso` - compromiso, que incluye ambas cosas:
  - `<null>` - Bloque desconocido
  - `<array>` - compromiso, matriz de enteros u64 que registran la cantidad de participación del clúster en lamportas que ha votado en el bloque en cada profundidad desde 0 hasta `MAX_LOCKOUT_HISTORY` + 1
- `totalStake` - total stake activo, en lamports, de la época actual

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "commitment": [
      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      0, 0, 0, 0, 0, 10, 32
    ],
    "totalStake": 42
  },
  "id": 1
}
```

### obtener Tiempo de bloque

Devuelve el tiempo estimado de producción de un bloque confirmado.

Cada validador reporta su tiempo de UTC al ledger en un intervalo regular agregando intermitentemente un timestamp a un Voto para un bloque en particular. El tiempo de un bloque solicitado se calcula a partir de la media ponderada por el stake de la marca de tiempo en un conjunto de bloques recientes registrados en el cuadro.

Los nodos que están iniciando desde instantánea o limitando el tamaño del contador (purgando ranuras antiguas) devolverán marcas de tiempo nulas para bloques por debajo de su raíz más baja + `TIMESTAMP_SLOT_RANGE`. Los usuarios interesados en tener estos datos históricos deben consultar un nodo que se construya a partir de génesis y retenga completamente el ledger.

#### Parámetros:

- `<u64>` - bloque, identificado por Ranura

#### Resultados:

- `<i64>` - tiempo de producción estimado, como sello de tiempo Unix (segundos desde la época Unix)
- `<null>` - la marca de tiempo no está disponible para este bloque

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockTime","params":[5]}
'
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 1574721591, "id": 1 }
```

### obtener ClusterNodes

Devuelve información sobre todos los nodos que participan en el clúster

#### Parámetros:

Ninguna

#### Resultados:

El campo resultante será una matriz de objetos JSON, cada uno con los siguientes subcampos:

- `pubkey: <string>` - Clave pública del nodo, como cadena codificada en base-58
- `gossip: <string>` - Dirección de red de gossip para el nodo
- `tpu: <string>` - Dirección de red TPU para el nodo
- `rpc: <string>|null` - Dirección de red RPC JSON para el nodo o `null` si el servicio RPC JSON no está activado
- `version: <string>|null` - La versión del software del nodo o `null` si la información de la versión no está disponible

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getClusterNodes"}
'
```

Resultado:

```json
{
  "jsonrpc": "2. ",
  "result": [
    {
      "gossip": "10.239.6. 8:8001",
      "pubkey": "9QzsJf7LPLj8GkXbYT3LFDKqsj2hHG7TA3xinJHu8epQ",
      "rpc": "10. 39.6.48:8899",
      "tpu": "10.239.6.48:8856",
      "version": "1. .0 c375ce1f"
    }
  ],
  "id": 1
}
```

### obtener bloque confirmado

Devuelve la identidad y la información de transacción sobre un bloque confirmado en el ledger

#### Parámetros:

- `<u64>` - ranura, como entero u64
- `<string>` - codificación para cada Transaction devuelta, ya sea "json", "jsonParsed", "base58" (_slow_), "base64". Si el parámetro no se proporciona, la codificación por defecto es "json". La codificación "jsonParsed" intenta usar los analizadores de instrucciones específicos del programa para devolver datos más legibles y explícitos en la lista de `transaction.message.instructions`. Si se solicita "jsonParsed" pero no se puede encontrar un analizador, la instrucción se vuelve a la codificación normal JSON (`cuentas`, `datos`y campos `programIdIndex`).

#### Resultados:

El campo resultado será un objeto con los siguientes campos:

- `<null>` - si el bloque especificado no está confirmado
- `<object>` - si el bloque está confirmado, un objeto con los siguientes campos:
  - `blockhash: <string>` - el blockhash de este bloque, como cadena codificada en base 58
  - `previousBlockhash: <string>` - el blockhash del padre de este bloque, como cadena codificada en base-58; si el bloque padre no está disponible debido a la limpieza del ledger, este campo devolverá "11111111111111111111111111111111111111111111111111"
  - `parentSlot: <u64>` - el índice de la ranura del padre de este bloque
  - `transacciones: <array>` - una matriz de objetos JSON que contienen:
    - `transacción: <object|[string,encoding]>` - [ transacción](#transaction-structure), ya sea en formato JSON o datos binarios codificados, dependiendo del parámetro de codificación
    - `meta: <object>` - objeto metadata del estado de la transacción, conteniendo `null` o:
      - `err: <object | null>` - Error si la transacción falló, null si la transacción tuvo éxito. [Definiciones de Error de Transacción](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `comisión: <u64>` - comisión de esta transacción fue cargada, como u64 entero
      - `preBalances: <array>` - matriz de saldos de cuentas u64 de antes de procesar la transacción
      - `postBalances: <array>` - matriz de balances de cuenta u64 después de procesar la transacción
      - `innerInstructions: <array|undefined>` - Lista de [instrucciones internas](#inner-instructions-structure) u omitida si la grabación de instrucciones internas aún no estaba habilitada durante esta transacción
      - `logMessages: <array>` - matriz de mensajes de registro de cadenas o omitidos si la grabación de mensajes de registro aún no estaba habilitada durante esta transacción
      - DEPRECIADO: `status: <object>` - Estado de la transacción
        - `"Ok": <null>` - La transacción fue exitosa
        - `"Err": <ERR>` - Transacción fallida con TransactionError
  - `recompensa: <array>` - una matriz de objetos JSON que contienen:
    - `pubkey: <string>` - La clave pública, como cadena codificada base-58 de la cuenta que recibió la recompensa
    - `lamports: <i64>`- número de lamports de recompensa acreditados o debitados por la cuenta, como un i64
    - `postBalance: <u64>` - balance de cuenta en lamports después de que la recompensa fue aplicada
    - `rewardType: <string|undefined>` - tipo de recompensa: "comisión", "renta", "votando", "staking"
  - `blockTime: <i64 | null>` - tiempo estimado de producción, como marca de tiempo Unix (segundos desde la época Unix). null si no está disponible

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "json"]}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "status": {
            "Ok": null
          }
        },
        "transaction": {
          "message": {
            "accountKeys": [
              "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
              "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
              "SysvarS1otHashes111111111111111111111111111",
              "SysvarC1ock11111111111111111111111111111111",
              "Vote111111111111111111111111111111111111111"
            ],
            "header": {
              "numReadonlySignedAccounts": 0,
              "numReadonlyUnsignedAccounts": 3,
              "numRequiredSignatures": 1
            },
            "instructions": [
              {
                "accounts": [1, 2, 3, 0],
                "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
                "programIdIndex": 4
              }
            ],
            "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
          },
          "signatures": [
            "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
          ]
        }
      }
    ]
  },
  "id": 1
}
```

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "base64"]}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "rewards": [],
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "status": {
            "Ok": null
          }
        },
        "transaction": [
          "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
          "base64"
        ]
      }
    ]
  },
  "id": 1
}
```

#### Estructura de la transacción

Las transacciones son muy diferentes a las de otras blockchains. Asegúrate de revisar [Anatomía de una transacción](developing/programming-model/transactions.md) para saber más sobre las transacciones en Solana.

La estructura JSON de una transacción se define de la siguiente manera:

- `firmas: <array[string]>` - Una lista de firmas codificadas en base 58 aplicadas a la transacción. La lista es siempre de longitud `message.header.numRequiredSignatures` y no vacía. La firma en el índice `i` corresponde a la clave pública en el índice `i` en `message.account_keys`. El primero se utiliza como el [id de transacción](../../terminology.md#transaction-id).
- `mensaje: <object>` - Define el contenido de la transacción.
  - `accountKeys: <array[string]>` - Lista de claves públicas codificadas en base a 58 utilizadas por la transacción, incluyendo las instrucciones y las firmas. Las primeras claves públicas de `message.header.numRequiredSignatures` deben firmar la transacción.
  - `encabezado: <object>` - Detalles de los tipos de cuenta y las firmas requeridas por la transacción.
    - `Signaturas numéricas: <number>` - El número total de firmas requeridas para que la transacción sea válida. Las firmas deben coincidir con las primeras `firmas numRequiredSignatures` de `message.account_keys`.
    - `numReadonlySignedAccounts: <number>` - El último `numReadonlySignedAccounts` de las claves firmadas son cuentas de solo lectura. Los programas pueden procesar múltiples transacciones que cargan cuentas de sólo lectura dentro de una única entrada de PoH, pero no se permite el crédito o débito de lamports o modificar los datos de la cuenta. Las transacciones dirigidas a la misma cuenta de lectura-escritura son evaluadas secuencialmente.
    - `numReadonlyUnsignedAccounts: <number>` - Las últimas `cuentas numReadonlyUnsignedAccounts` de las claves no firmadas son cuentas de solo lectura.
  - `recentBlockhash: <string>` - Un hash codificado en base 58 de un bloque reciente en el libro de valores usado para prevenir la duplicación de transacciones y para dar vidas a las transacciones.
  - `instrucciones: <array[object]>` - Lista de instrucciones del programa que serán ejecutadas en secuencia y confirmadas en una transacción atómica si todo tiene éxito.
    - `programIdIndex: <number>` - Índice en la matriz `message.accountKeys` que indica la cuenta del programa que ejecuta esta instrucción.
    - `cuentas: <array[number]>` - Lista de índices ordenados en la matriz `message.accountKeys` indicando qué cuentas pasar al programa.
    - `datos: <string>` - La entrada de datos del programa codificada en una cadena base-58.

#### Estructura de instrucciones internas

El tiempo de ejecución de Solana registra las instrucciones entre programas que se invocan durante el procesamiento de transacciones y las hace disponibles para una mayor transparencia de lo que se ejecutó en cadena por instrucción de transacción. Las instrucciones invocadas se agrupan por las instrucciones de la transacción original y se enumeran en orden de procesamiento.

La estructura JSON de las instrucciones internas se define como una lista de objetos en la siguiente estructura:

- `index: number` - Índice de la instrucción de la transacción de la que se originaron las instruccion(es) internas
- `instrucciones: <array[object]>` - Lista ordenada de instrucciones del programa interno que fueron invocadas durante una sola instrucción de transacción.
  - `programIdIndex: <number>` - Índice en la matriz `message.accountKeys` que indica la cuenta del programa que ejecuta esta instrucción.
  - `cuentas: <array[number]>` - Lista de índices ordenados en la matriz `message.accountKeys` indicando qué cuentas pasar al programa.
  - `datos: <string>` - La entrada de datos del programa codificada en una cadena base-58.

### obtener bloques confirmados

Devuelve una lista de bloques confirmados entre dos ranuras

#### Parámetros:

- `<u64>` - start_slot, como entero u64
- `<u64>` - (opcional) end_slot, como entero u64

#### Resultados:

El campo de resultado será un arreglo de enteros u64 que enumeran bloques confirmados entre `start_slot` y `end_slot`, si se proporciona, o el último bloque confirmado, inclusive. El rango máximo permitido es de 500.000 ranuras.

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7, 8, 9, 10], "id": 1 }
```

### obtener bloques confirmados con límite

Devuelve una lista de bloques confirmados comenzando en la ranura dada

#### Parámetros:

- `<u64>` - start_slot, como entero u64
- `<u64>` - límite, como entero u64

#### Resultados:

El campo de resultado será un arreglo de u64 enteros que listan bloques confirmados a partir de `start_slot` para hasta `límite` bloques, inclusive.

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### obtener firmas para la dirección

**DEPRECIADO: Por favor, utilice en su lugar obtener Firmas Confirmadas para la Dirección2**

Devuelve una lista de todas las firmas confirmadas para las transacciones que implican una dirección, dentro de un rango de Slot especificado. Rango máximo permitido es de 10.000 ranuras

#### Parámetros:

- `<string>` - dirección de cuenta como cadena codificada en base 58
- `<u64>` - ranura inicial, inclusive
- `<u64>` - ranura final, inclusive

#### Resultados:

El campo resultado será una matriz de:

- `<string>` - firma de transacción como cadena codificada en base 58

Las firmas serán ordenadas en base a la ranura en la que fueron confirmadas, desde la ranura más baja hasta la más alta

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress",
    "params": [
      "6H94zdiaYfRfPfKjYLjyr2VFBg6JHXygy84r3qhc3NsC",
      0,
      100
    ]
  }
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": [
    "35YGay1Lwjwgxe9zaH6APSHbt9gYQUCtBWTNL3aVwVGn9xTFw2fgds7qK5AL29mP63A9j3rh8KpN1TgSR62XCaby",
    "4bJdGN8Tt2kLWZ3Fa1dpwPSEkXWWTSszPSf1rRVsCwNjxbbUdwTeiWtmi8soA26YmwnKD4aAxNp8ci1Gjpdv4gsr",
    "4LQ14a7BYY27578Uj8LPCaVhSdJGLn9DJqnUJHpy95FMqdKf9acAhUhecPQNjNUy6VoNFUbvwYkPociFSf87cWbG"
  ],
  "id": 1
}
```

### recibir firmas confirmadas para la dirección 2

Devuelve firmas confirmadas para las transacciones que implican una dirección hacia atrás a tiempo desde la firma proporcionada o el bloque confirmado más reciente

#### Parámetros:

- `<string>` - dirección de cuenta como cadena codificada en base 58
- `<object>` - objeto de configuración (opcional) que contiene los siguientes campos:
  - `límite: <number>` - (opcional) máxima firma de transacción para devolver (entre 1 y 1,000, por defecto: 1,000).
  - `antes: <string>` - (opcional) empezar a buscar hacia atrás desde esta firma de transacción. Si no se proporciona, la búsqueda comienza desde la parte superior del bloque máximo confirmado.
  - `hasta: <string>` - búsqueda (opcional) hasta la firma de esta transacción, si se encuentra antes de alcanzar el límite.

#### Resultados:

El campo de resultado será una matriz de información de firma de transacción, ordenada de la transacción más reciente a la más antigua:

- `<object>`
  - `firma: <string>` - firma de transacción como cadena codificada base-58
  - `ranura: <u64>` - El espacio que contiene el bloque con la transacción
  - `err: <object | null>` - Error si la transacción falló, null si la transacción tuvo éxito. [Definiciones de Error de Transacción](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - Memo asociado con la transacción, null si no hay memo presente

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedSignaturesForAddress2",
    "params": [
      "Vote111111111111111111111111111111111111111",
      {
        "limit": 1
      }
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114
    }
  ],
  "id": 1
}
```

### obtener Transacción confirmada

Devuelve detalles de transacción para una transacción confirmada

#### Parámetros:

- `<string>` - firma de la transacción como cadena codificada en base-58 La codificación N intenta utilizar analizadores de instrucciones específicos del programa para devolver datos más legibles y explícitos en la lista `transaction.message.instructions`. Si se solicita "jsonParsed" pero no se puede encontrar un analizador, la instrucción se vuelve a la codificación normal JSON (`cuentas`, `datos`y campos `programIdIndex`).
- `<string>` - (opcional) codificación para la Transacción devuelta, ya sea "json", "jsonParsed", "base58" (_slow_), o "base64". Si el parámetro no se proporciona, la codificación por defecto es JSON.

#### Resultados:

- `<null>` - si la transacción no se encuentra o no está confirmada
- `<object>` - si la transacción está confirmada, un objeto con los siguientes campos:
  - `ranura: <u64>` - La ranura en la que esta transacción fue procesada
  - `transacción: <object|[string,encoding]>` - [ transacción](#transaction-structure), ya sea en formato JSON o datos binarios codificados, dependiendo del parámetro de codificación
  - `meta: <object | null>` - objeto metadata del estado de la transacción:
    - `err: <object | null>` - Error si la transacción falló, null si la transacción tuvo éxito. [Definiciones de Error de Transacción](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `comisión: <u64>` - comisión de esta transacción fue cargada, como u64 entero
    - `preBalances: <array>` - matriz de saldos de cuentas u64 de antes de procesar la transacción
    - `postBalances: <array>` - matriz de balances de cuenta u64 después de procesar la transacción
    - `innerInstructions: <array|undefined>` - Lista de [instrucciones internas](#inner-instructions-structure) u omitida si la grabación de instrucciones internas aún no estaba habilitada durante esta transacción
    - `logMessages: <array>` - matriz de mensajes de registro de cadenas o omitidos si la grabación de mensajes de registro aún no estaba habilitada durante esta transacción
    - DEPRECIADO: `status: <object>` - Estado de la transacción
      - `"Ok": <null>` - La transacción fue exitosa
      - `"Err": <ERR>` - Transacción fallida con TransactionError

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "json"
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": {
      "message": {
        "accountKeys": [
          "3UVYmECPPMZSCqWKfENfuoTv51fTDTWicX9xmBD2euKe",
          "AjozzgE83A3x1sHNUR64hfH7zaEBWeMaFuAN9kQgujrc",
          "SysvarS1otHashes111111111111111111111111111",
          "SysvarC1ock11111111111111111111111111111111",
          "Vote111111111111111111111111111111111111111"
        ],
        "header": {
          "numReadonlySignedAccounts": 0,
          "numReadonlyUnsignedAccounts": 3,
          "numRequiredSignatures": 1
        },
        "instructions": [
          {
            "accounts": [1, 2, 3, 0],
            "data": "37u9WtQpcm6ULa3WRQHmj49EPs4if7o9f1jSRVZpm2dvihR9C8jY4NqEwXUbLwx15HBSNcP1",
            "programIdIndex": 4
          }
        ],
        "recentBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B"
      },
      "signatures": [
        "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv"
      ]
    }
  },
  "id": 1
}
```

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2. ",
    "id": 1,
    "method": "getConfirmedTransaction",
    "Parámetros": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqY.Uuv",
      "base64"
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "status": {
        "Ok": null
      }
    },
    "slot": 430,
    "transaction": [
      "AVj7dxHlQ9IrvdYVIjuiRFs1jLaDMHixgrv+qtHBwz51L4/ImLZhszwiyEJDIp7xeBSpm/TX5B7mYzxa+fPOMw0BAAMFJMJVqLw+hJYheizSoYlLm53KzgT82cDVmazarqQKG2GQsLgiqktA+a+FDR4/7xnDX7rsusMwryYVUdixfz1B1Qan1RcZLwqvxvJl4/t3zHragsUp0L47E24tAFUgAAAABqfVFxjHdMkoVmOYaR1etoteuKObS21cc1VbIQAAAAAHYUgdNXR0u3xNdiTr072z2DVec9EQQ/wNo1OAAAAAAAtxOUhPBp2WSjUNJEgfvy70BbxI00fZyEPvFHNfxrtEAQQEAQIDADUCAAAAAQAAAAAAAACtAQAAAAAAAAdUE18R96XTJCe+YfRfUp6WP+YKCy/72ucOL8AoBFSpAA==",
      "base64"
    ]
  },
  "id": 1
}
```

### obtener información de la época

Devuelve información sobre la época actual

#### Parámetros:

- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

El campo resultado será un objeto con los siguientes campos:

- `absoluteSlot: <u64>`, la ranura actual
- `altura del bloque: <u64>`, la altura del bloque actual
- `epoch: <u64>`, la época actual
- `slotIndex: <u64>`, la ranura actual relativa al inicio de la época actual
- `slotsInEpoch: <u64>`, el número de ranuras en esta época

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}
'
```

Resultado:

```json
"jsonrpc": "2.0",
  "result": {
    "absoluteSlot": 166598,
    "blockHeight": 166500,
    "epoch": 27,
    "slotIndex": 2790,
    "slotsInEpoch": 8192
  },
  "id": 1
}
```

### obtener calendario de la época

Devuelve información de programación de la época desde la configuración de génesis de este clúster

#### Parámetros:

Ninguna

#### Resultados:

El campo resultado será un objeto con los siguientes campos:

- `ranuras PerEpoch: <u64>`, el número máximo de ranuras en cada época
- `leaderScheduleSlotOffset: <u64>`, el número de ranuras antes del comienzo de una época para calcular un horario líder para esa época
- `warmup: <bool>`, si las épocas comienzan cortas y crecen
- `firstNormalEpoch: <u64>`, primer epoch, log2(slotsPerEpoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
- `firstNormalSlot: <u64>`, MINIMUM_SLOTS_PER_EPOCH \* (2.power (firstNormalEpoch) - 1)

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "firstNormalEpoch": 8,
    "firstNormalSlot": 8160,
    "leaderScheduleSlotOffset": 8192,
    "slotsPerEpoch": 8192,
    "warmup": true
  },
  "id": 1
}
```

### obtener la calculadora de tarifas para Blockhash

Devuelve la calculadora de comisión asociada con el blockhash de consulta, o `null` si el blockhash ha caducado

#### Parámetros:

- `<string>` - blockhash de consulta como una cadena codificada en Base58
- `<object>` - (optional) [compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a:

- `<null>` - si el blockhash de consulta ha expirado
- `<object>` - de lo contrario, un objeto JSON que contiene:
  - `feeCalculator: <object>`, `FeCalculator` objeto describiendo la tasa de clúster en el blockhash consultado

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getFeeCalculatorForBlockhash",
    "params": [
      "GJxqhuxcgfn5Tcj6y3f8X4FeCDd2RQ6SnEMo1AAxrPRZ"
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 221
    },
    "value": {
      "feeCalculator": {
        "lamportsPerSignature": 5000
      }
    }
  },
  "id": 1
}
```

### obtener una tasa de gobernanza

Devuelve la información del gobernador de tasa de comisión desde el banco raíz

#### Parámetros:

Ninguna

#### Resultados:

El campo `resultado` será un `objeto` con los siguientes campos:

- `Porcentaje quemado: <u8>`, Porcentaje de comisiones recogidas para ser destruidas
- `maxLamportsPerSignature: <u64>`, el valor más grande `lamportsPerSignature` puede alcanzar para la siguiente ranura
- `minLamportsPerSignature: <u64>`, el valor más pequeño `lamportsPerSignature` puede alcanzar en la siguiente ranura
- `targetLamportsPerSignature: <u64>`, Tasa de comisión deseada para el clúster
- `targetSignaturesPerSlot: <u64>`, Tasa de firma deseada para el clúster

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFeeeRateGovernor"}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 54
    },
    "value": {
      "feeRateGovernor": {
        "burnPercent": 50,
        "maxLamportsPerSignature": 100000,
        "minLamportsPerSignature": 5000,
        "targetLamportsPerSignature": 10000,
        "targetSignaturesPerSlot": 20000
      }
    }
  },
  "id": 1
}
```

### obtener tasas

Devuelve un hash de bloque reciente del ledger, un programa de comisión que puede ser utilizado para calcular el costo de enviar una transacción usando ella, y el último espacio en que el blockhash será válido.

#### Parámetros:

- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` establecido a un objeto JSON con los siguientes campos:

- `blockhash: <string>` - un Hash como cadena codificada en base 58
- `feeCalculator: <object>` - Objeto FeeCalculator, el programa de comisión para este bloque hash
- `último espacio válido: <u64>` - último espacio en el cual un blockhash será válido

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      },
      "lastValidSlot": 297
    }
  },
  "id": 1
}
```

### obtener el primer bloque disponible

Devuelve la ranura del bloque confirmado más bajo que no ha sido purgado del ledger

#### Parámetros:

Ninguna

#### Resultados:

- `<u64>` - Ranura

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFirstResourableBlock"}
'
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 250000, "id": 1 }
```

### obtener Genesis Hash

Devuelve el hash genesis

#### Parámetros:

Ninguna

#### Resultados:

- `<string>` - un Hash como cadena codificada en base 58

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": "GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC",
  "id": 1
}
```

### obtener Salud

Devuelve la salud actual del nodo.

Si uno o más `--trusted-validator` argumentos son proporcionados a `solana-validator`, "ok" es devuelto cuando el nodo tiene dentro de `espacios de confianza HEALTH_CHECK_SLOT_DISTANCE` del validador de confianza más alto. de lo contrario devuelve un error. "ok" siempre se devuelve si no se proporcionan validadores de confianza.

#### Parámetros:

Ninguna

#### Resultados:

Si el nodo es saludable: "ok" Si el nodo no es saludable, se devuelve una respuesta de error JSON RPC. Las especificaciones de la respuesta de error son **DESTABLE** y pueden cambiar en el futuro

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getHealth"}
'
```

Resultado saludable:

```json
{ "jsonrpc": "2.0", "result": "ok", "id": 1 }
```

Resultado poco saludable (genérico):

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is unhealthy",
    "data": {}
  },
  "id": 1
}
```

Resultado poco saludable (si hay información adicional disponible)

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32005,
    "message": "Node is behind by 42 slots",
    "data": {
      "numSlotsBehind": 42
    }
  },
  "id": 1
}
```

### obtener Identidad

Devuelve la clave de identidad para el nodo actual

#### Parámetros:

Ninguna

#### Resultados:

El campo resultado será un objeto JSON con los siguientes campos:

- `identidad`, la pubkey de identidad del nodo actual \(como una cadena codificada en base-58\)

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getIdentity"}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": { "identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4WuW2DJBppN" },
  "id": 1
}
```

### obtener inflación de governanza

Devuelve el gobernador de inflación actual

#### Parámetros:

- `<object>` - (opcional) [compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

El campo resultado será un objeto JSON con los siguientes campos:

- `inicial: <f64>`, el porcentaje de inflación inicial del tiempo 0
- `terminal: <f64>`, porcentaje de inflación terminal
- `disminuye: <f64>`, tasa por año a la que se reduce la inflación
- `fundación: <f64>`, porcentaje de inflación total asignado a la fundación
- `Término de la fundación: <f64>`, duración de la inflación de la fundación en años

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationGovernor"}
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "foundation": 0.05,
    "foundationTerm": 7,
    "initial": 0.15,
    "taper": 0.15,
    "terminal": 0.015
  },
  "id": 1
}
```

### obtener Tasa de inflación

Devuelve los valores de inflación específicos para la época actual

#### Parámetros:

Ninguna

#### Resultados:

El campo resultado será un objeto JSON con los siguientes campos:

- `total: <f64>`, inflación total
- `validador: <f64>`, inflación asignada a validadores
- `fundación: <f64>`, inflación asignada a la fundación
- `epoca: <f64>`, época para la que son válidos estos valores

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationRate"}
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "epoch": 100,
    "foundation": 0.001,
    "total": 0.149,
    "validator": 0.148
  },
  "id": 1
}
```

### obtener las mayores cuentas

Devuelve las 20 cuentas más grandes, por balance de lamports

#### Parámetros:

- `<object>` - (opcional) objeto de configuración que contiene los siguientes campos opcionales:
  - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)
  - (opcional) `filtro: <string>` - filtrar resultados por tipo de cuenta; actualmente soportado: `circulando|no circulando`

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a una matriz de:

- `<object>` - de lo contrario, un objeto JSON que contiene:
  - `dirección: <string>`, dirección codificada en base 58 de la cuenta
  - `lamports: <u64>`, número de lamports en la cuenta, como u64

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLargestAccounts"}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 54
    },
    "value": [
      {
        "lamports": 999974,
        "address": "99P8ZgtJYe1buSK8JXkvpLh8xPsCFuLYhz9hQFNw93WJ"
      },
      {
        "lamports": 42,
        "address": "uPwWLo16MVehpyWqsLkK3Ka8nLowWvAHbBChqv2FZeL"
      },
      {
        "lamports": 42,
        "address": "aYJCgU7REfu3XF8b3QhkqgqQvLizx8zxuLBHA25PzDS"
      },
      {
        "lamports": 42,
        "address": "CTvHVtQ4gd4gUcw3bdVgZJJqApXE9nCbbbP4VTS5wE1D"
      },
      {
        "lamports": 20,
        "address": "4fq3xJ6kfrh9RkJQsmVd5gNMvJbuSHfErywvEjNQDPxu"
      },
      {
        "lamports": 4,
        "address": "AXJADheGVp9cruP8WYu46oNkRbeASngN5fPCMVGQqNHa"
      },
      {
        "lamports": 2,
        "address": "8NT8yS6LiwNprgW4yM1jPPow7CwRUotddBVkrkWgYp24"
      },
      {
        "lamports": 1,
        "address": "SysvarEpochSchedu1e111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Stake11111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarC1ock11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "StakeConfig11111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarRent111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Config1111111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarStakeHistory1111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarRecentB1ockHashes11111111111111111111"
      },
      {
        "lamports": 1,
        "address": "SysvarFees111111111111111111111111111111111"
      },
      {
        "lamports": 1,
        "address": "Vote111111111111111111111111111111111111111"
      }
    ]
  },
  "id": 1
}
```

### obtener el horario del líder

Devuelve el horario del líder para una época

#### Parámetros:

- `<u64>` - (opcional) Obtener el horario del líder para la época que corresponde a la ranura proporcionada. Si no se especifica, se obtiene el calendario de líder para la época actual
- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `<null>` - si no se encuentra la época solicitada
- `<object>` - de lo contrario, el campo resultante será un diccionario de claves públicas de líderes (como cadenas codificadas en base-58) y sus correspondientes índices de ranuras de líderes como valores (los índices son relativos a la primera ranura de la época solicitada)

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}
'
```

Resultado:

```json
{
  "jsonrpc":"2.0",
  "result":{
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F":[0,1,2,3,4,5,6,8,9,10,12,13,14,15,17,18,19,21,22,23,24,25,26,28,29,30,31,32,33,34,35,37,38,39,40,41,42,43,44,45,46,47,49,49,50,51,52,53,54,55,56,57,58,59,60,61,61,62,63]
  },
  "id":1
```

### obtener el saldo mínimo para la exención del alquiler

Devuelve el saldo mínimo necesario para hacer exento del alquiler de la cuenta.

#### Parámetros:

- `<usize>` - longitud de datos de la cuenta
- `<object>` - (optional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `<u64>` - Los lamports mínimos requeridos en la cuenta

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getMinimumBalanceForRentExemption"","params":[50]}
'
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 500, "id": 1 }
```

### obtener cuentas múltiples

Devuelve la información de la cuenta para una lista de Pubkeys

#### Parámetros:

- `<array>` - Una matriz de Pubkeys para consultar, como cadenas codificadas en base-58
- `<object>` - (opcional) objeto de configuración que contiene los siguientes campos opcionales:
  - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)
  - `codificación: <string>` - codificación para datos de cuenta, ya sea "base58" (_slow_), "base64", "base64+zstd", o "jsonParsed". "base58" se limita a los datos de la cuenta de menos de 128 bytes. "base64" devolverá datos codificados en base64 para datos de Cuenta de cualquier tamaño. "base64+zstd" comprime los datos de la cuenta usando [Zstandard](https://facebook.github.io/zstd/) y base64 codifica el resultado. La codificación "jsonParsed" intenta usar analizadores de estado específicos del programa para devolver datos de estado más legibles y explícitos de la cuenta. Si se solicita "jsonParsed" pero no se puede encontrar un analizador, el campo vuelve a la codificación "base64", detectable cuando el campo `data` es de tipo `<string>`.
  - (opcional) `datalice: <object>` - limitar los datos de la cuenta devuelta usando el `offset proporcionado: <usize>` y `longitud: <usize>` campos; sólo disponible para codificaciones "base58", "base64" o "base64+zstd".

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a:

Una matriz de:

- `<null>` - si la cuenta de ese Pubkey no existe
- `<object>` - de lo contrario, un objeto JSON que contiene:
  - `lamports: <u64>`, número de lamports asignados a esta cuenta, como u64
  - `propietario: <string>`, código base-58 del programa que esta cuenta ha sido asignada a
  - `datos: <[string, encoding]|object>`, datos asociados con la cuenta, ya sea como datos binarios codificados o formato JSON `{<program>: <state>}`, dependiendo del parámetro de codificación
  - `ejecutable: <bool>`, boolean indicando si la cuenta contiene un programa \(y es estrictamente de solo lectura\)
  - `rentEpoch: <u64>`, la época en el que se alquilará esta cuenta, como u64

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getMultipleAccounts",
    "params": [
      [
        "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
        "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA"
      ],
      {
        "dataSlice": {
          "offset": 0,
          "length": 0
        }
      }
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": ["AAAAAAEAAAACtzNsyJrW0g==", "base64"],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": ["", "base64"],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getMultipleAccounts",
    "params": [
      [
        "vines1vzrYbzLMRdu58ou5XTby4qAqVRLmqo36NKPTg",
        "4fYNw3dojWmQ4dXtSGE9epjRGy9pFSx62YypT7avPYvA"
      ],
      {
        "encoding": "base58"
      }
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": [
      {
        "data": [
          "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHRTPuR3oZ1EioKtYGiYxpxMG5vpbZLsbcBYBEmZZcMKaSoGx9JZeAuWf",
          "base58"
        ],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": ["", "base58"],
        "executable": false,
        "lamports": 5000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      }
    ]
  },
  "id": 1
}
```

### obtener Programar cuentas

Devuelve todas las cuentas propiedad del programa proporcionado Pubkey

#### Parámetros:

- `<string>` - Pubkey del programa, como cadena codificada en base-58
- `<object>` - (opcional) objeto de configuración que contiene los siguientes campos opcionales:
  - (opcional) [Comprobación](jsonrpc-api.md#configuring-state-commitment)
  - `codificación: <string>` - codificación para datos de cuenta, ya sea "base58" (_slow_), "base64", "base64+zstd", o "jsonParsed". "base58" se limita a los datos de la cuenta de menos de 128 bytes. "base64" devolverá datos codificados en base64 para datos de Cuenta de cualquier tamaño. "base64+zstd" comprime los datos de la cuenta usando [Zstandard](https://facebook.github.io/zstd/) y base64 codifica el resultado. La codificación "jsonParsed" intenta usar analizadores de estado específicos del programa para devolver datos de estado más legibles y explícitos de la cuenta. Si se solicita "jsonParsed" pero no se puede encontrar un analizador, el campo vuelve a la codificación "base64", detectable cuando el campo `data` es de tipo `<string>`.
  - (opcional) `datalice: <object>` - limitar los datos de la cuenta devuelta usando el `offset proporcionado: <usize>` y `longitud: <usize>` campos; sólo disponible para codificaciones "base58", "base64" o "base64+zstd".
  - (opcional) `filtros: <array>` - filtrar resultados usando varios [objetos de filtro](jsonrpc-api.md#filters); la cuenta debe cumplir con todos los criterios de filtro para ser incluida en los resultados

##### Filtros:

- `memcmp: <object>` - compara una serie proporcionada de bytes con los datos de la cuenta del programa en un desplazamiento particular. Campos:

  - `offset: <usize>` - offset en los datos de la cuenta del programa para iniciar la comparación
  - `bytes: <string>` - datos a coincidir, como cadena codificada en base-58

- `dataSize: <u64>` - compara la longitud de los datos de la cuenta del programa con el tamaño de datos proporcionado

#### Resultados:

El campo resultado será una matriz de objetos JSON, que contendrá:

- `pubkey: <string>` - la cuenta Pubkey como cadena codificada base-58
- `cuenta: <object>` - un objeto JSON, con los siguientes subcampos:
  - `lamports: <u64>`, número de lamports asignados a esta cuenta, como u64
  - `propietario: <string>`, base-58 codificado Pubkey del programa que esta cuenta ha sido asignada a `datos: <[string,encoding]|objeto>`, datos asociados con la cuenta, ya sea como datos binarios codificados o formato JSON `{<program>: <state>}`, dependiendo del parámetro de codificación
  - `ejecutable: <bool>`, boolean indicando si la cuenta contiene un programa \(y es estrictamente de solo lectura\)
  - `rentEpoch: <u64>`, la época en el que se alquilará esta cuenta, como u64

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getProgramAccounts", "params":["4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T"]}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getProgramAccounts",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "filters": [
          {
            "dataSize": 17
          },
          {
            "memcmp": {
              "offset": 4,
              "bytes": "3Mc6vR"
            }
          }
        ]
      }
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "account": {
        "data": "2R9jLfiAQ9bgdcw6h8s44439",
        "executable": false,
        "lamports": 15298080,
        "owner": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
        "rentEpoch": 28
      },
      "pubkey": "CxELquR1gPP8wHe33gZ4QxqGB3sZ9RSwsJ2KshVewkFY"
    }
  ],
  "id": 1
}
```

### obtener Blockhash reciente

Devuelve un hash de bloque reciente del libro mayor, y una tabla de tarifas que puede utilizarse para calcular el coste de enviar una transacción utilizándola.

#### Parámetros:

- `<object>` - (optional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

Un RpcResponse que contiene un objeto JSON consistente en un blockhash de cadena y objeto JSON de FeeCalculator.

- `RpcResponse<object>` - RpcResponse objeto JSON con `valor` campo establecido en un objeto JSON incluyendo:
- `blockhash: <string>` - un Hash como cadena codificada en base 58
- `feeCalculator: <object>` - Objeto FeeCalculator, el programa de comisión para este bloque hash

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d 'i
  {"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}
'
```

Resultado:

```json
"jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1
    },
    "value": {
      "blockhash": "CSymwgTNX1j3E4qhKfJAUE41nBWEwXufoYryPbkde5RR",
      "feeCalculator": {
        "lamportsPerSignature": 5000
      }
    }
  },
  "id": 1
}
```

### obtener muestras de rendimiento recientes

Devuelve una lista de muestras de rendimiento recientes, en orden de ranura inversa. Las muestras de rendimiento se toman cada 60 segundos e incluyen el número de transacciones y ranuras que ocurren en una ventana de tiempo determinada.

#### Parámetros:

- `límite: <usize>` - (opcional) número de muestras a devolver (máximo 720)

#### Resultados:

Una matriz de:

- `RpcPerfSample<object>`
  - `ranura: <u64>` - Ranura en la que se tomó la muestra
  - `numTransacciones: <u64>` - Número de transacciones en la muestra
  - `numSlots: <u64>` - Número de ranuras en muestra
  - `samplePeriodSecs: <u16>` - Número de segundos en una ventana de muestra

#### Ejemplo:

Solicitud:

```bash
// Solicitud
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getRecentPerformanceSamples", "params": [4]}
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 348125
    },
    {
      "numSlots": 126,
      "numTransactions": 126,
      "samplePeriodSecs": 60,
      "slot": 347999
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347873
    },
    {
      "numSlots": 125,
      "numTransactions": 125,
      "samplePeriodSecs": 60,
      "slot": 347748
    }
  ],
  "id": 1
}
```

### getSnapshotSlot

Devuelve la ranura más alta para la que el nodo tiene una instantánea para

#### Parámetros:

Ninguna

#### Resultados:

- `<u64>` - Snapshot slot

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSnapshotSlot"}
'
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 100, "id": 1 }
```

Resultado cuando el nodo no tiene ninguna instantánea:

```json
{
  "jsonrpc": "2.0",
  "error": { "code": -32008, "message": "No snapshot" },
  "id": 1
}
```

### obtener Estados de las firmas

Devuelve los estados de una lista de firmas. A menos que se incluya el parámetro de configuración `buscar historial de transacciones`, este método sólo busca en la caché de estados recientes de las firmas, que conserva los estados de todas las ranuras activas más `MAX_RECENT_BLOCKHASHES` ranuras rooteadas.

#### Parámetros:

- `<array>` - Una matriz de firmas de transacciones para confirmar, como cadenas codificadas en base-58
- `<object>` - objeto de configuración (opcional) que contiene el siguiente campo:
  - `buscar el historial de transacciones: <bool>` - si es verdadero, un nodo Solana buscará en su caché del libro mayor cualquier firma que no se encuentre en la caché de estados recientes

#### Resultados:

Un RpcResponse que contiene un objeto JSON que consiste en una matriz de objetos TransactionStatus.

- `RpcResponse<object>` - RpcResponse objeto JSON con `valor` campo:

Una matriz de:

- `<null>` - Transacción desconocida
- `<object>`
  - `ranura: <u64>` - La ranura en la que la transacción fue procesada
  - `confirmaciones: <usize | null>` - Número de bloques desde la confirmación de la firma, nulo si es rooteado, así como finalizado por una supermayoría del clúster
  - `err: <object | null>` - Error si la transacción falló, null si la transacción tuvo éxito. [Definiciones de Error de Transacción](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `confirmationStatus: <string | null>` - Estado de confirmación del clúster de la transacción; `procesado`, `confirmado`o `finalizado`. Vea [Commitment](jsonrpc-api.md#configuring-state-commitment) para más sobre confirmación optimista.
  - DEPRECIADO: `status: <object>` - Estado de la transacción
    - `"Ok": <null>` - La transacción fue exitosa
    - `"Err": <ERR>` - Transacción fallida con TransactionError

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
        "5j7s6NiJS3JAkvgkoc18WVAsiSaci2pxB2A6ueCJP4tprA2TFg9wSyTLeYouxPBJEMzJinENTkpA52YStRW5Dia7"
      ]
    ]
  }
'
```

Resultado:

```json
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
        },
        "confirmationStatus": "confirmed"
      },
      null
    ]
  },
  "id": 1
}
```

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignatureStatuses",
    "params": [
      [
        "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW"
      ],
      {
        "searchTransactionHistory": true
      }
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 82
    },
    "value": [
      {
        "slot": 48,
        "confirmations": null,
        "err": null,
        "status": {
          "Ok": null
        },
        "confirmationStatus": "finalized"
      },
      null
    ]
  },
  "id": 1
}
```

### obtener ranura

Devuelve el slot actual que el nodo está procesando

#### Parámetros:

- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `<u64>` - Ranura actual

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### obtener ranura lider

Devuelve el líder de la ranura actual

#### Parámetros:

- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `<string>` - Node identity Pubkey como cadena codificada base-58

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": "ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS",
  "id": 1
}
```

### obtener Activación de stake

Devuelve la información sobre la activación de la época de una cuenta de stake

#### Parámetros:

- `<string>` - Bloque de cuenta de stake a consultar, como cadena codificada en base a 58
- `<object>` - (opcional) objeto de configuración que contiene los siguientes campos opcionales:
  - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)
  - (opcional) `epoca: <u64>` - época para la que calcular los detalles de activación. Si el parámetro no se proporciona, el valor predeterminado es el de la época actual.

#### Resultados:

El resultado será un objeto JSON con los siguientes campos:

- `estado: <cadena` - el estado de activación de la cuenta de estado, uno de: `activo`, `inactivos`, `activando`, `desactivando`
- `activo: <u64>` - stake activo durante la época
- `inactivo: <u64>` - stake inactivo durante la época

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getStakeActivation", "params": ["CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT"]}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": { "active": 197717120, "inactive": 0, "state": "active" },
  "id": 1
}
```

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getStakeActivation",
    "params": [
      "CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT",
      {
        "epoch": 4
      }
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "active": 124429280,
    "inactive": 73287840,
    "state": "activating"
  },
  "id": 1
}
```

### obtener suministro

Devuelve información sobre el suministro actual.

#### Parámetros:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a un objeto JSON que contiene:

- `total: <u64>` - Suministro total en lamports
- `circulante: <u64>` - Suministro circulante en lamports
- `no Circulando: <u64>` - Suministro no circulante en lamports
- `Cuentas no circulantes: <array>` - una matriz de direcciones de cuentas no circulantes, como cadenas

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSupply"}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "circulating": 16000,
      "nonCirculating": 1000000,
      "nonCirculatingAccounts": [
        "FEy8pTbP5fEoqMV1GdTz83byuA8EKByqYat1PKDgVAq5",
        "9huDUZfxoJ7wGMTffUE7vh1xePqef7gyrLJu9NApncqA",
        "3mi1GmwEE3zo2jmfDuzvjSX9ovRXsDUKHvsntpkhuLJ9",
        "BYxEJTDerkaRWBem3XgnVcdhppktBXa2HbkHPKj2Ui4Z"
      ],
      "total": 1016000
    }
  },
  "id": 1
}
```

### obtener el saldo de la cuenta deToken

Devuelve el saldo del token de una cuenta SPL Token. **UNSTABLE**

#### Parámetros:

- `<string>` - La Pubkey de la cuenta de token a consultar, como cadena codificada en base a 58
- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a un objeto JSON que contiene:

- `uiAmount: <f64>` - el saldo, usando decimales prescritos
- `monto: <string>` - el balance bruto sin decimales, una representación de cadena de u64
- `decimales: <u8>` - número de 10 dígitos base a la derecha del decimal

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenAccountBalance", "params": ["7fUAJdStEuGbc3sM84cKRL6yYaaSstyLSU4ve5oovLS7"]}
'
```

Resultado:

```json
"jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "uiAmount": 98.64,
      "amount": "9864",
      "decimals": 2
    },
    "id": 1
  }
}
```

### obtener cuentas de tokens por delegado

Devuelve todas las cuentas SPL Token por Delegate aprobado. **UNSTABLE**

#### Parámetros:

- `<string>` - Pubkey de la cuenta delegada a consulta, como cadena codificada en base a 58
- `<object>` - tampoco:
  - `mint: <string>` - Clave Pubkey del token específico a la que limitar las cuentas, como cadena codificada en base-58; o
  - `programId: <string>` - Pubkey del ID del programa Token que posee las cuentas, como cadena codificada en base 58
- `<object>` - (opcional) objeto de configuración que contiene los siguientes campos opcionales:
  - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)
  - `codificación: <string>` - codificación para datos de cuenta, ya sea "base58" (_slow_), "base64", "base64+zstd", o "jsonParsed". La codificación "jsonParsed" intenta usar analizadores de estado específicos del programa para devolver datos de estado más legibles y explícitos de la cuenta. Si se solicita "jsonParsed" pero no se puede encontrar una moneda válida para una cuenta en particular, esa cuenta se filtrará de los resultados.
  - (opcional) `datalice: <object>` - limitar los datos de la cuenta devuelta usando el `offset proporcionado: <usize>` y `longitud: <usize>` campos; sólo disponible para codificaciones "base58", "base64" o "base64+zstd".

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a una matriz de objetos JSON, que contiene:

- `pubkey: <string>` - la cuenta Pubkey como cadena codificada base-58
- `cuenta: <object>` - un objeto JSON, con los siguientes subcampos:
  - `lamports: <u64>`, número de lamports asignados a esta cuenta, como u64
  - `propietario: <string>`, código base-58 del programa al que esta cuenta ha sido asignada
  - `datos: <object>`, datos del estado del token asociados con la cuenta, ya sea como datos binarios codificados o en formato JSON `{<program>: <state>}`
  - `ejecutable: <bool>`, boolean indicando si la cuenta contiene un programa \(y es estrictamente de solo lectura\)
  - `rentEpoch: <u64>`, el epicentro en el que se alquilará esta cuenta, como u64

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByDelegate",
    "params": [
      "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
      {
        "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "uiAmount": 0.1,
                "decimals": 1
              },
              "delegate": "4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T",
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "CnPoSPKXu7wJqxe59Fs72tkBeALovhsCxYeFwPCQH9TD"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### obtener cuentas de tokens por propietario

Devuelve todas las cuentas SPL Token por token poseído. **UNSTABLE**

#### Parámetros:

- `<string>` - Bloque del dueño de la cuenta a consultar, como cadena codificada en base-58
- `<object>` - tampoco:
  - `mint: <string>` - Clave Pubkey del token específico a la que limitar las cuentas, como cadena codificada en base-58; o
  - `programId: <string>` - Pubkey del ID del programa Token que posee las cuentas, como cadena codificada en base 58
- `<object>` - objeto de configuración (opcional) que contiene los siguientes campos opcionales:
  - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)
  - `codificación: <string>` - codificación para datos de cuenta, ya sea "base58" (_slow_), "base64", "base64+zstd", o "jsonParsed". La codificación "jsonParsed" intenta usar analizadores de estado específicos del programa para devolver datos de estado más legibles y explícitos de la cuenta. Si se solicita "jsonParsed" pero no se puede encontrar una moneda válida para una cuenta en particular, esa cuenta se filtrará de los resultados.
  - (opcional) `datalice: <object>` - limitar los datos de la cuenta devuelta usando el `offset proporcionado: <usize>` y `longitud: <usize>` campos; sólo disponible para codificaciones "base58", "base64" o "base64+zstd".

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a una matriz de objetos JSON, que contiene:

- `pubkey: <string>` - la cuenta Pubkey como cadena codificada base-58
- `cuenta: <object>` - un objeto JSON, con los siguientes subcampos:
  - `lamports: <u64>`, número de lamports asignados a esta cuenta, como u64
  - `propietario: <string>`, código base-58 del programa que esta cuenta ha sido asignada a
  - `datos: <object>`, datos del estado del token asociados con la cuenta, ya sea como datos binarios codificados o en formato JSON `{<program>: <state>}`
  - `ejecutable: <bool>`, boolean indicando si la cuenta contiene un programa \(y es estrictamente de solo lectura\)
  - `rentEpoch: <u64>`, el epicentro en el que se alquilará esta cuenta, como u64

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByOwner",
    "params": [
      "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F",
      {
        "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"
      },
      {
        "encoding": "jsonParsed"
      }
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "data": {
          "program": "spl-token",
          "parsed": {
            "accountType": "account",
            "info": {
              "tokenAmount": {
                "amount": "1",
                "uiAmount": 0.1,
                "decimals": 1
              },
              "delegate": null,
              "delegatedAmount": 1,
              "isInitialized": true,
              "isNative": false,
              "mint": "3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E",
              "owner": "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F"
            }
          }
        },
        "executable": false,
        "lamports": 1726080,
        "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "rentEpoch": 4
      }
    ]
  },
  "id": 1
}
```

### obtener cuentas más grandes

Devuelve las 20 cuentas más grandes de un tipo particular de token SPL. **UNSTABLE**

#### Parámetros:

- `<string>` - Pubkey de token Mint a consultar, como cadena codificada en base a 58
- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a una matriz de objetos JSON que contienen:

- `dirección: <string>` - la dirección de la cuenta del token
- `uiAmount: <f64>` - el saldo de cuenta de tokens, usando decimales prescritos
- `monto: <string>` - el saldo bruto de cuenta de token sin decimales, una representación de cadena de u64
- `decimales: <u8>` - número de 10 dígitos base a la derecha del decimal

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenLargestAccounts", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": [
      {
        "address": "FYjHNoFtSQ5uijKrZFyYAxvEr87hsKXkXcxkcmkBAf4r",
        "amount": "771",
        "decimals": 2,
        "uiAmount": 7.71
      },
      {
        "address": "BnsywxTcaYeNUtzrPxQUvzAWxfzZe3ZLUJ4wMMuLESnu",
        "amount": "229",
        "decimals": 2,
        "uiAmount": 2.29
      }
    ]
  },
  "id": 1
}
```

### obtener suministro

Devuelve el suministro total de un tipo de token SPL. **UNSTABLE**

#### Parámetros:

- `<string>` - Pubkey de token Mint a consultar, como cadena codificada en base a 58
- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

El resultado será un objeto RpcResponse JSON con `valor` igual a un objeto JSON que contiene:

- `uiAmount: <f64>` - el suministro total de tokens, usando decimales prescritos
- `monto: <string>` - el suministro total de tokens sin decimales, una representación de cadena de u64
- `decimales: <u8>` - número de dígitos de base 10 a la derecha del decimal

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenSupply", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "uiAmount": 1000,
      "amount": "100000",
      "decimals": 2
    }
  },
  "id": 1
}
```

### obtener el número de transacciones

Devuelve el recuento actual de transacciones del ledger

#### Parámetros:

- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `<u64>` - contador

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}
'

```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 268, "id": 1 }
```

### obtener Version

Devuelve las versiones actuales de solana corriendo en el nodo

#### Parámetros:

Ninguna

#### Resultados:

El campo resultado será un objeto JSON con los siguientes campos:

- `solana-core`, versión de software de solana-core
- `feature-set`, identificador único del conjunto de características del software actual

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVersion"}
'
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": { "solana-core": "1.6.0" }, "id": 1 }
```

### obtener Cuentas de voto

Devuelve la información de la cuenta y la participación asociada para todas las cuentas de votación en el banco actual.

#### Parámetros:

- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

El campo de resultado será un objeto JSON de `cuentas actuales` y `contables` pendientes, cada uno conteniendo una matriz de objetos JSON con los siguientes subcampos:

- `votePubkey: <string>` - Clave pública de la cuenta de voto, como cadena codificada en base a 58
- `nodePubkey: <string>` - Clave pública de Node, como cadena codificada en base-58
- `Stake activado: <u64>` - el stake en lamports, delegado a esta cuenta de voto y activo en esta época
- `epochVoteAccount: <bool>` - bool, si la cuenta de voto es puesta en stake para esta época
- `comisión: <number>`, porcentaje (0-100) del pago de recompensas debidas a la cuenta de voto
- `último Voto: <u64>` - Ranura más reciente votada por esta cuenta de voto
- `EpochCredits: <array>` - Historia de cuántos créditos ganados al final de cada época, como una matriz de arreglos que contienen: `[epoch, credits, previousCredits]`

#### Ejemplo:

Solicitud:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "current": [
      {
        "commission": 0,
        "epochVoteAccount": true,
        "epochCredits": [
          [1, 64, 0],
          [2, 192, 64]
        ],
        "nodePubkey": "B97CCUW3AEZFGy6uUg6zUdnNYvnVq5VG8PUtb2HayTDD",
        "lastVote": 147,
        "activatedStake": 42,
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
      }
    ],
    "delinquent": [
      {
        "commission": 127,
        "epochVoteAccount": false,
        "epochCredits": [],
        "nodePubkey": "6ZPxeQaDo4bkZLRsdNrCzchNQr5LN9QMc9sipXv9Kw8f",
        "lastVote": 0,
        "activatedStake": 0,
        "votePubkey": "CmgCk4aMS7KW1SHX3s9K5tBJ6Yng2LBaC8MFov4wx9sm"
      }
    ]
  },
  "id": 1
}
```

### espacio mínimo del Ledger

Devuelve la ranura más baja sobre la que el nodo tiene información en su ledger. Este valor puede aumentar con el tiempo si el nodo está configurado para purgar datos antiguos del ledger

#### Parámetros:

Ninguna

#### Resultados:

- `u64` - Ranura mínima del Ledger

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### solicitar Airdrop

Solicita un airdrop de lamports a un Pubkey

#### Parámetros:

- `<string>` - Bloque de cuenta para recibir lamports, como cadena codificada base-58
- `<integer>` - lamports, como u64
- `<object>` - (opcional) [Commitment](jsonrpc-api.md#configuring-state-commitment) (usado para recuperar el blockhash y verificar el éxito del airdrop)

#### Resultados:

- `<string>` - Firma de la transacción del airdrop, como cadena codificada base-58

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 50]}
'

```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
  "id": 1
}
```

### enviar Transacción

Envía una transacción firmada al clúster para procesarla.

Este método no altera la transacción de ninguna manera; transmite la transacción creada por los clientes al nodo tal como es.

Si el servicio rpc del nodo recibe la transacción, este método inmediatamente tiene éxito, sin esperar ninguna confirmación. Una respuesta exitosa de este método no garantiza que la transacción sea procesada o confirmada por el clúster.

Mientras que el servicio rpc intentará razonablemente enviarlo, la transacción podría ser rechazada si la transacción `recent_blockhash` expira antes de que aterrice.

Utilice [`getSignatureStatuses`](jsonrpc-api.md#getsignaturestatuses) para asegurarse de que una transacción es procesada y confirmada.

Antes de enviar, se realizan las siguientes comprobaciones de prevuelo:

1. Las firmas de transacción son verificadas
2. La transacción se simula contra la ranura bancaria especificada por el compromiso de prevuelo. En caso de fallo, se devolverá un error. Las verificaciones de reposición pueden estar desactivadas si se desea. Se recomienda especificar el mismo compromiso y compromiso de prevuelo para evitar comportamientos confusos.

La firma devuelta es la primera en la transacción, la cual se utiliza para identificar la transacción ([Id de transacción](../../terminology.md#transanction-id)). Este identificador puede extraerse fácilmente de los datos de la transacción antes de enviar.

#### Parámetros:

- `<string>` - Transacción completamente firmada, como cadena codificada
- `<object>` - objeto de configuración (opcional) que contiene el siguiente campo:
  - `skipPreflight: <bool>` - Si es verdadero, omite las verificaciones de la transacción de prevuelo (por defecto: falso)
  - `preflightCommitment: <string>` - (opcional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level a usar para prevuelo (por defecto: `"max"`).
  - `codificación: <string>` - Codificación (opcional) utilizada para los datos de transacción. O bien `"base58"` (_lento_, **DEPRECIADO**), o `"base64"`. (por defecto: `"base58"`).

#### Resultados:

- `<string>` - Primera firma de transacción incrustada en la transacción, como cadena codificada en base a 58 ([id de transacción](../../terminology.md#transanction-id))

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "sendTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'

```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": "2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwE5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb",
  "id": 1
}
```

### simular Transacción

Simular el envío de una transacción

#### Parámetros:

- `<string>` - Transacción, como una cadena codificada. La transacción debe tener un blockhash válido, pero no es necesario que se firme.
- `<object>` - objeto de configuración (opcional) que contiene el siguiente campo:
  - `sigVerify: <bool>` - Si es verdad las firmas de transacción serán verificadas (por defecto: falso)
  - `commitment: <string>` - (opcional) [Commitment](jsonrpc-api.md#configuring-state-commitment) nivel para simular la transacción en (por defecto: `"max"`).
  - `codificación: <string>` - Codificación (opcional) utilizada para los datos de transacción. O bien `"base58"` (_lento_, **DEPRECIADO**), o `"base64"`. (por defecto: `"base58"`).

#### Resultados:

Un RpcResponse que contiene un objeto TransactionStatus El resultado será un objeto RpcResponse JSON con `valor` establecido a un objeto JSON con los siguientes campos:

- `err: <object | string | null>` - Error si la transacción falló, null si la transacción tuvo éxito. [Definiciones de Error de Transacción](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs: <array | null>` - Matriz de mensajes de registro que las instrucciones de la transacción emiten durante la ejecución, null si la simulación falló antes de que la transacción pudiera ejecutarse (por ejemplo, debido a un fallo en el blockhash o en la verificación de la firma)

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "simulateTransaction",
    "params": [
      "4hXTCkRzt9WyecNzV1XPgCDfGAZzQKNxLXgynz5QDuWWPSAZBZSHptvWRL3BjCvzUXRdKvHL2b7yGrRQcWyaqsaBCncVG7BFggS8w9snUts67BSh3EqKpXLUm5UMHfD7ZBe9GhARjbNQMLJ1QD3Spr6oMTBU6EhdB4RD8CP2xUxr2u3d6fos36PD98XS6oX8TQjLpsMwncs5DAMiD4nNnR8NBfyghGCWvCVifVwvA8B8TJxE1aiyiv2L429BCWfyzAme5sZW8rDb14NeCQHhZbtNqfXhcp2tAnaAT"
    ]
  }
'
```

Resultado:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 218
    },
    "value": {
      "err": null,
      "logs": [
        "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
      ]
    }
  },
  "id": 1
}
```

### establecer el filtro de registro

Establece el filtro de registro en el validador

#### Parámetros:

- `<string>` - el nuevo filtro de registro a usar

#### Resultados:

- `<null>`

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"setLogFilter", "params":["solana_core=debug"]}
'
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": null, "id": 1 }
```

### salida de Validador

Si un validador arranca con la salida RPC habilitada (`parámetro --enable-rpc-exit`), esta solicitud causa que el validador salga.

#### Parámetros:

Ninguna

#### Resultados:

- `<bool>` - Si la operación de salida del validador fue exitosa

#### Ejemplo:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"validatorExit"}
'

```

Resultado:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

## Suscripción Websocket

Después de conectarse al websocket RPC PubSubb en `ws://<ADDRESS>/`:

- Enviar solicitudes de suscripción al websocket utilizando los métodos a continuación
- Múltiples suscripciones pueden estar activas a la vez
- Muchas suscripciones toman el parámetro opcional [`compromiso`](jsonrpc-api.md#configuring-state-commitment), definiendo lo finalizado que debe ser un cambio para desencadenar una notificación. Para suscripciones, si el compromiso no se especifica, el valor predeterminado es `"singleGossip"`.

### cuenta Suscripción

Suscríbete a una cuenta para recibir notificaciones cuando cambien los lamports o datos de una cuenta pública

#### Parámetros:

- `<string>` - cuenta Pubkey, como cadena codificada base-58
- `<object>` - objeto de configuración (opcional) que contiene los siguientes campos opcionales:
  - `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)
  - `codificación: <string>` - codificación para datos de cuenta, ya sea "base58" (_slow_), "base64", "base64+zstd", o "jsonParsed". La codificación "jsonParsed" intenta usar analizadores de estado específicos del programa para devolver datos de estado más legibles y explícitos de la cuenta. Si se solicita "jsonParsed" pero no se puede encontrar un analizador, el campo vuelve a la codificación binaria, detectable cuando el campo `data` es de tipo `<string>`.

#### Resultados:

- `<number>` - Id de suscripción \(necesario para cancelar la suscripción\)

#### Ejemplo:

Solicitud:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "base64",
      "commitment": "root"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "jsonParsed"
    }
  ]
}
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 23784, "id": 1 }
```

#### Formato de notificación:

Codificación Base58:

```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": [
          "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR",
          "base58"
        ],
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

Codificación JSON analizada:

```json
{
  "jsonrpc": "2.0",
  "method": "accountNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5199307
      },
      "value": {
        "data": {
          "program": "nonce",
          "parsed": {
            "type": "initialized",
            "info": {
              "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
              "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
              "feeCalculator": {
                "lamportsPerSignature": 5000
              }
            }
          }
        },
        "executable": false,
        "lamports": 33594,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 635
      }
    },
    "subscription": 23784
  }
}
```

### cancelar suscripción

Desinscribirse de las notificaciones de cambio de cuenta

#### Parámetros:

- `<number>` - id de la suscripción a la cuenta para cancelar

#### Resultados:

- `<bool>` - mensaje de anulación de suscripción correcta

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "accountUnsubscribe", "params": [0] }
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### registros Suscripción

Suscríbete al registro de transacciones. **UNSTABLE**

#### Parámetros:

- `filtro: <string>|<object>` - criterios de filtro para que los registros reciban resultados por tipo de cuenta; actualmente soportados:
  - "todos" - suscríbete a todas las transacciones excepto para transacciones de voto simples
  - "allWithVotes" - suscribirse a todas las transacciones incluyendo transacciones de voto simples
  - `{ "mentions": [ <string> ] }` - suscribirse a todas las transacciones que mencionan la clave Pubkey proporcionada (como cadena codificada base-58)
- `<object>` - (opcional) objeto de configuración que contiene los siguientes campos opcionales:
  - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `<integer>` - Id de suscripción \(necesario para cancelar la suscripción\)

#### Ejemplo:

Solicitud:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [
    {
      "mentions": [ "11111111111111111111111111111111" ]
    },
    {
      "commitment": "max"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "logsSubscribe",
  "params": [ "all" ]
}
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Formato de notificación:

Codificación Base58:

```json
{
  "jsonrpc": "2.0",
  "method": "logsNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
        "err": null,
        "logs": [
          "BPF program 83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri success"
        ]
      }
    },
    "subscription": 24040
  }
}
```

### registros anular suscripción

Cancelar suscripción al registro de transacciones

#### Parámetros:

- `<integer>` - id de suscripción a cancelar

#### Resultados:

- `<bool>` - mensaje de anulación de suscripción correcta

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "logsUnsubscribe", "params": [0] }
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### programa suscribirse

Suscríbete a un programa para recibir notificaciones cuando los lamports o datos de una cuenta determinada propiedad del programa cambien

#### Parámetros:

- `<string>` - program_id Pubkey, como cadena codificada en base-58
- `<object>` - objeto de configuración (opcional) que contiene los siguientes campos opcionales:
  - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)
  - `codificación: <string>` - codificación para datos de cuenta, ya sea "base58" (_slow_), "base64", "base64+zstd", o "jsonParsed". La codificación "jsonParsed" intenta usar analizadores de estado específicos del programa para devolver datos de estado más legibles y explícitos de la cuenta. Si se solicita "jsonParsed" pero no se puede encontrar un analizador, el campo vuelve a la codificación base64, detectable cuando el campo `data` es de tipo `<string>`.
  - (opcional) `filtros: <array>` - filtrar resultados usando varios [objetos de filtro](jsonrpc-api.md#filters); la cuenta debe cumplir con todos los criterios de filtro para ser incluida en los resultados

#### Resultados:

- `<integer>` - Id de suscripción \(necesario para cancelar la suscripción\)

#### Ejemplo:

Solicitud:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "commitment": "max"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "jsonParsed"
    }
  ]
}
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "filters": [
        {
          "dataSize": 80
        }
      ]
    }
  ]
}
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Formato de notificación:

Codificación Base58:

```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": [
            "11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR",
            "base58"
          ],
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        }
      }
    },
    "subscription": 24040
  }
}
```

Codificación JSON analizada:

```json
{
  "jsonrpc": "2.0",
  "method": "programNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5208469
      },
      "value": {
        "pubkey": "H4vnBqifaSACnKa7acsxstsY1iV1bvJNxsCY7enrd1hq",
        "account": {
          "data": {
            "program": "nonce",
            "parsed": {
              "type": "initialized",
              "info": {
                "authority": "Bbqg1M4YVVfbhEzwA9SpC9FhsaG83YMTYoR4a8oTDLX",
                "blockhash": "LUaQTmM7WbMRiATdMMHaRGakPtCkc2GHtH57STKXs6k",
                "feeCalculator": {
                  "lamportsPerSignature": 5000
                }
              }
            }
          },
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        }
      }
    },
    "subscription": 24040
  }
}
```

### programa Anular suscripción

Anular suscripción de las notificaciones de cambio de cuenta de propiedad del programa

#### Parámetros:

- `<integer>` - id de la suscripción a la cuenta para cancelar

#### Resultados:

- `<bool>` - mensaje de anulación de suscripción correcta

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "programUnsubscribe", "params": [0] }
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### suscribirse a firma

Suscríbete a una firma de transacción para recibir una notificación cuando la transacción se confirma en `signatureNotification`, la suscripción se cancela automáticamente

#### Parámetros:

- `<string>` - Firma de la transacción, como cadena codificada base-58
- `<object>` - (opcional) [Compromiso](jsonrpc-api.md#configuring-state-commitment)

#### Resultados:

- `integer` - Id de suscripción \(necesario para anular suscripción\)

#### Ejemplo:

Solicitud:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b"
  ]
}

{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "signatureSubscribe",
  "params": [
    "2EBVM6cB8vAAD93Ktr6Vd8p67XPbQzCJX47MpReuiCXJAtcjaxpvWpcg9Ege1Nr5Tk3a2GFrByT7WPBjdsTycY9b",
    {
      "commitment": "max"
    }
  ]
}
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Formato de notificación:

```bash
{
  "jsonrpc": "2.0",
  "method": "signatureNotification",
  "params": {
    "result": {
      "context": {
        "slot": 5207624
      },
      "value": {
        "err": null
      }
    },
    "subscription": 24006
  }
}
```

### dejar de suscribirse

Desuscribirse de la notificación de confirmación de la firma

#### Parámetros:

- `<integer>` - id de suscripción a cancelar

#### Resultados:

- `<bool>` - mensaje de anulación de suscripción correcta

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "signatureUnsubscribe", "params": [0] }
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### suscribirse Ranura

Suscríbete para recibir notificaciones en cualquier momento en que el validador procese una ranura

#### Parámetros:

Ninguna

#### Resultados:

- `integer` - Id de suscripción \(necesario para anular suscripción\)

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotSubscribe" }
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Formato de notificación:

```bash
{
  "jsonrpc": "2.0",
  "method": "slotNotification",
  "params": {
    "result": {
      "parent": 75,
      "root": 44,
      "slot": 76
    },
    "subscription": 0
  }
}
```

### ranura Anular suscripción

Anular suscripción de las notificaciones de ranura

#### Parámetros:

- `<integer>` - id de suscripción a cancelar

#### Resultados:

- `<bool>` - mensaje de anulación de suscripción correcta

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotUnsubscribe", "params": [0] }
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### root Suscribirse

Suscríbase para recibir una notificación cada vez que el validador establezca un nuevo root.

#### Parámetros:

Ninguna

#### Resultados:

- `integer` - Id de suscripción \(necesario para anular suscripción\)

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootSubscribe" }
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Formato de notificación:

El resultado es el último número de ranura raíz.

```bash
{
  "jsonrpc": "2.0",
  "method": "rootNotification",
  "params": {
    "result": 42,
    "subscription": 0
  }
}
```

### anular suscripción de root

Anular suscripción de las notificaciones root

#### Parámetros:

- `<integer>` - id de suscripción a cancelar

#### Resultados:

- `<bool>` - mensaje de anulación de suscripción correcta

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootUnsubscribe", "params": [0] }
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### voteSusscribe - inestable, deshabilitado por defecto

**Esta suscripción es inestable y solo está disponible si el validador se inició con la bandera `--rpc-pubsub-enable-vote-subscription`. El formato de esta suscripción puede cambiar en el futuro**

Suscríbete para recibir notificaciones en cualquier momento en que se observa una nueva votación en gossip. Estos votos son pre-consenso, por lo tanto no hay garantía de que estos votos ingresen al ledger.

#### Parámetros:

Ninguna

#### Resultados:

- `integer` - Id de suscripción \(necesario para anular suscripción\)

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteSubscribe" }
```

Resultado:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Formato de notificación:

El resultado es la última votación, que contiene su hash, una lista de listas de votos y una marca de tiempo opcional.

```json
{
  "jsonrpc": "2.0",
  "method": "voteNotification",
  "params": {
    "result": {
      "hash": "8Rshv2oMkPu5E4opXTRyuyBeZBqQ4S477VG26wUTFxUM",
      "slots": [1, 2],
      "timestamp": null
    },
    "subscription": 0
  }
}
```

### desuscribir voto

Anular suscripción de las notificaciones de voto

#### Parámetros:

- `<integer>` - id de suscripción a cancelar

#### Resultados:

- `<bool>` - mensaje de anulación de suscripción correcta

#### Ejemplo:

Solicitud:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteUnsubscribe", "params": [0] }
```

Respuesta:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```
