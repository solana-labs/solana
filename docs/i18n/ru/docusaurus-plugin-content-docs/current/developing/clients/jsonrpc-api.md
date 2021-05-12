---
title: JSON RPC API
---

Узлы Solana принимают HTTP запросы, используя спецификацию [JSON-RPC 2.0](https://www.jsonrpc.org/specification).

Чтобы взаимодействовать с узлом Solana внутри JavaScript-приложения, используйте [solana-web3. s](https://github.com/solana-labs/solana-web3.js) библиотека, которая предоставляет удобный интерфейс для методов RPC.

## Конечная точка HTTP RPC

**Default port:** 8899 eg. [http://localhost:8899](http://localhost:8899), [http://192.168.1.88:8899](http://192.168.1.88:8899)

## RPC PubSub WebSocket эндпоинт

**Default port:** 8900 eg. ws://localhost:8900, [http://192.168.1.88:8900](http://192.168.1.88:8900)

## Методы

- [getAccountInfo](jsonrpc-api.md#getaccountinfo)
- [getBalance](jsonrpc-api.md#getbalance)
- [getBlock](jsonrpc-api.md#getblock)
- [getBlockProduction](jsonrpc-api.md#getblockproduction)
- [getBlockCommitment](jsonrpc-api.md#getblockcommitment)
- [getBlocks](jsonrpc-api.md#getblocks)
- [getBlocksWithLimit](jsonrpc-api.md#getblockswithlimit)
- [getBlockTime](jsonrpc-api.md#getblocktime)
- [getClusterNodes](jsonrpc-api.md#getclusternodes)
- [getEpochInfo](jsonrpc-api.md#getepochinfo)
- [getEpochSchedule](jsonrpc-api.md#getepochschedule)
- [getFeeCalculatorForBlockhash](jsonrpc-api.md#getfeecalculatorforblockhash)
- [getFeeRateGovernor](jsonrpc-api.md#getfeerategovernor)
- [getFees](jsonrpc-api.md#getfees)
- [getFirstAvailableBlock](jsonrpc-api.md#getfirstavailableblock)
- [getGenesisHash](jsonrpc-api.md#getgenesishash)
- [getHealth](jsonrpc-api.md#gethealth)
- [getIdentity](jsonrpc-api.md#getidentity)
- [getInflationGovernor](jsonrpc-api.md#getinflationgovernor)
- [getInflationRate](jsonrpc-api.md#getinflationrate)
- [getInflationReward](jsonrpc-api.md#getinflationreward)
- [getLargestAccounts](jsonrpc-api.md#getlargestaccounts)
- [getLeaderSchedule](jsonrpc-api.md#getleaderschedule)
- [getMaxRetransmitSlot](jsonrpc-api.md#getmaxretransmitslot)
- [getMaxShredInsertSlot](jsonrpc-api.md#getmaxshredinsertslot)
- [getMinimumBalanceForRentExemption](jsonrpc-api.md#getminimumbalanceforrentexemption)
- [getMultipleAccounts](jsonrpc-api.md#getmultipleaccounts)
- [getProgramAccounts](jsonrpc-api.md#getprogramaccounts)
- [getRecentBlockhash](jsonrpc-api.md#getrecentblockhash)
- [getRecentPerformanceSamples](jsonrpc-api.md#getrecentperformancesamples)
- [getSignaturesForAddress](jsonrpc-api.md#getsignaturesforaddress)
- [getSignatureStatuses](jsonrpc-api.md#getsignaturestatuses)
- [getSlot](jsonrpc-api.md#getslot)
- [getSlotLeader](jsonrpc-api.md#getslotleader)
- [getSlotLeaders](jsonrpc-api.md#getslotleaders)
- [getkeActivation](jsonrpc-api.md#getstakeactivation)
- [getSupply](jsonrpc-api.md#getsupply)
- [getTokenAccountBalance](jsonrpc-api.md#gettokenaccountbalance)
- [getTokenAccountsByDelegate](jsonrpc-api.md#gettokenaccountsbydelegate)
- [getTokenAccountsByOwner](jsonrpc-api.md#gettokenaccountsbyowner)
- [getTokenLargestAccounts](jsonrpc-api.md#gettokenlargestaccounts)
- [getTokenSupply](jsonrpc-api.md#gettokensupply)
- [getTransaction](jsonrpc-api.md#gettransaction)
- [getTransactionCount](jsonrpc-api.md#gettransactioncount)
- [getVersion](jsonrpc-api.md#getversion)
- [getVoteAccounts](jsonrpc-api.md#getvoteaccounts)
- [minimumLedgerSlot](jsonrpc-api.md#minimumledgerslot)
- [requestAirdrop](jsonrpc-api.md#requestairdrop)
- [sendTransaction](jsonrpc-api.md#sendtransaction)
- [simulateTransaction](jsonrpc-api.md#simulatetransaction)
- [Subscription Websocket](jsonrpc-api.md#subscription-websocket)
  - [accountSubscribe](jsonrpc-api.md#accountsubscribe)
  - [accountUnsubscribe](jsonrpc-api.md#accountunsubscribe)
  - [logsSubscribe](jsonrpc-api.md#logssubscribe)
  - [logsUnsubscribe](jsonrpc-api.md#logsunsubscribe)
  - [programSubscribe](jsonrpc-api.md#programsubscribe)
  - [programUnsubscribe](jsonrpc-api.md#programunsubscribe)
  - [signatureSubscribe](jsonrpc-api.md#signaturesubscribe)
  - [signatureUnsubscribe](jsonrpc-api.md#signatureunsubscribe)
  - [slotSubscribe](jsonrpc-api.md#slotsubscribe)
  - [slotUnsubscribe](jsonrpc-api.md#slotunsubscribe)

### Deprecated Methods

- [getConfirmedBlock](jsonrpc-api.md#getconfirmedblock)
- [getConfirmedBlocks](jsonrpc-api.md#getconfirmedblocks)
- [getConfirmedBlocksWithit](jsonrpc-api.md#getconfirmedblockswithlimit)
- [getConfirmedSignaturesForAddres2](jsonrpc-api.md#getconfirmedsignaturesforaddress2)
- [getConfirmedTransaction](jsonrpc-api.md#getconfirmedtransaction)

## Форматирование запроса

Чтобы сделать запрос JSON-RPC отправьте запрос POST HTTP с заголовком `Content-Type: application/json`. Данные запроса JSON должны содержать 4 поля:

- `jsonrpc: <string>`, установите `"2.0"`
- `id: <number>`, уникальное целое число генерируемое клиентом
- `метод: <string>`, строка, содержащая метод для вызова
- `параметров: <array>`, массив JSON упорядоченных значений параметров

Пример использования curl:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"
    ]
  }
'
```

Результатом ответа будет объект JSON со следующими полями:

- `jsonrpc: <string>`, соответствующие спецификации запроса
- `id: <number>`, соответствующие идентификатору запроса
- `результат: <array|number|object|string>`, затребованные данные или подтверждение успеха

Запросы могут быть отправлены в пакетных пакетах, посылая массив объектов JSON-RPC в качестве данных для одного POST.

## Определения

- Hash: A SHA-256чанка данных.
- Pubkey: Открытый ключ пары Ed25519.
- Transaction: Перечень Solana инструкций, подписанных парой ключей для авторизации этих действий.
- Signature: An Ed25519 подпись данных транзакции с инструкциями. Это может быть использовано для идентификации транзакций.

## Настройка комманды состояния

Для предварительных проверок и обработки транзакций узлы Solana выбирают состояние банка для запроса на основе требования к обязательству, установленного клиентом. Обязательство описывает, насколько завершен блок на данный момент. При запросе состояния реестра рекомендуется использовать более низкие уровни обязательств для отчета о прогрессе и более высокие уровни, чтобы гарантировать, что состояние не будет возвращено.

В порядке убывания обязательств (от наиболее завершенных до наименее завершенных) клиенты могут указать:

- `"finalized"` - the node will query the most recent block confirmed by supermajority of the cluster as having reached maximum lockout, meaning the cluster has recognized this block as finalized
- `"confirmed"` - the node will query the most recent block that has been voted on by supermajority of the cluster.
  - Он включает голоса gossip и повторов.
  - Он не учитывает голоса потомков блока, а только прямые голоса в этом блоке.
  - Этот уровень подтверждения также поддерживает гарантии «оптимистичного подтверждения» в версии 1.3 и новее.
- `"processed"` - the node will query its most recent block. Обратите внимание, что блок может быть незавершен.

For processing many dependent transactions in series, it's recommended to use `"confirmed"` commitment, which balances speed with rollback safety. For total safety, it's recommended to use`"finalized"` commitment.

#### Примеры

Параметр фиксации должен быть включен в качестве последнего элемента в массив `params`:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBalance",
    "params": [
      "83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri",
      {
        "commitment": "finalized"
      }
    ]
  }
'
```

#### По умолчанию:

If commitment configuration is not provided, the node will default to `"finalized"` commitment

Только методы, которые запрашивают состояние банка принимают параметр обязательства. Они указаны ниже в описании API.

#### Структура RpcResponse

Многие методы, которые принимают параметр обязательства возвращают объект RpcResponse JSON, состоящий из двух частей:

- `context` : Структура JSON RpcResponseContext с `слотом` в котором была вычислена операция.
- `value`: значение, возвращаемое самой операцией.

## Проверка состояния

Хотя это и не JSON RPC API, `GET / health` в конечной точке RPC HTTP предоставляет механизм проверки работоспособности для использования балансировщиками нагрузки или другой сетевой инфраструктурой. This request will always return a HTTP 200 OK response with a body of "ok", "behind" or "unknown" based on the following conditions:

1. If one or more `--trusted-validator` arguments are provided to `solana-validator`, "ok" is returned when the node has within `HEALTH_CHECK_SLOT_DISTANCE` slots of the highest trusted validator, otherwise "behind". "unknown" is returned when no slot information from trusted validators is not yet available.
2. "Ok" всегда возвращается, если нет доверенных валидаторов.

## Справочник по JSON RPC API

### getAccountInfo

Возвращает всю информацию, связанную с учетной записью Pubkey

#### Параметры:

- `& lt; string & gt;` - Pubkey аккаунта для запроса в виде строки в кодировке Base-58
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - `кодировка: <string>` - кодировка для аккаунта, либо "base58" (_slow_), "base64", "base64+zstd", или "jsonParsed". "base58" ограничивается данными счета менее 129 байт. "base64" возвращает данные base64 для данных аккаунта любого размера. "base64+zstd" сжимает данные аккаунта с помощью [Zstandard](https://facebook.github.io/zstd/) и base64-кодирует результат. "jsonParsed" кодировка пытается использовать парсеры специфичных для программы состояний для возврата более читаемых и явных данных о состоянии аккаунта. Если запрошен "jsonParsed", но парсер не найден, поле возвращается в кодировку "base64", обнаруживается, когда поле `данных` типа `<string>`.
  - (опционное) `dataSlice: <object>` - ограничить возвращаемые данные аккаунта, используя предоставленные `смещение: <usize>` и `длина: <usize>` поля; доступно только для кодировок "base58", "base64" или "base64+zstd".

#### Результаты:

Результатом будет объект JSON RpcResponse со значением `value`, равным:

- `<null>` - если запрашиваемого аккаунта не существует
- `<object>` - в противном случае объект JSON, содержащий:
  - `лампы: <u64>`, количество лємпортов, назначенных этому аккаунту, в качестве u64
  - `owner: <string>`,, Pubkey в кодировке base-58 программы, которой назначена этому аккаунту
  - `data: <[string, encoding]|object>`,, данные, связанные с аккаунтом, либо в виде закодированных двоичных данных, либо в формате JSON `{<program>: <state>}`, в зависимости от параметра кодировки
  - `executable: <bool>`, логическое значение, указывающее, содержит ли учетная запись программу \ (и предназначена ли она только для чтения \)
  - `rentEpoch: <u64>`, эпоха, в которой этот аккаунт будет сдан в аренду, как u64

#### Примеры:

Запрос:

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

Ответ:

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

#### Примеры:

Запрос:

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

Ответ:

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

### getBalance

Возвращает баланс аккаунта предоставленного Pubkey

#### Параметры:

- `<string>` - Pubkey аккаунта для запроса в виде строки в кодировке base-58
- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

- `RpcResponse<u64>`- объект JSON RpcResponse с полем `value`, установленным на баланс

#### Пример:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": { "context": { "slot": 1 }, "value": 0 },
  "id": 1
}
```

### getBlock

Возвращает идентификационные данные и информацию транзакции о подтвержденном блоке в реестре

#### Параметры:

- `<u64>` - слот, как u64 целое число
- `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". Если параметр не указан, кодировка по умолчанию "json". Кодировка "jsonParsed" пытается использовать парсеры инструкций специфичных для программы для возврата более читаемых и явных данных в списке `transaction.message.instructions`. Если "jsonParsed" запрошен, но синтаксический анализатор не может быть найден, инструкция возвращается к обычной кодировке JSON (поля `accounts`, `data` и `programIdIndex`).
  - (optional) `transactionDetails: <string>` - level of transaction detail to return, either "full", "signatures", or "none". If parameter not provided, the default detail level is "full".
  - (optional) `rewards: bool` - whether to populate the `rewards` array. If parameter not provided, the default includes rewards.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Результаты:

Поле результата будет объектом со следующими полями:

- `<null>` - если указанный блок не подтвержден
- `<object>` - если блок подтвержден, объект со следующими полями:
  - `blockhash: <string>` - хэш блока, как строка с кодировкой 58
  - `previousBlockhash: & lt; string & gt;` - хеш-код родительского блока в виде строки в кодировке base-58; если родительский блок недоступен из-за очистки реестра, в этом поле будет возвращено «11111111111111111111111111111111»
  - `parentSlot: <u64>` - индекс ячейки родителя этого блока
  - `transactions: <array>` - present if "full" transaction details are requested; an array of JSON objects containing:
    - `transaction: <object|[string,encoding]>` - объект [ Transaction ](#transaction-structure), либо в формате JSON, либо в закодированных двоичных данных, в зависимости от параметр кодирования
    - `meta: <object>`- объект метаданных статуса транзакции, содержащий `null` или:
      - `err: <object | null>` - Ошибка при сбое транзакции, null если транзакция успешна. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>`- комиссия за транзакцию была списана в виде целого числа u64
      - `preBalances: <array>`- массив остатков на счете u64 до обработки транзакции
      - `postBalances: <array>` - массив остатков на счете u64 после обработки транзакции
      - `innerInstructions: <array|undefined>` - список [ внутренних инструкций ](#inner-instructions-structure) или опускается, если внутренняя запись инструкций еще не была включена во время этой транзакции
      - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `logMessages: <array>` - массив строковых сообщений журнала или опускается, если запись сообщения журнала еще не была включена во время этой транзакции
      - УСТАРЕЛО:`status: <object>` - статус транзакции
        - `"Ок": <null>` - Транзакция прошла успешно
        - `"Err": <ERR>` - Транзакция не удалась с ошибкой TransactionError
  - `signatures: <array>` - present if "signatures" are requested for transaction details; an array of signatures strings, corresponding to the transaction order in the block
  - `rewards: <array>` - present if rewards are requested; an array of JSON objects containing:
    - `pubkey: <string>`- открытый ключ в виде строки в кодировке Base-58 учетной записи, получившей вознаграждение
    - `lamports: <i64>` - количество лампортов вознаграждения, начисленных или списанных с учетной записи, как i64
    - `postBalance: <u64>` - баланс счета в лэмпортах после применения вознаграждения
    - `rewardType: <string|undefined>`- тип вознаграждения: «комиссия», «аренда», «голосование», «ставка»
  - `blockTime: <i64 | null>` - расчетное время производства в виде отметки времени Unix (секунды с начала эпохи Unix). null если не доступен

#### Пример:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlock","params":[430, {"encoding": "json","transactionDetails":"full","rewards":false}]}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlock","params":[430, "base64"]}
'
```

Результат:

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
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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

#### Структура транзакции

Транзакции значительно отличаются от транзакций других блокчейнов. Обязательно ознакомьтесь с [ анатомией транзакции ](developing/programming-model/transactions.md), чтобы узнать о транзакциях в Solana.

Структура JSON транзакции определяется следующим образом:

- `signatures: <array[string]>`- список подписей в кодировке base-58, примененных к транзакции. Длина списка всегда от `message.header.numRequiredSignatures` и не является пустой. Подпись на индексе `i` соответствует публичному ключу по индексу `i` в `message.account_keys`. Первый используется как [ id транзакции ](../../terminology.md#transaction-id).
- `message: <object>` - определяет содержание транзакции.
  - `accountKeys: <array[string]>`- список открытых ключей в кодировке base-58, используемых транзакцией, в том числе в инструкциях и для подписей. Первые публичные ключи `message.header.numRequiredSignatures` должны подписывать транзакцию.
  - `header: <object>` - подробные сведения о типах учетных записей и подписях, необходимых для транзакции.
    - `numRequiredSignatures: <number>` - общее количество подписей, необходимых для подтверждения транзакции. Подписи должны соответствовать первому `numRequiredSignatures` в `message.account_keys`.
    - `numReadonlySignedAccounts: <number>` - последние `numReadonlySignedAccounts` из подписанных ключей являются аккаунтами только для чтения. Программы могут обрабатывать несколько транзакций, которые загружают аккаунты только для чтения в одной записи PoH, но им не разрешается кредитовать или дебетовать лэмпорты или изменять данные аккаунта. Транзакции, ориентированные на одни и те же аккаунты для чтения и записи, оцениваются последовательно.
    - `numReadonlyUnsignedAccounts: <number>`- последние `numReadonlyUnsignedAccounts` из неподписанных ключей являются аккаунтами только для чтения.
  - `recentBlockhash: <string>` - хеш-код последнего блока в реестре в кодировке Base-58, используемый для предотвращения дублирования транзакций и определения времени жизни транзакций.
  - `instructions: <array[object]>` - список программных инструкций, которые будут выполняться последовательно и фиксироваться в одной атомарной транзакции, если все они выполнены успешно.
    - `programIdIndex: <number>` - индексирует массив `message.accountKeys`, указывающий аккаунт программы, которая выполняет эту инструкцию.
    - `accounts: <array[number]>` - список упорядоченных индексов в массиве `message.accountKeys`, указывающих, какие учетные записи передать программе.
    - `data: <string>` - входные данные программы, закодированные в строке base-58.

#### Структура внутренних инструкций

Среда выполнения Solana записывает межпрограммные инструкции, которые вызываются во время обработки транзакции, и делает их доступными для большей прозрачности того, что было выполнено в цепочке для каждой инструкции транзакции. Вызванные инструкции группируются по инструкции исходящей транзакции и перечисляются в порядке обработки.

Структура внутренних инструкций JSON определяется как список объектов в следующей структуре:

- `index: number` - индекс инструкции транзакции, из которой произошли внутренние инструкции
- `instructions: <array[object]>` - упорядоченный список внутренних программных инструкций, которые были вызваны во время одной инструкции транзакции.
  - `programIdIndex: <number>` - индексирует массив `message.accountKeys`, указывающий аккаунт программы, которая выполняет эту инструкцию.
  - `accounts: <array[number]>` - список упорядоченных индексов в массиве `message.accountKeys`, указывающих, какие учетные записи передать программе.
  - `data: <string>` - входные данные программы, закодированные в строке base-58.

#### Token Balances Structure

The JSON structure of token balances is defined as a list of objects in the following structure:

- `accountIndex: <number>` - Index of the account in which the token balance is provided for.
- `mint: <string>` - Pubkey of the token's mint.
- `uiTokenAmount: <object>` -
  - `amount: <string>` - Raw amount of tokens as a string, ignoring decimals.
  - `decimals: <number>` - Number of decimals configured for token's mint.
  - `uiAmount: <number | null>` - Token amount as a float, accounting for decimals. **DEPRECATED**
  - `uiAmountString: <string>` - Token amount as a string, accounting for decimals.

### getBlockProduction

Returns recent block production information from the current or previous epoch.

#### Параметры:

- `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `range: <object>` - Slot range to return block production for. Если параметр не указан, по умолчанию используется текущая эпоха.
    - `firstSlot: <u64>` - first slot to return block production information for (inclusive)
    - (optional) `lastSlot: <u64>` - last slot to return block production information for (inclusive). If parameter not provided, defaults to the highest slot
  - (optional) `identity: <string>` - Only return results for this validator identity (base-58 encoded)

#### Результаты:

Результатом будет объект JSON RpcResponse со значением `value`, равным:

- `<object>`
  - `byIdentity: <object>` - a dictionary of validator identities, as base-58 encoded strings. Value is a two element array containing the number of leader slots and the number of blocks produced.
  - `range: <object>` - Block production slot range
    - `firstSlot: <u64>` - first slot of the block production information (inclusive)
    - `lastSlot: <u64>` - last slot of block production information (inclusive)

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockProduction"}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 9887
    },
    "value": {
      "byIdentity": {
        "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr": [9888, 9886]
      },
      "range": {
        "firstSlot": 0,
        "lastSlot": 9887
      }
    }
  },
  "id": 1
}
```

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getBlockProduction",
    "params": [
      {
        "identity": "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr",
        "range": {
          "firstSlot": 40,
          "lastSlot": 50
        }
      }
    ]
  }
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 10102
    },
    "value": {
      "byIdentity": {
        "85iYT5RuzRTDgjyRa3cP8SYhM2j21fj7NhfJ3peu1DPr": [11, 11]
      },
      "range": {
        "firstSlot": 50,
        "lastSlot": 40
      }
    }
  },
  "id": 1
}
```

### getBlockCommitment

Возвращает обязательства для конкретного блока

#### Параметры:

- `<u64>` - block, identified by Slot

#### Результаты:

Поле результата будет объектом JSON, содержащим:

- `commitment` - приверженность, включая либо:
  - `<null>` - Неизвестный блок
  - `<array>`- обязательство, массив целых чисел u64, регистрирующий количество ставок кластера в лємпортах, проголосовавших за блок на каждой глубине от 0 до `MAX_LOCKOUT_HISTORY` + 1
- `всего разбито` - общая активнеая ставка в лємпорт, текущей эпохи

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

Результат:

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

### getBlocks

Возвращает список подтвержденных блоков между двумя слотами

#### Параметры:

- `<u64>` - start_slot,, как целое число u64
- `<u64>` - (опционально) end_slot, как u64 целое число
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Результаты:

Поле результата будет представлять собой массив целых чисел u64, в котором перечислены подтвержденные блоки между `start_slot` и либо `end_slot`, если он предоставлен, либо последним подтвержденным блоком, включительно. Максимальный допустимый диапазон - 500,000 слотов.

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlocks","params":[5, 10]}
'
```

Результат:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7, 8, 9, 10], "id": 1 }
```

### getBlocksWithLimit

Возвращает список подтвержденных блоков, начиная с указанного слота

#### Параметры:

- `<u64>` - start_slot, как целое число u64
- `<u64>` - limit, как u64 целое число
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Результаты:

Поле результата будет представлять собой массив целых чисел u64, в котором перечислены подтвержденные блоки, начинающиеся с `start_slot`, до `limit` блоков включительно.

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getBlocksWithLimit","params":[5, 3]}
'
```

Результат:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### getBlockTime

Returns the estimated production time of a block.

Каждый валидатор сообщает свое время в формате UTC в регистр через регулярный интервал, периодически добавляя временную метку к голосованию для определенного блока. Время запрошенного блока рассчитывается на основе взвешенного по ставке среднего значения временных меток голосования в наборе последних блоков, записанных в реестре.

#### Параметры:

- `<u64>` - block, identified by Slot

#### Результаты:

- `& lt; i64 & gt;` - расчетное время производства в виде отметки времени Unix (секунды с начала эпохи Unix)
- `<null>` - временная метка недоступна для этого блока

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockTime","params":[5]}
'
```

Результат:

```json
{ "jsonrpc": "2.0", "result": 1574721591, "id": 1 }
```

### getClusterNodes

Возвращает информацию обо всех узлах, участвующих в кластере

#### Параметры:

None

#### Результаты:

Поле результата будет массивом объектов JSON, каждый со следующими подполями:

- `pubkey: <string>` - публичный ключ узла, как строка в кодировке base-58
- `gossip: <string>` - Gossip сетевой адрес для узла
- `tpu: <string>` - сетевой адрес TPU для узла
- `rpc: <string>|null` - JSON RPC адрес для узла, или `null` если служба JSON RPC не включена
- `version: <string>|null` - версия программного обеспечения узла или `null`, если информация о версии недоступна

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getClusterNodes"}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "gossip": "10.239.6.48:8001",
      "pubkey": "9QzsJf7LPLj8GkXbYT3LFDKqsj2hHG7TA3xinJHu8epQ",
      "rpc": "10.239.6.48:8899",
      "tpu": "10.239.6.48:8856",
      "version": "1.0.0 c375ce1f"
    }
  ],
  "id": 1
}
```

### getEpochInfo

Возвращает информацию о текущей эпохе

#### Параметры:

- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результаты:

Поле результата будет объектом со следующими полями:

- `absoluteSlot: <u64>`, текущий слот
- `blockВысота: <u64>`, текущая высота блока
- `epoch: <u64>`, текущая эпоха
- `slotIndex: <u64>`, текущий слот относительно начала текущей эпохи
- `slotsInEpoch: <u64>`, количество слотов в эту эпоху

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}
'
```

Результат:

```json
{
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

### getEpochSchedule

Возвращает информацию о расписании эпох из конфигурации генезиса этого кластера

#### Параметры:

None

#### Результаты:

Поле результата будет объектом со следующими полями:

- `slotsPerEpoch: <u64>`, максимальное количество слотов в каждой эпохе
- `leaderScheduleSlotOffset: <u64>`, количество слотов перед началом эпохи для расчета расписания лидера для этой эпохи
- `warmup: <bool>`, начинается с более коротких эпох, которые растут в дальнейшем
- `firstNormalEpoch: <u64>`, первая эпоха нормальной длины, log2(slotsPerEpoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
- `firstNormalSlot: <u64>`, MINIMUM_SLOTS_PER_EPOCH \* (2.pow(firstNormalEpoch) - 1)

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getEpochSchedule"}
'
```

Результат:

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

### getFeeCalculatorForBlockhash

Возвращает калькулятор комиссии, связанный с хэшем блока запроса, или `null`, если срок действия хэша блока истек

#### Параметры:

- `<string>` - запросить хэш блока как строку в кодировке Base58
- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результаты:

Результатом будет объект JSON RpcResponse со значением `value`, равным:

- `<null>` - если срок действия блок хэша запроса истек
- `<object>` - в противном случае объект JSON, содержащий:
  - `feeCalculator: <object>`, `FeeCalculator ` объект, описывающий ставку комиссии кластера при запрошенном хеш-коде блоков

#### Примеры:

Запрос:

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

Результат:

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

### getFeeRateGovernor

Возвращает информацию о регуляторе ставки вознаграждения из корневого банка

#### Параметры:

None

#### Результаты:

Поле `result` будет `объектом` со следующими полями:

- `burnPercent: <u8>`, процент собранных сборов, подлежащих уничтожению
- `maxLamportsPerSignature: <u64>`, максимальное значение, которое `lamportsPerSignature` может получить для следующего слота
- ` minLamportsPerSignature: <u64>`, наименьшее значение, которое `lamportsPerSignature` может получить для следующего слота
- ` targetLamportsPerSignature:<u64>`, желаемая ставка комиссии для кластера
- `targetSignaturesPerSlot: <u64>` желаемая скорость подписи для кластера

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFeeRateGovernor"}
'
```

Результат:

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

### getFees

Возвращает хэш последнего блока из реестра, график комиссий, который можно использовать для вычислить стоимость отправки транзакции с ее использованием, и последний слот в какой блок-хеш будет действителен.

#### Параметры:

- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результаты:

Результатом будет объект JSON RpcResponse со значением `value`, установленным на объект JSON со следующими полями:

- `blockhash: <string>` - хэш в виде строки в кодировке base-58
- `feeCalculator: <object>` - объект FeeCalculator, тарифный план для этого хэша блока
- `lastValidSlot: <u64>` - DEPRECATED - this value is inaccurate and should not be relied upon

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFees"}
'
```

Результат:

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

### getFirstAvailableBlock

Возвращает слот самого низкого подтвержденного блока, который не был удален из реестра

#### Параметры:

None

#### Результаты:

- `<u64>` - Слот

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFirstAvailableBlock"}
'
```

Результат:

```json
{ "jsonrpc": "2.0", "result": 250000, "id": 1 }
```

### getGenesisHash

Возвращает хэш генеза

#### Параметры:

None

#### Результаты:

- `<string>` - хэш в виде строки в кодировке base-58

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getGenesisHash"}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": "GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC",
  "id": 1
}
```

### getHealth

Возвращает текущее состояние узла.

Если один или несколько аргументов `--trusted-validator` предоставлены для `solana-validator`, возвращается "ok", когда узел находится внутри слота `HEALTH_CHECK_SLOT_DISTANCE` самого надежного валидатора, в противном случае возвращается ошибка. "Ok" всегда возвращается, если нет доверенных валидаторов.

#### Параметры:

None

#### Результаты:

Если узел исправен: "ок" Если узел неисправен, возвращается ответ об ошибке JSON RPC. Особенности ответа об ошибке: **UNSTABLE** и могут измениться в будущем

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getHealth"}
'
```

Хороший результат:

```json
{ "jsonrpc": "2.0", "result": "ok", "id": 1 }
```

Нехороший результат (общий):

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

Нехороший результат (если есть дополнительная информация)

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

### getIdentity

Возвращает идентификатор pubkey для текущего узла

#### Параметры:

None

#### Результаты:

Поле результата будет объектом JSON со следующими полями:

- `identity`, идентификатор pubkey текущего узла \ (в виде строки в кодировке base-58 \)

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getIdentity"}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": { "identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4WuW2DJBppN" },
  "id": 1
}
```

### getInflationGovernor

Возвращает текущего регулятора инфляции

#### Параметры:

- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результаты:

Поле результата будет объектом JSON со следующими полями:

- `initial: <f64>`,начальный процент инфляции с момента 0
- `terminal: <f64>`, предельная инфляция в процентах
- `taper: <f64>`,годовая скорость снижения инфляции. Rate reduction is derived using the target slot time in genesis config
- `foundation: <f64>`процент от общей инфляции, выделенный на фонд
- `foundationTerm: <f64>`,продолжительность инфляции фонда в годах

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationGovernor"}
'
```

Результат:

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

### getInflationRate

Возвращает конкретные значения инфляции для текущей эпохи

#### Параметры:

None

#### Результаты:

Поле результата будет объектом JSON со следующими полями:

- `total: <f64>`, общая инфляция
- `validator: <f64>`, инфляция, назначенная валидаторам
- `foundation: <f64>`, инфляция, назначенная фондом
- `epoch: <f64>`, эпоха, для которой эти значения действительны

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationRate"}
'
```

Результат:

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

### getInflationReward

Returns the inflation reward for a list of addresses for an epoch

#### Параметры:

- `<array>` - An array of addresses to query, as base-58 encoded strings

* `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `epoch: <u64>` - An epoch for which the reward occurs. If omitted, the previous epoch will be used

#### Результаты

The result field will be a JSON array with the following fields:

- `epoch: <u64>`, epoch for which reward occured
- `effectiveSlot: <u64>`, the slot in which the rewards are effective
- `amount: <u64>`, reward amount in lamports
- `postBalance: <u64>`, post balance of the account in lamports

#### Образец

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getInflationReward",
    "params": [
       ["6dmNQ5jwLeLk5REvio1JcMshcbvkYMwy26sJ8pbkvStu", "BGsqMegLpV6n6Ve146sSX2dTjUMj3M92HnU8BbNRMhF2"], 2
    ]
  }
'
```

Ответ:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "amount": 2500,
      "effectiveSlot": 224,
      "epoch": 2,
      "postBalance": 499999442500
    },
    null
  ],
  "id": 1
}
```

### getLargestAccounts

Returns the 20 largest accounts, by lamport balance (results may be cached up to two hours)

#### Параметры:

- `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - (опционально) `filter: <string>` - фильтровать результаты по типу аккаунта; в настоящее время поддерживается: `circulating|nonCirculating`

#### Результаты:

Результатом будет объект JSON RpcResponse со значением `value`, равным массиву:

- `<object>` - в противном случае объект JSON, содержащий:
  - `address: <string>`, адрес аккаунта в кодировке Base-58
  - `lamports: <u64>`, количество лємпортов, назначенных этому аккаунту, в качестве u64

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLargestAccounts"}
'
```

Результат:

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

### getLeaderSchedule

Возвращает расписание лидеров для эпохи

#### Параметры:

- `<u64>` - (optional) получает расписание лидеров для эпохи, которая соответствует указанному слоту. Если не указано, то будет получено расписание лидеров текущей эпохи
- `<object>` - (необязательно) объект конфигурации, содержащий следующее поле:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `identity: <string>` - Only return results for this validator identity (base-58 encoded)

#### Результаты:

- `<null>` - если запрашиваемая эпоха не была найдена
- `<object>` - otherwise, the result field will be a dictionary of validator identities, as base-58 encoded strings, and their corresponding leader slot indices as values (indices are relative to the first slot in the requested epoch)

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F": [
      0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
      21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
      39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
      57, 58, 59, 60, 61, 62, 63
    ]
  },
  "id": 1
}
```

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getLeaderSchedule",
    "params": [
      null,
      {
        "identity": "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F"
      }
    ]
  }
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F": [
      0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
      21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
      39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
      57, 58, 59, 60, 61, 62, 63
    ]
  },
  "id": 1
}
```

### getMaxRetransmitSlot

Get the max slot seen from retransmit stage.

#### Результаты:

- `<u64>` - Слот

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getMaxRetransmitSlot"}
'
```

Результат:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getMaxShredInsertSlot

Get the max slot seen from after shred insert.

#### Результат:

- `<u64>` - Слот

#### Пример:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getMaxShredInsertSlot"}
'
```

Результат:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getMinimumBalanceForRentExemption

Возвращает минимальный баланс, необходимый для освобождения аккаунта от арендной платы.

#### Параметры:

- `<usize>` - длина данных аккаунта
- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

- `<u64>` - минимум лэмпортов, необходимых в аккаунте

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getMinimumBalanceForRentExemption", "params":[50]}
'
```

Результат:

```json
{ "jsonrpc": "2.0", "result": 500, "id": 1 }
```

### getMultipleAccounts

Возвращает информацию об аккаунте для списка Pubkeys

#### Параметры:

- `<array>` - массив ключей Pubkeys для запроса в виде строк в кодировке base-58
- `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - `кодировка: <string>` - кодировка для аккаунта, либо "base58" (_slow_), "base64", "base64+zstd", или "jsonParsed". "base58" is limited to Account data of less than 129 bytes. "base64" возвращает данные base64 для данных аккаунта любого размера. "base64+zstd" сжимает данные аккаунта с помощью [Zstandard](https://facebook.github.io/zstd/) и base64-кодирует результат. "jsonParsed" кодировка пытается использовать парсеры специфичных для программы состояний для возврата более читаемых и явных данных о состоянии аккаунта. Если запрошен "jsonParsed", но парсер не найден, поле возвращается в кодировку "base64", обнаруживается, когда поле `данных` типа `<string>`.
  - (опционное) `dataSlice: <object>` - ограничить возвращаемые данные аккаунта, используя предоставленные `смещение: <usize>` и `длина: <usize>` поля; доступно только для кодировок "base58", "base64" или "base64+zstd".

#### Результаты:

Результатом будет объект JSON RpcResponse со значением `value`, равным:

Массив из:

- `<null>` - если аккаунт в этом Pubkey не существует
- `<object>` - в противном случае объект JSON, содержащий:
  - `лампы: <u64>`, количество лємпортов, назначенных этому аккаунту, в качестве u64
  - `owner: <string>`,, Pubkey в кодировке base-58 программы, которой назначена этому аккаунту
  - `data: <[string, encoding]|object>`,, данные, связанные с аккаунтом, либо в виде закодированных двоичных данных, либо в формате JSON `{<program>: <state>}`, в зависимости от параметра кодировки
  - `executable: <bool>`, логическое значение, указывающее, содержит ли учетная запись программу \ (и предназначена ли она только для чтения \)
  - `rentEpoch: <u64>`, эпоха, в которой этот аккаунт будет сдан в аренду, как u64

#### Примеры:

Запрос:

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
```

Результат:

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

#### Примеры:

Запрос:

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

Результат:

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

### getProgramAccounts

Возвращает все аккаунты, принадлежащие указанной программе Pubkey

#### Параметры:

- `<string>` - Pubkey аккаунта для запроса в виде строки в кодировке base-58
- `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - `кодировка: <string>` - кодировка для аккаунта, либо "base58" (_slow_), "base64", "base64+zstd", или "jsonParsed". "base58" is limited to Account data of less than 129 bytes. "base64" возвращает данные base64 для данных аккаунта любого размера. "base64+zstd" сжимает данные аккаунта с помощью [Zstandard](https://facebook.github.io/zstd/) и base64-кодирует результат. "jsonParsed" кодировка пытается использовать парсеры специфичных для программы состояний для возврата более читаемых и явных данных о состоянии аккаунта. Если запрошен "jsonParsed", но парсер не найден, поле возвращается в кодировку "base64", обнаруживается, когда поле `данных` типа `<string>`.
  - (опционное) `dataSlice: <object>` - ограничить возвращаемые данные аккаунта, используя предоставленные `смещение: <usize>` и `длина: <usize>` поля; доступно только для кодировок "base58", "base64" или "base64+zstd".
  - (опционное) `filters: <array>` - фильтровать результаты, используя различные [filter objects](jsonrpc-api.md#filters); аккаунт должен соответствовать всем критериям фильтрации для включения в результаты

##### Фильтры:

- `memcmp: <object>` - сравнивает введенную серию байт с данными аккаунта программы по конкретному смещению. Поля:

  - `offset: <usize>` - смещение в данных учетной записи программы для начала сравнения
  - `bytes: <string>` - data to match, as base-58 encoded string and limited to less than 129 bytes

- `dataSize: <u64>` - сравнивает длину данных учетной записи программы с предоставленным размером данных

#### Результаты:

Полем результата будет массив JSON-объектов, который будет содержать:

- `pubkey: <string>` - Pubkey аккаунта в виде строки в кодировке base-58
- `account: <object>` - объект JSON со следующими подполями:
  - `лампы: <u64>`, количество лємпортов, назначенных этому аккаунту, в качестве u64
  - `owner: <string>`, Pubkey в кодировке base-58 программы, которой назначен аккаунт `data: <[string,encoding]|object>` данные, связанные с аккаунтом, либо в виде закодированных двоичных данных, либо в формате JSON `{<program>: <state>}`, в зависимости от параметра кодировки
  - `executable: <bool>`, логическое значение, указывающее, содержит ли учетная запись программу \ (и предназначена ли она только для чтения \)
  - `rentEpoch: <u64>`, эпоха, в которой этот аккаунт будет сдан в аренду, как u64

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getProgramAccounts", "params":["4Nd1mBQtrMJVYVfKf2PJy9NZUZdTAsp7D4xWLs4gDB4T"]}
'
```

Результат:

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

#### Пример:

Запрос:

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

Результат:

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

### getRecentBlockhash

Возвращает хэш последнего блока из реестра и график комиссий, который можно использовать для вычисления стоимости отправки транзакции с его использованием.

#### Параметры:

- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

RpcResponse, содержащий объект JSON, состоящий из строкового хэша блока и объекта FeeCalculator JSON.

- `RpcResponse<object>` - объект JSON RpcResponse с полем `value`, установленным на объект JSON, включая:
- `blockhash: <string>` - хэш в виде строки в кодировке base-58
- `feeCalculator: <object>` - объект FeeCalculator, тарифный план для этого хэша блока

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getRecentBlockhash"}
'
```

Результат:

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
      }
    }
  },
  "id": 1
}
```

### getRecentPerformanceSamples

Возвращает список последних образцов производительности в обратном порядке слотов. Образцы производительности берутся каждые 60 секунд и включать количество транзакций и интервалов, которые происходят в данном временном окне.

#### Параметры:

- `limit: <usize>` - (необязательно) количество возвращаемых выборок (максимум 720)

#### Результаты:

Массив из:

- `RpcPerfSample< object>`
  - `slot: <u64>`- слот, в котором выборка была взята
  - `numTransactions: < u64>` - Количество транзакций в выборке
  - `numTransactions: < u64>` - Количество транзакций в выборке
  - `samplePeriodSecs: <u16>` - Количество секунд в окне выборок

#### Примеры:

Запрос:

```bash
// Request
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getRecentPerformanceSamples", "params": [4]}
'
```

Результат:

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

Возвращает самый высокий слот, для которого у узла есть снимок

#### Параметры:

None

#### Результат:

- `<u64>` - Слот снимка

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSnapshotSlot"}
'
```

Результат:

```json
{ "jsonrpc": "2.0", "result": 100, "id": 1 }
```

Результат, когда узел не имеет снимка:

```json
{
  "jsonrpc": "2.0",
  "error": { "code": -32008, "message": "No snapshot" },
  "id": 1
}
```

### getSignaturesForAddress

Возвращает подтвержденные подписи для транзакций, связанных с обратный адрес во времени от предоставленной подписи или самого последнего подтвержденного блока

#### Параметры:

- `<string>` - адрес аккаунта в виде строки в кодировке base-58
- `<object>` - (необязательно) объект конфигурации, содержащий следующие поля:
  - `limit: <number>` - (необязательно) максимальное количество возвращаемых подписей транзакций (от 1 до 1000, по умолчанию: 1000).
  - `before: <string>` - (необязательно) начать поиск в обратном направлении от этой подписи транзакции. Если не указано иное, поиск начинается с вершины самого высокого максимального подтвержденного блока.
  - `until: <string>` - (необязательно) поиск до этой подписи транзакции, если она была найдена до достижения лимита.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Результаты:

Поле результата будет представлять собой массив информации о подписи транзакции, упорядоченный от самой новой к самой старой транзакции:

- `<object>`
  - `signature: <string>` - подпись транзакции в виде строки в кодировке base-58
  - `slot: <u64>`- слот, содержащий блок с транзакцией
  - `err: <object | null>` - Ошибка при сбое транзакции, null если транзакция успешна. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - заметка, связанная с транзакцией, null, если заметки нет
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when transaction was processed. null if not available.

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [
      "Vote111111111111111111111111111111111111111",
      {
        "limit": 1
      }
    ]
  }
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114,
      "blockTime": null
    }
  ],
  "id": 1
}
```

### getSignatureStatuses

Возвращает статусы списка подписей. `searchTransactionHistory` параметр конфигурации включен, только этот метод выполняет поиск в кэше последних статусов подписей, который сохраняет статусы для всех активные слоты плюс `MAX_RECENT_BLOCKHASHES` корневые слоты.

#### Параметры:

- `<array>` - Массив подписей транзакций для подтверждения в виде строк в кодировке base-58
- `<object>` - (необязательно) объект конфигурации, содержащий следующее поле:
  - `searchTransactionHistory: <bool>` - если true, узел Solana будет искать в своем кэше реестра любые подписи, не найденные в кэше недавнего состояния

#### Результаты:

RpcResponse, содержащий объект JSON, состоящий из массива объектов TransactionStatus.

- `RpcResponse<object> ` - JSON-объект RpcResponse с полем `value`:

Массив из:

- `<null>` - Неизвестная транзакция
- `<object>`
  - `slot: <u64>` - Слот, в котором была обработана транзакция
  - `confirmations: <usize | null>`- Количество блоков с момента подтверждения подписи, null, если внедрено, а также завершено подавляющим большинством кластера
  - `err: <object | null>` - Ошибка при сбое транзакции, null если транзакция успешна. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `confirmStatus: <string | null>` - статус подтверждения транзакции в кластере; либо `обработано`, `подтверждено`, либо `завершено`. См. [ Обязательство ](jsonrpc-api.md#configuring-state-commitment) для получения дополнительной информации об оптимистическом подтверждении.
  - УСТАРЕЛО:`status: <object>` - статус транзакции
    - `"Ок": <null>` - Транзакция прошла успешно
    - `"Err": <ERR>` - Транзакция не удалась с ошибкой TransactionError

#### Примеры:

Запрос:

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

Результат:

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

#### Пример:

Запрос:

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

Результат:

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

### getSlot

Возвращает текущий слот, который обрабатывает узел

#### Параметры:

- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

- `<u64>` - Текущий слот

#### Пример:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

Результат:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### getSlotLeader

Возвращает текущий слота лидера

#### Параметры:

- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результаты:

- `pubkey: < string>` - Pubkey аккаунта в виде строки в кодировке base-58

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": "ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS",
  "id": 1
}
```

### getSlotLeaders

Returns the slot leaders for a given slot range

#### Параметры:

- `<u64>` - Start slot, as u64 integer
- `<u64>` - Limit, как u64 целое число

#### Результаты:

- `<array<string>>` - Node identity public keys as base-58 encoded strings

#### Примеры:

If the current slot is #99, query the next 10 leaders with the following request:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeaders", "params":[100, 10]}
'
```

Результат:

The first leader returned is the leader for slot #100:

```json
{
  "jsonrpc": "2.0",
  "result": [
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "ChorusmmK7i1AxXeiTtQgQZhQNiXYU84ULeaYF1EH15n",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "Awes4Tr6TX8JDzEhCZY2QVNimT6iD1zWHzf1vNyGvpLM",
    "DWvDTSh3qfn88UoQTEKRV2JnLt5jtJAVoiCo3ivtMwXP",
    "DWvDTSh3qfn88UoQTEKRV2JnLt5jtJAVoiCo3ivtMwXP"
  ],
  "id": 1
}
```

### getkeActivation

Возвращает информацию активации эпохи для аккаунта ставки

#### Параметры:

- `<string>` - Pubkey аккаунта ставки для запроса в виде строки в кодировке base-58
- `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - (необязательно) `эпоха: <u64>` - эпоха, для которой нужно рассчитать детали активации. Если параметр не указан, по умолчанию используется текущая эпоха.

#### Результаты:

Результатом будет объект JSON со следующими полями:

- ` state: <string` - состояние активации лицевого счета, одно из: `активный`, `неактивный`, `активация`, `деактивация `
- `active: <u64>` - ставка активна в эпоху
- `inactive: <u64>` - ставка неактивна в эпоху

#### Примеры:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getStakeActivation", "params": ["CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT"]}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": { "active": 197717120, "inactive": 0, "state": "active" },
  "id": 1
}
```

#### Пример:

Запрос:

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

Результат:

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

### getSupply

Возвращает информацию о текущем предложении.

#### Параметры:

- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Результатом будет объект JSON RpcResponse со значением `value`, равным объекту JSON, содержащему:

- `total: <u64>` - Общее количество в лэмпортах
- `Циркуляционный: <u64>` - Циркуляция в лэмпортах
- `nonCirculating: <u64>`Не в обращении в лэмпортах
- `nonCirculatingAccounts: <array>` - массив адресов аккаунтов необращающихся аккаунтов в виде строк

#### Пример:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getSupply"}
'
```

Результат:

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

### getTokenAccountBalance

Возвращает баланс токена аккаунта токена SPL.

#### Параметры:

- `<string>` - Pubkey аккаунта токена для запроса в виде строки в кодировке base-58
- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Результатом будет объект JSON RpcResponse со значением `value`, равным объекту JSON, содержащему:

- `amount: <string>` - исходный баланс без десятичных знаков, строковое представление u64
- `decimals: <u8>` - количество десятичных цифр справа от десятичного разряда
- `uiAmount: <number | null>` - the balance, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the balance as a string, using mint-prescribed decimals

#### Пример:

Запрос:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenAccountBalance", "params": ["7fUAJdStEuGbc3sM84cKRL6yYaaSstyLSU4ve5oovLS7"]}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "amount": "9864",
      "decimals": 2,
      "uiAmount": 98.64,
      "uiAmountString": "98.64"
    },
    "id": 1
  }
}
```

### getTokenAccountsByDelegate

Возвращает все аккаунты токенов SPL утвержденным делегатом.

#### Параметры:

- `<string>` - Pubkey аккаунта делегата для запроса в виде строки в кодировке base-58
- `<object>` - Либо:
  - `мин: < string>` - Pubkey конкретного токена Mint для ограничения аккаунтов в виде base-58; или
  - `programId: <string>` - Pubkey идентификатора Token программы, которой принадлежат аккаунту, в виде строки в кодировке base-58
- `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - кодировка данных Пользователя, либо "base58" (_ slow _), "base64", "base64 + zstd" или "jsonParsed". "jsonParsed" кодировка пытается использовать парсеры специфичных для программы состояний для возврата более читаемых и явных данных о состоянии аккаунта. Если запрошен «jsonParsed», но не может быть найден корректный mint для конкретного аккаунта, этот аккаунт будет отфильтрован по результатам.
  - (опционное) `dataSlice: <object>` - ограничить возвращаемые данные аккаунта, используя предоставленные `смещение: <usize>` и `длина: <usize>` поля; доступно только для кодировок "base58", "base64" или "base64+zstd".

#### Результат:

Результатом будет объект JSON RpcResponse со значением `value`, равным массиву объектов JSON, который будет содержать:

- `pubkey: <string>` - Pubkey аккаунта в виде строки в кодировке base-58
- `account: <object>` - объект JSON со следующими подполями:
  - `лампы: <u64>`, количество лємпортов, назначенных этому аккаунту, в качестве u64
  - `owner: <string>`,, Pubkey в кодировке base-58 программы, которой назначена этому аккаунту
  - `data: <object>`, данные о состоянии токена, связанных с аккаунтом, либо в виде закодированных двоичных данных, либо в формате JSON `{<program>: <state>}`
  - `executable: <bool>`, логическое значение, указывающее, содержит ли учетная запись программу \ (и предназначена ли она только для чтения \)
  - `rentEpoch: <u64>`, эпоха, в которой этот аккаунт будет сдан в аренду, как u64

#### Примеры:

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

Результат:

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
                "decimals": 1,
                "uiAmount": 0.1,
                "uiAmountString": "0.1"
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

### getTokenAccountsByOwner

Возвращает все аккаунты SPL Token по владельцу токена.

#### Параметры:

- `& lt; string & gt;` - Pubkey аккаунта для запроса в виде строки в кодировке base-58
- `<object>` - Либо:
  - `мин: < string>` - Pubkey конкретного токена Mint для ограничения аккаунтов в виде base-58; или
  - `programId: <string>` - Pubkey идентификатора Token программы, которой принадлежат аккаунту, в виде строки в кодировке base-58
- `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - кодировка данных Пользователя, либо "base58" (_ slow _), "base64", "base64 + zstd" или "jsonParsed". "jsonParsed" кодировка пытается использовать парсеры специфичных для программы состояний для возврата более читаемых и явных данных о состоянии аккаунта. Если запрошен «jsonParsed», но не может быть найден корректный mint для конкретного аккаунта, этот аккаунт будет отфильтрован по результатам.
  - (опционное) `dataSlice: <object>` - ограничить возвращаемые данные аккаунта, используя предоставленные `смещение: <usize>` и `длина: <usize>` поля; доступно только для кодировок "base58", "base64" или "base64+zstd".

#### Результат:

Результатом будет объект JSON RpcResponse со значением `value`, равным массиву объектов JSON, который будет содержать:

- `pubkey: <string>` - Pubkey аккаунта в виде строки в кодировке base-58
- `account: <object>` - объект JSON со следующими подполями:
  - `лампы: <u64>`, количество лємпортов, назначенных этому аккаунту, в качестве u64
  - `owner: <string>`,, Pubkey в кодировке base-58 программы, которой назначена этому аккаунту
  - `data: <object>`, данные о состоянии токена, связанных с аккаунтом, либо в виде закодированных двоичных данных, либо в формате JSON `{<program>: <state>}`
  - `executable: <bool>`, логическое значение, указывающее, содержит ли учетная запись программу \ (и предназначена ли она только для чтения \)
  - `rentEpoch: <u64>`, эпоха, в которой этот аккаунт будет сдан в аренду, как u64

#### Пример:

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

Результат:

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
                "decimals": 1,
                "uiAmount": 0.1,
                "uiAmountString": "0.1"
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

### getTokenLargestAccounts

Возвращает 20 самых больших аккаунтов определенного типа SPL Токена.

#### Параметры:

- `<string>` - Pubkey аккаунта токена для запроса в виде строки в кодировке base-58
- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Результатом будет объект JSON RpcResponse со значением `value`, равным объекту JSON, содержащему:

- `address: <string>` - адрес токена аккаунта
- `amount: <string>` - исходный баланс без десятичных знаков, строковое представление u64
- `decimals: <u8>` - количество десятичных цифр справа от десятичного разряда
- `uiAmount: <number | null>` - the token account balance, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the token account balance as a string, using mint-prescribed decimals

#### Пример:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenLargestAccounts", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Результат:

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
        "uiAmount": 7.71,
        "uiAmountString": "7.71"
      },
      {
        "address": "BnsywxTcaYeNUtzrPxQUvzAWxfzZe3ZLUJ4wMMuLESnu",
        "amount": "229",
        "decimals": 2,
        "uiAmount": 2.29,
        "uiAmountString": "2.29"
      }
    ]
  },
  "id": 1
}
```

### getTokenSupply

Возвращает общее количество токена SPL.

#### Параметры:

- `<string>` - Pubkey аккаунта токена для запроса в виде строки в кодировке base-58
- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Результатом будет объект JSON RpcResponse со значением `value`, равным объекту JSON, содержащему:

- `amount: <string>` - исходный баланс без десятичных знаков, строковое представление u64
- `decimals: <u8>` - количество десятичных цифр справа от десятичного разряда
- `uiAmount: <number | null>` - the total token supply, using mint-prescribed decimals **DEPRECATED**
- `uiAmountString: <string>` - the total token supply as a string, using mint-prescribed decimals

#### Пример:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getTokenSupply", "params": ["3wyAj7Rt1TWVPZVteFJPLa26JmLvdb1CAKEFZm3NY75E"]}
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "context": {
      "slot": 1114
    },
    "value": {
      "amount": "100000",
      "decimals": 2,
      "uiAmount": 1000,
      "uiAmountString": "1000"
    }
  },
  "id": 1
}
```

### getTransaction

Возвращает детали транзакции для подтвержденной транзакции

#### Параметры:

- `<string>` - подпись транзакции в виде строки в кодировке base-58
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Результат:

- `<null>` - if transaction is not found or not confirmed
- `<object>` - if transaction is confirmed, an object with the following fields:
  - `slot: <u64>` - the slot this transaction was processed in
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when the transaction was processed. null if not available
  - `meta: <object | null>` - transaction status metadata object:
    - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - fee this transaction was charged, as u64 integer
    - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
    - `postBalances: <array>` - array of u64 account balances after the transaction was processed
    - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
    - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
    - DEPRECATED: `status: <object>` - Transaction status
      - `"Ok": <null>` - Transaction was successful
      - `"Err": <ERR>` - Transaction failed with TransactionError

#### Пример:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "json"
    ]
  }
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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
  "blockTime": null,
  "id": 1
}
```

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "base64"
    ]
  }
'
```

Результат:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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

### getTransactionCount

Returns the current Transaction count from the ledger

#### Parameters:

- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `<u64>` - count

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}
'

```

Result:

```json
{ "jsonrpc": "2.0", "result": 268, "id": 1 }
```

### getVersion

Returns the current solana versions running on the node

#### Parameters:

None

#### Results:

The result field will be a JSON object with the following fields:

- `solana-core`, software version of solana-core
- `feature-set`, unique identifier of the current software's feature set

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVersion"}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": { "solana-core": "1.7.0" }, "id": 1 }
```

### getVoteAccounts

Returns the account info and associated stake for all the voting accounts in the current bank.

#### Parameters:

- `<object>` - (optional) Configuration object containing the following field:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - (optional) `votePubkey: <string>` - Only return results for this validator vote address (base-58 encoded)

#### Results:

The result field will be a JSON object of `current` and `delinquent` accounts, each containing an array of JSON objects with the following sub fields:

- `votePubkey: <string>` - Vote account address, as base-58 encoded string
- `nodePubkey: <string>` - Validator identity, as base-58 encoded string
- `activatedStake: <u64>` - the stake, in lamports, delegated to this vote account and active in this epoch
- `epochVoteAccount: <bool>` - bool, whether the vote account is staked for this epoch
- `commission: <number>`, percentage (0-100) of rewards payout owed to the vote account
- `lastVote: <u64>` - Most recent slot voted on by this vote account
- `epochCredits: <array>` - History of how many credits earned by the end of each epoch, as an array of arrays containing: `[epoch, credits, previousCredits]`

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}
'
```

Результат:

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

#### Example: Restrict results to a single validator vote account

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getVoteAccounts",
    "params": [
      {
        "votePubkey": "3ZT31jkAGhUaw8jsy4bTknwBMP8i4Eueh52By4zXcsVw"
      }
    ]
  }
'
```

Результат:

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
    "delinquent": []
  },
  "id": 1
}
```

### minimumLedgerSlot

Returns the lowest slot that the node has information about in its ledger. This value may increase over time if the node is configured to purge older ledger data

#### Parameters:

None

#### Results:

- `u64` - Minimum ledger slot

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

Результат:

```json
{ "jsonrpc": "2.0", "result": 1234, "id": 1 }
```

### requestAirdrop

Requests an airdrop of lamports to a Pubkey

#### Parameters:

- `<string>` - Pubkey of account to receive lamports, as base-58 encoded string
- `<integer>` - lamports, as a u64
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) (used for retrieving blockhash and verifying airdrop success)

#### Results:

- `<string>` - Transaction Signature of airdrop, as base-58 encoded string

#### Example:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 1000000000]}
'

```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW",
  "id": 1
}
```

### sendTransaction

Submits a signed transaction to the cluster for processing.

This method does not alter the transaction in any way; it relays the transaction created by clients to the node as-is.

If the node's rpc service receives the transaction, this method immediately succeeds, without waiting for any confirmations. A successful response from this method does not guarantee the transaction is processed or confirmed by the cluster.

While the rpc service will reasonably retry to submit it, the transaction could be rejected if transaction's `recent_blockhash` expires before it lands.

Use [`getSignatureStatuses`](jsonrpc-api.md#getsignaturestatuses) to ensure a transaction is processed and confirmed.

Before submitting, the following preflight checks are performed:

1. Подписи транзакций проверены
2. Транзакция моделируется относительно банковского слота, указанного в предначальном обязательстве. При ошибке будет возвращена ошибка. При желании предварительные проверки могут быть отключены. Рекомендуется указать те же обязательства и предначальных обязательств во избежание путаницы.

The returned signature is the first signature in the transaction, which is used to identify the transaction ([transaction id](../../terminology.md#transanction-id)). This identifier can be easily extracted from the transaction data before submission.

#### Parameters:

- `<string>` - fully-signed Transaction, as encoded string
- `<object>` - (optional) Configuration object containing the following field:
  - `skipPreflight: <bool>` - if true, skip the preflight transaction checks (default: false)
  - `preflightCommitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to use for preflight (default: `"finalized"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (_slow_, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

- `<string>` - First Transaction Signature embedded in the transaction, as base-58 encoded string ([transaction id](../../terminology.md#transanction-id))

#### Example:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": "2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwE5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb",
  "id": 1
}
```

### simulateTransaction

Simulate sending a transaction

#### Parameters:

- `<string>` - Transaction, as an encoded string. The transaction must have a valid blockhash, but is not required to be signed.
- `<object>` - (optional) Configuration object containing the following field:
  - `sigVerify: <bool>` - if true the transaction signatures will be verified (default: false)
  - `commitment: <string>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment) level to simulate the transaction at (default: `"finalized"`).
  - `encoding: <string>` - (optional) Encoding used for the transaction data. Either `"base58"` (_slow_, **DEPRECATED**), or `"base64"`. (default: `"base58"`).

#### Results:

An RpcResponse containing a TransactionStatus object The result will be an RpcResponse JSON object with `value` set to a JSON object with the following fields:

- `err: <object | string | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs: <array | null>` - Array of log messages the transaction instructions output during execution, null if simulation failed before the transaction was able to execute (for example due to an invalid blockhash or signature verification failure)

#### Example:

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

Result:

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

## Subscription Websocket

After connecting to the RPC PubSub websocket at `ws://<ADDRESS>/`:

- Submit subscription requests to the websocket using the methods below
- Multiple subscriptions may be active at once
- Many subscriptions take the optional [`commitment` parameter](jsonrpc-api.md#configuring-state-commitment), defining how finalized a change should be to trigger a notification. For subscriptions, if commitment is unspecified, the default value is `"finalized"`.

### accountSubscribe

Subscribe to an account to receive notifications when the lamports or data for a given account public key changes

#### Parameters:

- `<string>` - account Pubkey, as base-58 encoded string
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to binary encoding, detectable when the `data` field is type `<string>`.

#### Results:

- `<number>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "accountSubscribe",
  "params": [
    "CM78CPUeXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNH12",
    {
      "encoding": "base64",
      "commitment": "finalized"
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

Result:

```json
{ "jsonrpc": "2.0", "result": 23784, "id": 1 }
```

#### Notification Format:

Base58 encoding:

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

Parsed-JSON encoding:

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

### accountUnsubscribe

Unsubscribe from account change notifications

#### Parameters:

- `<number>` - id of account Subscription to cancel

#### Results:

- `< bool>` - сообщение об отмене подписки

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "accountUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### logsSubscribe

Subscribe to transaction logging

#### Parameters:

- `filter: <string>|<object>` - filter criteria for the logs to receive results by account type; currently supported:
  - "all" - subscribe to all transactions except for simple vote transactions
  - "allWithVotes" - subscribe to all transactions including simple vote transactions
  - `{ "mentions": [ <string> ] }` - subscribe to all transactions that mention the provided Pubkey (as base-58 encoded string)
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - (опционально) [Обязательство](jsonrpc-api. md#configuring-state-commitment)

#### Results:

- `< integer>` - Идентификатор Подписки \(необходим для отказа от подписки\)

#### Example:

Request:

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
      "commitment": "finalized"
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

Result:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Notification Format:

Base58 encoding:

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

### logsUnsubscribe

Unsubscribe from transaction logging

#### Parameters:

- `<integer>` - id of subscription to cancel

#### Results:

- `<bool>` - сообщение об отмене подписки

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "logsUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### programSubscribe

Subscribe to a program to receive notifications when the lamports or data for a given account owned by the program changes

#### Parameters:

- `<string>` - program_id Pubkey, as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>` - encoding for Account data, either "base58" (_slow_), "base64", "base64+zstd" or "jsonParsed". "jsonParsed" encoding attempts to use program-specific state parsers to return more human-readable and explicit account state data. If "jsonParsed" is requested but a parser cannot be found, the field falls back to base64 encoding, detectable when the `data` field is type `<string>`.
  - (optional) `filters: <array>` - filter results using various [filter objects](jsonrpc-api.md#filters); account must meet all filter criteria to be included in results

#### Results:

- `<integer>` - Subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "programSubscribe",
  "params": [
    "11111111111111111111111111111111",
    {
      "encoding": "base64",
      "commitment": "finalized"
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

Result:

```json
{ "jsonrpc": "2.0", "result": 24040, "id": 1 }
```

#### Notification Format:

Base58 encoding:

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

Parsed-JSON encoding:

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

### programUnsubscribe

Unsubscribe from program-owned account change notifications

#### Parameters:

- `<integer>` - id of account Subscription to cancel

#### Results:

- `<bool>` - сообщение об отмене подписки

#### Example:

Запрос:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "programUnsubscribe", "params": [0] }
```

Результат:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### signatureSubscribe

Subscribe to a transaction signature to receive notification when the transaction is confirmed On `signatureNotification`, the subscription is automatically cancelled

#### Parameters:

- `<string>` - Transaction Signature, as base-58 encoded string
- `<object>` - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment)

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

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
      "commitment": "finalized"
    }
  ]
}
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

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

### signatureUnsubscribe

Unsubscribe from signature confirmation notification

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "signatureUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### slotSubscribe

Subscribe to receive notification anytime a slot is processed by the validator

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

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

### slotUnsubscribe

Unsubscribe from slot notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Запрос:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "slotUnsubscribe", "params": [0] }
```

Результат:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### rootSubscribe

Subscribe to receive notification anytime a new root is set by the validator.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Запрос:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

The result is the latest root slot number.

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

### rootUnsubscribe

Unsubscribe from root notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "rootUnsubscribe", "params": [0] }
```

Result:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

### voteSubscribe - Unstable, disabled by default

**This subscription is unstable and only available if the validator was started with the `--rpc-pubsub-enable-vote-subscription` flag. The format of this subscription may change in the future**

Subscribe to receive notification anytime a new vote is observed in gossip. These votes are pre-consensus therefore there is no guarantee these votes will enter the ledger.

#### Parameters:

None

#### Results:

- `integer` - subscription id \(needed to unsubscribe\)

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteSubscribe" }
```

Result:

```json
{ "jsonrpc": "2.0", "result": 0, "id": 1 }
```

#### Notification Format:

The result is the latest vote, containing its hash, a list of voted slots, and an optional timestamp.

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

### voteUnsubscribe

Unsubscribe from vote notifications

#### Parameters:

- `<integer>` - subscription id to cancel

#### Results:

- `<bool>` - unsubscribe success message

#### Example:

Request:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "voteUnsubscribe", "params": [0] }
```

Response:

```json
{ "jsonrpc": "2.0", "result": true, "id": 1 }
```

## JSON RPC API Deprecated Methods

### getConfirmedBlock

**DEPRECATED: Please use [getBlock](jsonrpc-api.md#getblock) instead** This method is expected to be removed in solana-core v1.8

Returns identity and transaction information about a confirmed block in the ledger

#### Parameters:

- `<u64>` - slot, as u64 integer
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) `transactionDetails: <string>` - level of transaction detail to return, either "full", "signatures", or "none". If parameter not provided, the default detail level is "full".
  - (optional) `rewards: bool` - whether to populate the `rewards` array. If parameter not provided, the default includes rewards.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an object with the following fields:

- `<null>` - if specified block is not confirmed
- `<object>` - if block is confirmed, an object with the following fields:
  - `blockhash: <string>` - the blockhash of this block, as base-58 encoded string
  - `previousBlockhash: <string>` - the blockhash of this block's parent, as base-58 encoded string; if the parent block is not available due to ledger cleanup, this field will return "11111111111111111111111111111111"
  - `parentSlot: <u64>` - the slot index of this block's parent
  - `transactions: <array>` - present if "full" transaction details are requested; an array of JSON objects containing:
    - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
    - `meta: <object>` - transaction status metadata object, containing `null` or:
      - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>` - fee this transaction was charged, as u64 integer
      - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
      - `postBalances: <array>` - array of u64 account balances after the transaction was processed
      - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
      - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
      - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
      - DEPRECATED: `status: <object>` - Transaction status
        - `"Ok": <null>` - Transaction was successful
        - `"Err": <ERR>` - Transaction failed with TransactionError
  - `signatures: <array>` - present if "signatures" are requested for transaction details; an array of signatures strings, corresponding to the transaction order in the block
  - `rewards: <array>` - present if rewards are requested; an array of JSON objects containing:
    - `pubkey: <string>` - The public key, as base-58 encoded string, of the account that received the reward
    - `lamports: <i64>`- number of reward lamports credited or debited by the account, as a i64
    - `postBalance: <u64>` - account balance in lamports after the reward was applied
    - `rewardType: <string|undefined>` - type of reward: "fee", "rent", "voting", "staking"
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch). null if not available

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, {"encoding": "json","transactionDetails":"full","rewards":false}]}
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "blockTime": null,
    "blockhash": "3Eq21vXNB5s86c62bVuUfTeaMif1N2kUqRPBmGRJhyTA",
    "parentSlot": 429,
    "previousBlockhash": "mfcyqEXB3DnHXki6KjjmZck6YjmZLvpAByy2fj4nh6B",
    "transactions": [
      {
        "meta": {
          "err": null,
          "fee": 5000,
          "innerInstructions": [],
          "logMessages": [],
          "postBalances": [499998932500, 26858640, 1, 1, 1],
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "base64"]}
'
```

Result:

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
          "postTokenBalances": [],
          "preBalances": [499998937500, 26858640, 1, 1, 1],
          "preTokenBalances": [],
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

For more details on returned data: [Transaction Structure](jsonrpc-api.md#transactionstructure) [Inner Instructions Structure](jsonrpc-api.md#innerinstructionsstructure) [Token Balances Structure](jsonrpc-api.md#tokenbalancesstructure)

### getConfirmedBlocks

**DEPRECATED: Please use [getBlocks](jsonrpc-api.md#getblocks) instead** This method is expected to be removed in solana-core v1.8

Returns a list of confirmed blocks between two slots

#### Parameters:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - (optional) end_slot, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of u64 integers listing confirmed blocks between `start_slot` and either `end_slot`, if provided, or latest confirmed block, inclusive. Max range allowed is 500,000 slots.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7, 8, 9, 10], "id": 1 }
```

### getConfirmedBlocksWithLimit

**DEPRECATED: Please use [getBlocksWithLimit](jsonrpc-api.md#getblockswithlimit) instead** This method is expected to be removed in solana-core v1.8

Returns a list of confirmed blocks starting at the given slot

#### Parameters:

- `<u64>` - start_slot, as u64 integer
- `<u64>` - limit, as u64 integer
- (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of u64 integers listing confirmed blocks starting at `start_slot` for up to `limit` blocks, inclusive.

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

Result:

```json
{ "jsonrpc": "2.0", "result": [5, 6, 7], "id": 1 }
```

### getConfirmedSignaturesForAddress2

**DEPRECATED: Please use [getSignaturesForAddress](jsonrpc-api.md#getsignaturesforaddress) instead** This method is expected to be removed in solana-core v1.8

Returns confirmed signatures for transactions involving an address backwards in time from the provided signature or most recent confirmed block

#### Parameters:

- `<string>` - account address as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following fields:
  - `limit: <number>` - (optional) maximum transaction signatures to return (between 1 and 1,000, default: 1,000).
  - `before: <string>` - (optional) start searching backwards from this transaction signature. If not provided the search starts from the top of the highest max confirmed block.
  - `until: <string>` - (optional) search until this transaction signature, if found before limit reached.
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

The result field will be an array of transaction signature information, ordered from newest to oldest transaction:

- `<object>`
  - `signature: <string>` - transaction signature as base-58 encoded string
  - `slot: <u64>` - The slot that contains the block with the transaction
  - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - `memo: <string |null>` - Memo associated with the transaction, null if no memo is present
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when transaction was processed. null if not available.

#### Example:

Request:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": [
    {
      "err": null,
      "memo": null,
      "signature": "5h6xBEauJ3PK6SWCZ1PGjBvj8vDdWG3KpwATGy1ARAXFSDwt8GFXM7W5Ncn16wmqokgpiKRLuS83KUxyZyv2sUYv",
      "slot": 114,
      "blockTime": null
    }
  ],
  "id": 1
}
```

### getConfirmedTransaction

**DEPRECATED: Please use [getTransaction](jsonrpc-api.md#gettransaction) instead** This method is expected to be removed in solana-core v1.8

Returns transaction details for a confirmed transaction

#### Parameters:

- `<string>` - transaction signature as base-58 encoded string
- `<object>` - (optional) Configuration object containing the following optional fields:
  - (optional) `encoding: <string>` - encoding for each returned Transaction, either "json", "jsonParsed", "base58" (_slow_), "base64". If parameter not provided, the default encoding is "json". "jsonParsed" encoding attempts to use program-specific instruction parsers to return more human-readable and explicit data in the `transaction.message.instructions` list. If "jsonParsed" is requested but a parser cannot be found, the instruction falls back to regular JSON encoding (`accounts`, `data`, and `programIdIndex` fields).
  - (optional) [Commitment](jsonrpc-api.md#configuring-state-commitment); "processed" is not supported. If parameter not provided, the default is "finalized".

#### Results:

- `<null>` - if transaction is not found or not confirmed
- `<object>` - if transaction is confirmed, an object with the following fields:
  - `slot: <u64>` - the slot this transaction was processed in
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure) object, either in JSON format or encoded binary data, depending on encoding parameter
  - `blockTime: <i64 | null>` - estimated production time, as Unix timestamp (seconds since the Unix epoch) of when the transaction was processed. null if not available
  - `meta: <object | null>` - transaction status metadata object:
    - `err: <object | null>` - Error if transaction failed, null if transaction succeeded. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - fee this transaction was charged, as u64 integer
    - `preBalances: <array>` - array of u64 account balances from before the transaction was processed
    - `postBalances: <array>` - array of u64 account balances after the transaction was processed
    - `innerInstructions: <array|undefined>` - List of [inner instructions](#inner-instructions-structure) or omitted if inner instruction recording was not yet enabled during this transaction
    - `preTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from before the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `postTokenBalances: <array|undefined>` - List of [token balances](#token-balances-structure) from after the transaction was processed or omitted if token balance recording was not yet enabled during this transaction
    - `logMessages: <array>` - array of string log messages or omitted if log message recording was not yet enabled during this transaction
    - DEPRECATED: `status: <object>` - Transaction status
      - `"Ok": <null>` - Transaction was successful
      - `"Err": <ERR>` - Transaction failed with TransactionError

#### Example:

Request:

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

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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
  "blockTime": null,
  "id": 1
}
```

#### Example:

Request:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getConfirmedTransaction",
    "params": [
      "2nBhEBYYvfaAe16UMNqRHre4YNSskvuYgx3M6E4JP1oDYvZEJHvoPzyUidNgNX5r9sTyN1J9UxtbCXy2rqYcuyuv",
      "base64"
    ]
  }
'
```

Result:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [499998932500, 26858640, 1, 1, 1],
      "postTokenBalances": [],
      "preBalances": [499998937500, 26858640, 1, 1, 1],
      "preTokenBalances": [],
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
