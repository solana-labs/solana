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
- [getBlockCommitment](jsonrpc-api.md#getblockcommitment)
- [getBlockTime](jsonrpc-api.md#getblocktime)
- [getClusterNodes](jsonrpc-api.md#getclusternodes)
- [getConfirmedBlock](jsonrpc-api.md#getconfirmedblock)
- [getConfirmedBlocks](jsonrpc-api.md#getconfirmedblocks)
- [getConfirmedBlocksWithit](jsonrpc-api.md#getconfirmedblockswithlimit)
- [getConfirmedSignaturesForAddress](jsonrpc-api.md#getconfirmedsignaturesforaddress)
- [getConfirmedSignaturesForAddres2](jsonrpc-api.md#getconfirmedsignaturesforaddress2)
- [getConfirmedTransaction](jsonrpc-api.md#getconfirmedtransaction)
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
- [getLargestAccounts](jsonrpc-api.md#getlargestaccounts)
- [getLeaderSchedule](jsonrpc-api.md#getleaderschedule)
- [getMinimumBalanceForRentExemption](jsonrpc-api.md#getminimumbalanceforrentexemption)
- [getMultipleAccounts](jsonrpc-api.md#getmultipleaccounts)
- [getProgramAccounts](jsonrpc-api.md#getprogramaccounts)
- [getRecentBlockhash](jsonrpc-api.md#getrecentblockhash)
- [getRecentPerformanceSamples](jsonrpc-api.md#getrecentperformancesamples)
- [getSignatureStatuses](jsonrpc-api.md#getsignaturestatuses)
- [getSlot](jsonrpc-api.md#getslot)
- [getSlotLeader](jsonrpc-api.md#getslotleader)
- [getkeActivation](jsonrpc-api.md#getstakeactivation)
- [getSupply](jsonrpc-api.md#getsupply)
- [getTransactionCount](jsonrpc-api.md#gettransactioncount)
- [getVersion](jsonrpc-api.md#getversion)
- [getVoteAccounts](jsonrpc-api.md#getvoteaccounts)
- [minimumLedgerSlot](jsonrpc-api.md#minimumledgerslot)
- [requestAirdrop](jsonrpc-api.md#requestairdrop)
- [sendTransaction](jsonrpc-api.md#sendtransaction)
- [simulateTransaction](jsonrpc-api.md#simulatetransaction)
- [setLogFilter](jsonrpc-api.md#setlogfilter)
- [validatorExit](jsonrpc-api.md#validatorexit)
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

## Нестабильные методы

Нестабильные методы могут претерпевать критические изменения в выпусках исправлений и могут не поддерживаться бессрочно.

- [getTokenAccountBalance](jsonrpc-api.md#gettokenaccountbalance)
- [getTokenAccountsByDelegate](jsonrpc-api.md#gettokenaccountsbydelegate)
- [getTokenAccountsByOwner](jsonrpc-api.md#gettokenaccountsbyowner)
- [getTokenLargestAccounts](jsonrpc-api.md#gettokenlargestaccounts)
- [getTokenSupply](jsonrpc-api.md#gettokensupply)

## Форматирование запроса

Чтобы сделать запрос JSON-RPC отправьте запрос POST HTTP с заголовком `Content-Type:
application/json`. Данные запроса JSON должны содержать 4 поля:

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

Для предварительных проверок и обработки транзакций узлы Solana выбирают состояние банка для запроса на основе требования к обязательству, установленного клиентом. Обязательство описывает, насколько завершен блок на данный момент.  При запросе состояния реестра рекомендуется использовать более низкие уровни обязательств для отчета о прогрессе и более высокие уровни, чтобы гарантировать, что состояние не будет возвращено.

В порядке убывания обязательств (от наиболее завершенных до наименее завершенных) клиенты могут указать:

- ` "max" ` - узел будет запрашивать самый последний блок, подтвержденный супербольшинством кластера как достигший максимальной блокировки, что означает, что кластер распознал этот блок как завершенный
- ` "root" ` - узел будет запрашивать самый последний блок, достигший максимальной блокировки на этом узле, что означает, что узел распознал этот блок как завершенный
- `"singleGossip"` - узел будет запрашивать самый последний блок, за который проголосовало супербольшинство кластера.
  - Он включает голоса gossip и повторов.
  - Он не учитывает голоса потомков блока, а только прямые голоса в этом блоке.
  - Этот уровень подтверждения также поддерживает гарантии «оптимистичного подтверждения» в версии 1.3 и новее.
- `"recent"` - узел запросит свой самый последний блок.  Обратите внимание, что блок может быть незавершен.

Для обработки многих зависимых транзакций в серии рекомендуется использовать команду `"singleGossip"`, которая балансирует скорость и безопасность возврата. Для полной безопасности рекомендуется использовать обязательство ` "max" `.

#### Примеры

Параметр фиксации должен быть включен в качестве последнего элемента в массив ` params `:

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

#### По умолчанию:

Если конфигурация фиксации не указана, для узла по умолчанию будет использоваться фиксация ` "max" `

Только методы, которые запрашивают состояние банка принимают параметр обязательства. Они указаны ниже в описании API.

#### Структура RpcResponse

Многие методы, которые принимают параметр обязательства возвращают объект RpcResponse JSON, состоящий из двух частей:

- `context` : Структура JSON RpcResponseContext с `слотом` в котором была вычислена операция.
- ` value `: значение, возвращаемое самой операцией.

## Проверка состояния

Хотя это и не JSON RPC API, ` GET / health ` в конечной точке RPC HTTP предоставляет механизм проверки работоспособности для использования балансировщиками нагрузки или другой сетевой инфраструктурой. Этот запрос всегда возвращает HTTP 200 OK ответ с телом "OK" или "behind" на основе следующих условий:

1. Если один или несколько аргументов ` --trusted-validator ` предоставлены для ` solana-validator `, возвращается "ok", когда узел имеет в пределах ` HEALTH_CHECK_SLOT_DISTANCE ` слоты самого надежного валидатора, в противном случае возвращается значение "behind".
2. "Ok" всегда возвращается, если нет доверенных валидаторов.

## Справочник по JSON RPC API

### getAccountInfo

Возвращает всю информацию, связанную с учетной записью Pubkey

#### Параметры:

- ` & lt; string & gt; ` - Pubkey аккаунта для запроса в виде строки в кодировке Base-58
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - (опционное) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - `кодировка: <string>` - кодировка для аккаунта, либо "base58" (*slow*), "base64", "base64+zstd", или "jsonParsed". "base58" ограничивается данными счета менее 128 байт. "base64" возвращает данные base64 для данных аккаунта любого размера. "base64+zstd" сжимает данные аккаунта с помощью [Zstandard](https://facebook.github.io/zstd/) и base64-кодирует результат. "jsonParsed" кодировка пытается использовать парсеры специфичных для программы состояний для возврата более читаемых и явных данных о состоянии аккаунта. Если запрошен "jsonParsed", но парсер не найден, поле возвращается в кодировку "base64", обнаруживается, когда поле `данных` типа `<string>`.
  - (опционное) `dataSlice: <object>` - ограничить возвращаемые данные аккаунта, используя предоставленные `смещение: <usize>` и `длина: <usize>` поля; доступно только для кодировок "base58", "base64" или "base64+zstd".

#### Результаты:

Результатом будет объект JSON RpcResponse со значением ` value `, равным:

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

- `RpcResponse<u64>`- объект JSON RpcResponse с полем ` value `, установленным на баланс

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getBalance", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri"]}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":0},"id":1}
```

### getBlockCommitment

Возвращает обязательства для конкретного блока

#### Параметры:

- `<u64>` - block, identified by Slot

#### Результаты:

Поле результата будет объектом JSON, содержащим:

- `commitment` - приверженность, включая либо:
  - `<null>` - Неизвестный блок
  - `<array>`- обязательство, массив целых чисел u64, регистрирующий количество ставок кластера в лємпортах, проголосовавших за блок на каждой глубине от 0 до ` MAX_LOCKOUT_HISTORY ` + 1
- `всего разбито` - общая активнеая ставка в лємпорт, текущей эпохи

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockCommitment","params":[5]}
'
```

Результат:
```json
{
  "jsonrpc":"2.0",
  "result":{
    "commitment":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,10,32],
    "totalStake": 42
  },
  "id":1
}
```

### getBlockTime

Возвращает предполагаемое время производства утвержденного блока.

Каждый валидатор сообщает свое время в формате UTC в регистр через регулярный интервал, периодически добавляя временную метку к голосованию для определенного блока. Время запрошенного блока рассчитывается на основе взвешенного по ставке среднего значения временных меток голосования в наборе последних блоков, записанных в реестре.

Узлы, которые загружаются из моментального снимка или ограничивают размер реестра (путем очистки старых слотов), будут возвращать нулевые временные метки для блоков ниже их самого низкого корня + ` TIMESTAMP_SLOT_RANGE `. Пользователи, заинтересованные в том, что эти исторические данные должны запросить узел, который построен из генезиса, и сохранить весь реестр.

#### Параметры:

- `<u64>` - блок, идентифицированный слотом

#### Результаты:

* ` & lt; i64 & gt; ` - расчетное время производства в виде отметки времени Unix (секунды с начала эпохи Unix)
* `<null>` - временная метка недоступна для этого блока

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getBlockTime","params":[5]}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":1574721591,"id":1}
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
- `version: <string>|null` - версия программного обеспечения узла или ` null `, если информация о версии недоступна

#### Пример:

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

### getConfirmedBlock

Возвращает идентификационные данные и информацию транзакции о подтвержденном блоке в реестре

#### Параметры:

- `<u64>` - слот, как u64 целое число
- `<string>` - кодировка для каждой возвращенной транзакции, либо "json", "jsonParsed", "base58" (*slow*), "base64". Если параметр не указан, кодировка по умолчанию "json". Кодировка "jsonParsed" пытается использовать парсеры инструкций специфичных для программы для возврата более читаемых и явных данных в списке `transaction.message.instructions`. Если "jsonParsed" запрошен, но синтаксический анализатор не может быть найден, инструкция возвращается к обычной кодировке JSON (поля ` accounts `, ` data ` и ` programIdIndex `).

#### Результаты:

Поле результата будет объектом со следующими полями:

- `<null>` - если указанный блок не подтвержден
- `<object>` - если блок подтвержден, объект со следующими полями:
  - `blockhash: <string>` - хэш блока, как строка с кодировкой 58
  - ` previousBlockhash: & lt; string & gt; ` - хеш-код родительского блока в виде строки в кодировке base-58; если родительский блок недоступен из-за очистки реестра, в этом поле будет возвращено «11111111111111111111111111111111»
  - `parentSlot: <u64>` - индекс ячейки родителя этого блока
  - `транзакций: <array>` - массив объектов JSON, содержащий:
    - `transaction: <object|[string,encoding]>` - объект [ Transaction ](#transaction-structure), либо в формате JSON, либо в закодированных двоичных данных, в зависимости от параметр кодирования
    - `meta: <object>`- объект метаданных статуса транзакции, содержащий ` null ` или:
      - `err: <object | null>` - Ошибка при сбое транзакции, null если транзакция успешна. [TransactionError definitions](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
      - `fee: <u64>`- комиссия за транзакцию была списана в виде целого числа u64
      - `preBalances: <array>`- массив остатков на счете u64 до обработки транзакции
      - `postBalances: <array>` - массив остатков на счете u64 после обработки транзакции
      - `innerInstructions: <array|undefined>` - список [ внутренних инструкций ](#inner-instructions-structure) или опускается, если внутренняя запись инструкций еще не была включена во время этой транзакции
      - `logMessages: <array>` - массив строковых сообщений журнала или опускается, если запись сообщения журнала еще не была включена во время этой транзакции
      - УСТАРЕЛО:`status: <object>` - статус транзакции
        - `"Ок": <null>` - Транзакция прошла успешно
        - `"Err": <ERR>` - Транзакция не удалась с ошибкой TransactionError
  - `rewards: <array>` - массив объектов JSON, содержащий:
    - `pubkey: <string>`- открытый ключ в виде строки в кодировке Base-58 учетной записи, получившей вознаграждение
    - `lamports: <i64>` - количество лампортов вознаграждения, начисленных или списанных с учетной записи, как i64
    - `postBalance: <u64>` - баланс счета в лэмпортах после применения вознаграждения
    - `rewardType: <string|undefined>`- тип вознаграждения: «комиссия», «аренда», «голосование», «ставка»
  - `blockTime: <i64 | null>` - расчетное время производства в виде отметки времени Unix (секунды с начала эпохи Unix). null если не доступен

#### Примеры:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "json"]}
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
          "postBalances": [
            499998932500,
            26858640,
            1,
            1,
            1
          ],
          "preBalances": [
            499998937500,
            26858640,
            1,
            1,
            1
          ],
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
                "accounts": [
                  1,
                  2,
                  3,
                  0
                ],
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

#### Пример:
Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[430, "base64"]}
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
          "postBalances": [
            499998932500,
            26858640,
            1,
            1,
            1
          ],
          "preBalances": [
            499998937500,
            26858640,
            1,
            1,
            1
          ],
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
  - `accountKeys: <array[string]>`- список открытых ключей в кодировке base-58, используемых транзакцией, в том числе в инструкциях и для подписей. Первые публичные ключи ` message.header.numRequiredSignatures ` должны подписывать транзакцию.
  - `header: <object>` - подробные сведения о типах учетных записей и подписях, необходимых для транзакции.
    - `numRequiredSignatures: <number>` - общее количество подписей, необходимых для подтверждения транзакции. Подписи должны соответствовать первому ` numRequiredSignatures ` в ` message.account_keys `.
    - `numReadonlySignedAccounts: <number>` - последние ` numReadonlySignedAccounts ` из подписанных ключей являются аккаунтами только для чтения. Программы могут обрабатывать несколько транзакций, которые загружают аккаунты только для чтения в одной записи PoH, но им не разрешается кредитовать или дебетовать лэмпорты или изменять данные аккаунта. Транзакции, ориентированные на одни и те же аккаунты для чтения и записи, оцениваются последовательно.
    - `numReadonlyUnsignedAccounts: <number>`- последние ` numReadonlyUnsignedAccounts ` из неподписанных ключей являются аккаунтами только для чтения.
  - `recentBlockhash: <string>` - хеш-код последнего блока в реестре в кодировке Base-58, используемый для предотвращения дублирования транзакций и определения времени жизни транзакций.
  - `instructions: <array[object]>` - список программных инструкций, которые будут выполняться последовательно и фиксироваться в одной атомарной транзакции, если все они выполнены успешно.
    - `programIdIndex: <number>` - индексирует массив ` message.accountKeys `, указывающий аккаунт программы, которая выполняет эту инструкцию.
    - `accounts: <array[number]>` - список упорядоченных индексов в массиве ` message.accountKeys `, указывающих, какие учетные записи передать программе.
    - `data: <string>` - входные данные программы, закодированные в строке base-58.

#### Структура внутренних инструкций

Среда выполнения Solana записывает межпрограммные инструкции, которые вызываются во время обработки транзакции, и делает их доступными для большей прозрачности того, что было выполнено в цепочке для каждой инструкции транзакции. Вызванные инструкции группируются по инструкции исходящей транзакции и перечисляются в порядке обработки.

Структура внутренних инструкций JSON определяется как список объектов в следующей структуре:

- ` index: number ` - индекс инструкции транзакции, из которой произошли внутренние инструкции
- `instructions: <array[object]>` - упорядоченный список внутренних программных инструкций, которые были вызваны во время одной инструкции транзакции.
  - `programIdIndex: <number>` - индексирует массив ` message.accountKeys `, указывающий аккаунт программы, которая выполняет эту инструкцию.
  - `accounts: <array[number]>` - список упорядоченных индексов в массиве ` message.accountKeys `, указывающих, какие аккаунты передать программе.
  - `data: <string>`- входные данные программы, закодированные в строке base-58.

### getConfirmedBlocks

Возвращает список подтвержденных блоков между двумя слотами

#### Параметры:

- `<u64>` - start_slot,, как целое число u64
- `<u64>` - (опционально) end_slot, как u64 целое число

#### Результаты:

Поле результата будет представлять собой массив целых чисел u64, в котором перечислены подтвержденные блоки между ` start_slot ` и либо ` end_slot `, если он предоставлен, либо последним подтвержденным блоком, включительно.  Максимальный допустимый диапазон - 500,000 слотов.


#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5, 10]}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":[5,6,7,8,9,10],"id":1}
```

### getConfirmedBlocksWithit

Возвращает список подтвержденных блоков, начиная с указанного слота

#### Параметры:

- `<u64>` - start_slot, как целое число u64
- `<u64>` - limit, как u64 целое число

#### Результат:

Поле результата будет представлять собой массив целых чисел u64, в котором перечислены подтвержденные блоки, начинающиеся с ` start_slot `, до ` limit ` блоков включительно.

#### Примеры:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocksWithLimit","params":[5, 3]}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":[5,6,7],"id":1}
```

### getConfirmedSignaturesForAddress

**УСТАРЕЛО: используйте вместо этого getConfirmedSignaturesForAddress2**

Возвращает список всех подтвержденных подписей транзакций с адресом в пределах указанного диапазона слотов. Максимальный допустимый диапазон - 10,000 слотов

#### Параметры:

- `<string>` - адрес аккаунта в виде строки в кодировке base-58
- `<u64>` - начальный слот включительно
- `<u64>` - конец слота, включительно

#### Результат:

Поле результата будет состоять из массива:

- `<string>` - подпись транзакции в виде строки в кодировке base-58

Подписи будут упорядочены в зависимости от слота, в котором они были подтверждены, от самого низкого до самого высокого слота

#### Пример:

Запрос:
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
'
```

Результат:
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

### getConfirmedSignaturesForAddres2

Возвращает подтвержденные подписи для транзакций, связанных с обратный адрес во времени от предоставленной подписи или самого последнего подтвержденного блока

#### Параметры:
* `<string>` - адрес аккаунта в виде строки в кодировке base-58
* `<object>` - (необязательно) объект конфигурации, содержащий следующие поля:
  * `limit: <number>` - (необязательно) максимальное количество возвращаемых подписей транзакций (от 1 до 1000, по умолчанию: 1000).
  * `before: <string>` - (необязательно) начать поиск в обратном направлении от этой подписи транзакции. Если не указано иное, поиск начинается с вершины самого высокого максимального подтвержденного блока.
  * `until: <string>` - (необязательно) поиск до этой подписи транзакции, если она была найдена до достижения лимита.

#### Результат:
Поле результата будет представлять собой массив информации о подписи транзакции, упорядоченный от самой новой к самой старой транзакции:
* `<object>`
  * `signature: <string>` - подпись транзакции в виде строки в кодировке base-58
  * `slot: <u64>`- слот, содержащий блок с транзакцией
  * `err: <object | null>` - ошибка, если транзакция не удалась, null, если транзакция прошла успешно. [Определения TransactionError ](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  * `memo: <string |null>` - заметка, связанная с транзакцией, null, если заметки нет

#### Пример:
Запрос:
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

Результат:
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

### getConfirmedTransaction

Возвращает детали транзакции для подтвержденной транзакции

#### Параметры:

- `<string>`- подпись транзакции в виде строки в кодировке base-58 Кодирование N пытается использовать программные синтаксические анализаторы инструкций, чтобы вернуть более понятные и понятные данные в списке `transaction.message.instructions`. Если "jsonParsed" запрошен, но синтаксический анализатор не может быть найден, инструкция возвращается к обычной кодировке JSON (поля ` accounts `, ` data ` и ` programIdIndex `).
- `<string>` - (необязательно) кодировка для возвращенной транзакции, либо "json", "jsonParsed", "base58" (*slow*)или base64. Если параметр не указан, кодировка по умолчанию JSON.

#### Результаты:

- `<null>` - если транзакция не найдена или не подтверждена
- `<object>`- если транзакция подтверждена, объект со следующими полями:
  - `slot: <u64>` - слот, в котором была обработана эта транзакция
  - `transaction: <object|[string,encoding]>` - [Transaction](#transaction-structure), либо в формате JSON, либо в закодированных двоичных данных, в зависимости от параметра кодирования
  - `meta: <object | null>` - объект метаданных статуса транзакции:
    - `err: <object | null>` - ошибка, если транзакция не удалась, null, если транзакция прошла успешно. [ Определения TransactionError ](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
    - `fee: <u64>` - комиссия за транзакцию была списана в виде целого числа u64
    - `preBalances: <array>` - массив остатков на счете u64 до обработки транзакции
    - `postBalances: <array>`- массив остатков на счете u64 после обработки транзакции
    - `innerInstructions: <array|undefined>` - список [inner instructions](#inner-instructions-structure) или опускается, если внутренняя запись инструкций еще не была включена во время этой транзакции
    - `logMessages: <array>`- массив строковых сообщений журнала или опускается, если запись сообщения журнала еще не была включена во время этой транзакции
    - УСТАРЕЛО:`status: <object>` - статус транзакции
      - `"Ok": <null>` - транзакция прошла успешно
      - `"Err": <ERR>` - транзакция завершилась ошибкой TransactionError

#### Примеры:
Запрос:
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

Результат:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [
        499998932500,
        26858640,
        1,
        1,
        1
      ],
      "preBalances": [
        499998937500,
        26858640,
        1,
        1,
        1
      ],
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
            "accounts": [
              1,
              2,
              3,
              0
            ],
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

#### Примеры:
Запрос:
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

Результат:
```json
{
  "jsonrpc": "2.0",
  "result": {
    "meta": {
      "err": null,
      "fee": 5000,
      "innerInstructions": [],
      "postBalances": [
        499998932500,
        26858640,
        1,
        1,
        1
      ],
      "preBalances": [
        499998937500,
        26858640,
        1,
        1,
        1
      ],
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

### getEpochInfo

Возвращает информацию о текущей эпохе

#### Параметры:

- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Поле результата будет объектом со следующими полями:

- `absoluteSlot: <u64>`, текущий слот
- `blockВысота: <u64>`, текущая высота блока
- `epoch: <u64>`, текущая эпоха
- `slotIndex: <u64>`, текущий слот относительно начала текущей эпохи
- `slotsInEpoch: <u64>`, количество слотов в эту эпоху

#### Пример:

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

#### Пример:

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

Возвращает калькулятор комиссии, связанный с хэшем блока запроса, или ` null `, если срок действия хэша блока истек

#### Параметры:

- `<string>` - запросить хэш блока как строку в кодировке Base58
- `<object>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результаты:

Результатом будет объект JSON RpcResponse со значением ` value `, равным:

- `<null>` - если срок действия блок хэша запроса истек
- `<object>` - в противном случае объект JSON, содержит:
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

Поле ` result ` будет ` объектом ` со следующими полями:

- `burnPercent: <u8>`, процент собранных сборов, подлежащих уничтожению
- `maxLamportsPerSignature: <u64>`, максимальное значение, которое ` lamportsPerSignature ` может получить для следующего слота
- ` minLamportsPerSignature: <u64>`, наименьшее значение, которое ` lamportsPerSignature ` может получить для следующего слота
- ` targetLamportsPerSignature:<u64>`, желаемая ставка комиссии для кластера
- `targetSignaturesPerSlot: <u64>` желаемая скорость подписи для кластера

#### Пример:

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

- `<object>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Результатом будет объект JSON RpcResponse со значением ` value `, установленным на объект JSON со следующими полями:

- `blockhash: <string>` - хэш в виде строки в кодировке base-58
- `feeCalculator: <object>` - объект FeeCalculator, тарифный план для этого хэша блока
- `lastValidSlot: <u64>`- последний слот, в котором будет действителен хеш-блок

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

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getFirstAvailableBlock"}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":250000,"id":1}
```

### getGenesisHash

Возвращает хэш генеза

#### Параметры:

None

#### Результат:

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
{"jsonrpc":"2.0","result":"GH7ome3EiwEr7tu9JuTh2dpYWBJK3z69Xm1ZE3MEE6JC","id":1}
```

### getHealth

Возвращает текущее состояние узла.

Если один или несколько аргументов ` --trusted-validator ` предоставлены для ` solana-validator `, возвращается "ok", когда узел находится внутри слота ` HEALTH_CHECK_SLOT_DISTANCE ` самого надежного валидатора, в противном случае возвращается ошибка.  "Ok" всегда возвращается, если нет доверенных валидаторов.

#### Параметры:

None

#### Результаты:

Если узел исправен: "ок" Если узел неисправен, возвращается ответ об ошибке JSON RPC.  Особенности ответа об ошибке: **UNSTABLE** и могут измениться в будущем


#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getHealth"}
'
```

Хороший результат:
```json
{"jsonrpc":"2.0","result": "ok","id":1}
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

#### Результат:

Поле результата будет объектом JSON со следующими полями:

- ` identity `, идентификатор pubkey текущего узла \ (в виде строки в кодировке base-58 \)

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getIdentity"}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":{"identity": "2r1F4iWqVcb8M1DbAjQuFpebkQHY9hcVU4WuW2DJBppN"},"id":1}
```

### getInflationGovernor

Возвращает текущего регулятора инфляции

#### Параметры:

- `<object>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Поле результата будет объектом JSON со следующими полями:

- `initial: <f64>`,начальный процент инфляции с момента 0
- `terminal: <f64>`, предельная инфляция в процентах
- `taper: <f64>`,годовая скорость снижения инфляции
- `foundation: <f64>`процент от общей инфляции, выделенный на фонд
- `foundationTerm: <f64>`,продолжительность инфляции фонда в годах

#### Пример:

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

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getInflationRate"}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":{"epoch":100,"foundation":0.001,"total":0.149,"validator":0.148},"id":1}
```

### getLargestAccounts

Возвращает 20 крупнейших счетов по балансу лэмпорта

#### Параметры:

- `<object>`- (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - (опционально) `filter: <string>` - фильтровать результаты по типу аккаунта; в настоящее время поддерживается: `circulating|nonCirculating`

#### Результаты:

Результатом будет объект JSON RpcResponse со значением ` value `, равным массиву:

- `<object>` в противном случае объект JSON, содержащий:
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
- `<object>` - (optional) [требуемая степень подтвержденности](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

- `<null>` - если запрашиваемая эпоха не была найдена
- `<object>` - в противном случае результатом будет словарь, где ключами будут публичных адреса лидеров \(как строк с кодировкой base-58), а их соответствующие индексы слотов - в качестве значений ключа (нумерация индексов происходит относительно первого слота в запрошеваемую эпоху)

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getLeaderSchedule"}
'
```

Результат:
```json
{
  "jsonrpc":"2.0",
  "result":{
    "4Qkev8aNZcqFNSRhQzwyLMFSsi94jHqE8WNVTJzTP99F":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47,48,49,50,51,52,53,54,55,56,57,58,59,60,61,62,63]
  },
  "id":1
}
```

### getMinimumBalanceForRentExemption

Возвращает минимальный баланс, необходимый для освобождения аккаунта от арендной платы.

#### Параметры:

- `<usize>` - длина данных аккаунта
- `<object>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

- `<u64>` - минимум лэмпортов, необходимых в аккаунте

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0", "id":1, "method":"getMinimumBalanceForRentExemption", "params":[50]}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":500,"id":1}
```

### getMultipleAccounts

Возвращает информацию об аккаунте для списка Pubkeys

#### Параметры:

- `<array>` - массив ключей Pubkeys для запроса в виде строк в кодировке base-58
- `<object>` - (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>`- кодировка для данных аккаунта, либо "base58"(*slow*), "base64", "base64 + zstd", или "jsonParsed". "base58" ограничен данными Учетной записи размером менее 128 байт. "base64" возвращает данные base64 для данных аккаунта любого размера. "base64+zstd" сжимает данные аккаунта с помощью [Zstandard](https://facebook.github.io/zstd/) и base64-кодирует результат. Кодировка "jsonParsed" пытается использовать программно-зависимые парсеры состояния для возврата более понятных и явных данных о состоянии аккаунта. Если "jsonParsed" запрошен, но синтаксический анализатор не может быть найден, поле возвращается к кодировке "base64", что можно обнаружить, когда поле ` data ` имеет тип `<string>`.
  - (необязательно) `dataSlice: <object>` - ограничить возвращаемые данные аккаунта, используя предоставленные `offset: <usize>` и `length: <usize>` поля; доступно только для кодировок «base58», «base64» или «base64 + zstd».


#### Результат:

Результатом будет объект JSON RpcResponse со значением ` value `, равным:

Массив из:

- `<null>` - если аккаунт в этом Pubkey не существует
- `<object>` - в противном случае объект JSON, содержит:
  - `лампы: <u64>`, количество лємпортов, назначенных этому аккаунту, в качестве u64
  - `owner: <string>`, Pubkey в кодировке base-58 программы, которой назначена эта аккаунту
  - `data: <[string, encoding]|object>`, данные, связанные с аккаунтом, либо в виде закодированных двоичных данных, либо в формате JSON `{<program>: <state>}`, в зависимости от параметра кодировки
  - `executable: <bool>`, логическое значение, указывающее, содержит ли аккаунт программу \ (и предназначена ли она только для чтения \)
  - `rentEpoch: <u64>`, эпоха, в которую этот аккаунт будет в следующий раз должен арендовать, как u64

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
        "data": [
          "AAAAAAEAAAACtzNsyJrW0g==",
          "base64"
        ],
        "executable": false,
        "lamports": 1000000000,
        "owner": "11111111111111111111111111111111",
        "rentEpoch": 2
      },
      {
        "data": [
          "",
          "base64"
        ],
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
        "data": [
          "",
          "base58"
        ],
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
  - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)
  - `encoding: <string>`- кодировка для данных аккаунта, либо "base58"(*slow*), "base64", "base64 + zstd", или "jsonParsed". "base58" ограничивается данными счета менее 128 байт. "base64" возвращает данные base64 для данных аккаунта любого размера. "base64+zstd" сжимает данные аккаунта с помощью [Zstandard](https://facebook.github.io/zstd/) и base64-кодирует результат. "jsonParsed" кодировка пытается использовать парсеры специфичных для программы состояний для возврата более читаемых и явных данных о состоянии аккаунта. Если "jsonParsed" запрошен, но синтаксический анализатор не может быть найден, поле возвращается к кодировке "base64", что можно обнаружить, когда поле `data` имеет тип `<string>`.
  - (необязательно) `dataSlice: <object>`- ограничить возвращаемые данные аккаунта, используя предоставленные `offset: <usize>` and `length: <usize>` поля; доступно только для кодировок «base58», «base64» или «base64 + zstd».
  - (опционное) `filters: <array>` - фильтровать результаты, используя различные [filter objects](jsonrpc-api.md#filters); аккаунт должен соответствовать всем критериям фильтрации для включения в результаты

##### Фильтры:
- `memcmp: <object>` - сравнивает введенную серию байт с данными аккаунта программы по конкретному смещению. Поля:
  - `offset: <usize>` - смещение в данных учетной записи программы для начала сравнения
  - `bytes: <string>` - данные для сопоставления в виде строки в кодировке base-58

- `dataSize: <u64>` - сравнивает длину данных учетной записи программы с предоставленным размером данных

#### Результат:

Полем результата будет массив JSON-объектов, который будет содержать:

- `pubkey: <string>` - Pubkey аккаунта в виде строки в кодировке base-58
- `account: <object>` - объект JSON со следующими подполями:
   - `лампы: <u64>`, количество лємпортов, назначенных этому аккаунту, в качестве u64
   - `owner: <string>`, Pubkey в кодировке base-58 программы, которой назначен аккаунт `data: <[string,encoding]|object>` данные, связанные с аккаунтом, либо в виде закодированных двоичных данных, либо в формате JSON `{<program>: <state>}`, в зависимости от параметра кодировки
   - `executable: <bool>`, логическое значение, указывающее, содержит ли аккаунт программу \ (и предназначена ли она только для чтения \)
   - `rentEpoch: <u64>`, эпоха, в которую этот аккаунт будет в следующий раз должен арендовать, так как u64

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

#### Примеры:
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

- `<object>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

RpcResponse, содержащий объект JSON, состоящий из строкового хэша блока и объекта FeeCalculator JSON.

- `RpcResponse<object>` - объект JSON RpcResponse с полем ` value `, установленным на объект JSON, включая:
- `blockhash: <string>`- хэш в виде строки в кодировке base-58
- `feeCalculator: <object>` - объект FeeCalculator, график комиссий для хеша этого блока

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d 'i
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
- ` limit: <usize> ` - (необязательно) количество возвращаемых выборок (максимум 720)

#### Результат:

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

#### Результаты:

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
{"jsonrpc":"2.0","result":100,"id":1}
```

Результат, когда узел не имеет снимка:
```json
{"jsonrpc":"2.0","error":{"code":-32008,"message":"No snapshot"},"id":1}
```

### getSignatureStatuses

Возвращает статусы списка подписей. ` searchTransactionHistory ` параметр конфигурации включен, только этот метод выполняет поиск в кэше последних статусов подписей, который сохраняет статусы для всех активные слоты плюс ` MAX_RECENT_BLOCKHASHES ` корневые слоты.

#### Параметры:

- `<array>` - Массив подписей транзакций для подтверждения в виде строк в кодировке base-58
- `<object>` - (необязательно) объект конфигурации, содержащий следующее поле:
  - ` searchTransactionHistory: <bool> ` - если true, узел Solana будет искать в своем кэше реестра любые подписи, не найденные в кэше недавнего состояния

#### Результат:

RpcResponse, содержащий объект JSON, состоящий из массива объектов TransactionStatus.

- `RpcResponse<object> ` - JSON-объект RpcResponse с полем ` value `:

Массив из:

- `<null>` - Неизвестная транзакция
- `<object>`
  - ` slot: <u64> ` - Слот, в котором была обработана транзакция
  - `confirmations: <usize | null>`- Количество блоков с момента подтверждения подписи, null, если внедрено, а также завершено подавляющим большинством кластера
  - ` err: <object | null> ` - Ошибка при неудачной транзакции, null при успешной транзакции. [ Определения TransactionError ](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
  - ` confirmStatus: <string | null> ` - статус подтверждения транзакции в кластере; либо ` обработано `, ` подтверждено `, либо ` завершено `. См. [ Обязательство ](jsonrpc-api.md#configuring-state-commitment) для получения дополнительной информации об оптимистическом подтверждении.
  - УСТАРЕЛО: ` status: <object> ` - Статус транзакции
    - `"Ok": <null>`- транзакция прошла успешно
    - ` "Err": <ERR> ` - транзакция завершилась ошибкой TransactionError

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
        "confirmationStatus": "confirmed",
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
        "confirmationStatus": "finalized",
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

- `< object>` - (необязательно) [ Обязательство ](jsonrpc-api. md#configuring-state-commitment)

#### Результат:

- `<u64>` - Текущий слот

#### Примеры:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlot"}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":1234,"id":1}
```

### getSlotLeader

Возвращает текущий слота лидера

#### Параметры:

- `< object>` - (необязательно) [ Обязательство ](jsonrpc-api. md#configuring-state-commitment)

#### Результат:

- `pubkey: < string>` - Pubkey аккаунта в виде строки в кодировке base-58

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getSlotLeader"}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":"ENvAW7JScgYq6o4zKZwewtkzzJgDzuJAFxYasvmEQdpS","id":1}
```

### getkeActivation

Возвращает информацию активации эпохи для аккаунта ставки

#### Параметры:

* `<string>` - Pubkey аккаунта ставки для запроса в виде строки в кодировке base-58
* `<object>` - (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  * (опционально) [Обязательство](jsonrpc-api. md#configuring-state-commitment)
  * (необязательно) ` эпоха: <u64> ` - эпоха, для которой нужно рассчитать детали активации. Если параметр не указан, по умолчанию используется текущая эпоха.

#### Результат:

Результатом будет объект JSON со следующими полями:

* ` state: <string` - состояние активации лицевого счета, одно из: ` активный `, ` неактивный `, ` активация `, `деактивация `
* ` active: <u64> ` - ставка активна в эпоху
* ` inactive: <u64> ` - ставка неактивна в эпоху

#### Пример:
Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getStakeActivation", "params": ["CYRJWqiSjLitBAcRxPvWpgX3s5TvmN2SuRY3eEYypFvT"]}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":{"active":197717120,"inactive":0,"state":"active"},"id":1}
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

- `< object>` - (необязательно) [ Обязательство ](jsonrpc-api. md#configuring-state-commitment)

#### Результаты:

Результатом будет объект JSON RpcResponse со значением ` value `, равным объекту JSON, содержащему:

- ` total: <u64> ` - Общее количество в лэмпортах
- ` Циркуляционный: <u64> ` - Циркуляция в лэмпортах
- ` nonCirculating: <u64> `Не в обращении в лэмпортах
- ` nonCirculatingAccounts: <array> ` - массив адресов аккаунтов необращающихся аккаунтов в виде строк

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

Возвращает баланс токена аккаунта токена SPL. **НЕПОСТОЯННЫЙ**

#### Параметры:

- `<string>` - Pubkey аккаунта токена для запроса в виде строки в кодировке base-58
- `< object>` - (опционально) [Обязательство](jsonrpc-api. md#configuring-state-commitment)

#### Результат:

Результатом будет объект JSON RpcResponse со значением ` value `, равным объекту JSON, содержащему:

- `uiAmount: < f64>` - баланс, используя минимальные десятичные дроби
- ` amount: <string> ` - исходный баланс без десятичных знаков, строковое представление u64
- ` decimals: <u8> ` - количество десятичных цифр справа от десятичного разряда

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
      "uiAmount": 98.64,
      "amount": "9864",
      "decimals": 2
    },
    "id": 1
  }
}
```

### getTokenAccountsByDelegate

Возвращает все аккаунты токенов SPL утвержденным делегатом. **НЕПОСТОЯННЫЙ**

#### Параметры:

- `<string>` - Pubkey аккаунта делегата для запроса в виде строки в кодировке base-58
- `<object>` - Либо:
  * `мин: < string>` - Pubkey конкретного токена Mint для ограничения аккаунтов в виде base-58; или
  * ` programId: <string> ` - Pubkey идентификатора Token программы, которой принадлежат аккаунту, в виде строки в кодировке base-58
- `<object>` - (необязательно) объект конфигурации, содержащий следующие необязательные поля:
  - (опционально) [Обязательство](jsonrpc-api. md#configuring-state-commitment)
  - ` encoding: <string> ` - кодировка данных Пользователя, либо "base58" (* slow *), "base64", "base64 + zstd" или "jsonParsed". Кодировка "jsonParsed" пытается использовать программно-зависимые парсеры состояния для возврата более понятных и явных данных о состоянии аккаунта. Если запрошен «jsonParsed», но не может быть найден корректный mint для конкретного аккаунта, этот аккаунт будет отфильтрован по результатам.
  - (необязательно) ` dataSlice: <object> ` - ограничить возвращаемые данные аккаунта с помощью предоставленных полей ` offset: <usize> ` и ` length: <usize> `; доступно только для кодировок «base58», «base64» или «base64 + zstd».

#### Результат:

Результатом будет объект JSON RpcResponse со значением ` value `, равным массиву объектов JSON, который будет содержать:

- ` pubkey: <string> ` - аккаунт Pubkey в виде строки в кодировке base-58
- ` account: <object> ` - объект JSON со следующими подполями:
   - ` lamports: <u64> `, количество лэмпортов, назначенных аккаунту, как u64
   - `owner: <string>`, Pubkey в кодировке Base-58 программы, которой назначена аккаунту
   - ` data: <object> `, данные о состоянии токена, связанных с аккаунтом, либо в виде закодированных двоичных данных, либо в формате JSON ` {<program>: <state>} `
   - `executable: <bool>`, логическое значение, указывающее, содержит ли аккаунт программу \ (и предназначена ли она только для чтения \)
   - `rentEpoch: < u64>`, эпоха, в которой этот аккаунт будет сдан в аренду, как u64

#### Пример:

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

### getTokenAccountsByOwner

Возвращает все аккаунты SPL Token по владельцу токена. **НЕПОСТОЯННЫЙ**

#### Параметры:

- ` & lt; string & gt; ` - Pubkey аккаунта для запроса в виде строки в кодировке base-58
- `< object>` - Либо:
  * `мин: < string>` - Pubkey конкретного токена Mint для ограничения аккаунтов в виде base-58; или
  * ` programId: <string> ` - Pubkey идентификатора Token программы, которой принадлежат аккаунту, в виде строки в кодировке base-58
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - (опционально) [Обязательство](jsonrpc-api. md#configuring-state-commitment)
  - ` encoding: <string> ` - кодировка данных аккаунта, либо "base58" (* slow *), "base64", "base64 + zstd" или "jsonParsed". Кодировка "jsonParsed" пытается использовать программно-зависимые парсеры состояния для возврата более понятных и явных данных о состоянии аккаунта. Если запрошен «jsonParsed», но не может быть найден корректный mint для конкретного аккаунта, этот аккаунт будет отфильтрован по результатам.
  - (опционально) `dataSlice: <object>` - ограничить возвращаемые данные аккаунта, используя предоставленные `offset: <usize>` и `length: <usize>` поля; доступно только для кодировок «base58», «base64» или «base64 + zstd».

#### Результат:

Результатом будет объект JSON RpcResponse со значением ` value `, равным массиву объектов JSON, который будет содержать:

- `pubkey: <string>` - Pubkey аккаунта в виде строки в кодировке base-58
- ` account: <object> ` - объект JSON со следующими подполями:
   - ` lamports: <u64> `, количество лэмпортов, назначенных аккаунту, как u64
   - `owner: <string>`, Pubkey в кодировке base-58 программы, которой назначена эта учетная запись
   - `data: <object>`, данные состояния токена, связанные с учетной записью, в виде закодированных двоичных данных или в формате JSON `{<program>: <state>}`
   - `executable: <bool>`, логическое значение, указывающее, содержит ли аккаунт программу \ (и предназначена ли она только для чтения \)
   - `rentEpoch: < u64>`, эпоха, в которой этот аккаунт будет сдан в аренду, как u64

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

### getTokenLargestAccounts

Возвращает 20 самых больших аккаунтов определенного типа SPL Токена. **НЕПОСТОЯННЫЙ**

#### Параметры:

- `<string>` - Pubkey аккаунта токена для запроса в виде строки в кодировке base-58
- `<object>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Результатом будет объект JSON RpcResponse со значением ` value `, равным объекту JSON, содержащему:

- `address: <string>` - адрес токена аккаунта
- `uiAmount: <f64>` - баланс токена с использованием десятичных дробей
- ` amount: <string> ` - исходный баланс без десятичных знаков, строковое представление u64
- ` decimals: <u8> ` - количество десятичных цифр справа от десятичного разряда

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

### getTokenSupply

Возвращает общее количество токена SPL. **НЕПОСТОЯННЫЙ**

#### Параметры:

- `<string>` - Pubkey аккаунта токена для запроса в виде строки в кодировке base-58
- `<object>` - (необязательно) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Результатом будет объект JSON RpcResponse со значением ` value `, равным объекту JSON, содержащему:

- `uiAmount: < f64>` - баланс, используя минимальные десятичные дроби
- ` amount: <string> ` - исходный баланс без десятичных знаков, строковое представление u64
- ` decimals: <u8> ` - количество десятичных цифр справа от десятичного разряда

#### Примеры:

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
      "uiAmount": 1000,
      "amount": "100000",
      "decimals": 2
    }
  },
  "id": 1
}
```

### getTransactionCount

Возвращает текущее количество транзакций из реестра

#### Параметры:

- `<object>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

- ` & lt; u64 & gt; ` - подсчет

#### Пример:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getTransactionCount"}
'

```

Результат:
```json
{"jsonrpc":"2.0","result":268,"id":1}
```

### getVersion

Возвращает текущие версии соланы, запущенные на узле

#### Параметры:

None

#### Результат:

Поле результата будет объектом JSON со следующими полями:

- `solana-core`, программное обеспечение версии solana-core
- `feature-set`, уникальный идентификатор набора функций текущего программного обеспечения

#### Пример:

Запрос:
```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"getVersion"}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":{"solana-core": "1.6.0"},"id":1}
```

### getVoteAccounts

Возвращает информацию об учетной записи и связанную ставку для всех аккаунтов для голосования в текущем банке.

#### Параметры:

- `<object>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

Поле результата будет JSON объектом `текущих` и `преступных` аккаунтов, каждый из них содержит массив JSON объектов со следующими подполями:

- ` pubkey: <string> ` - аккаунт Pubkey в виде строки в кодировке base-58
- `pubkey: <string>` - публичный ключ узла, как строка в кодировке base-58
- `активированное: <u64>` - ставка, делегированная аккаунта для голосования и активная в эту эпоху
- `epochVoteAccount: <bool>` - bool, будет ли аккаунт для голосования делать ставку для этой эпохи
- `commission: <number>`,процент (0-100) вознаграждений выплачиваемых за счет голосования
- `lastVote: <u64>`- Последние голоса проголосовали за этот аккаунт
- `epochCredits: <array>` - история заработанных кредитов к концу каждой эпохи в виде массива массивов, содержащих: `[epoch, credits, previousCredits]`

#### Пример:
Запрос:
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
          [ 1, 64, 0 ],
          [ 2, 192, 64 ]
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

### minimumLedgerSlot

Возвращает самый нижний слот, о котором узел имеет информацию в реестре. Это значение может увеличиться со временем, если узел настроен на очистку старых данных

#### Параметры:

None

#### Результат:

- `u64` - Минимальный слот реестра

#### Пример:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"minimumLedgerSlot"}
'

```

Результат:
```json
{"jsonrpc":"2.0","result":1234,"id":1}
```

### requestAirdrop

Запрашивает раздачу лэмпортов в Pubkey

#### Параметры:

- `<string>` - Pubkey аккаунта для запроса в виде строки в кодировке base-58
- `<integer>` - лэмпорты, как u64
- `<object>` - (опционально) [ Обязательство ](jsonrpc-api.md#configuring-state-commitment) (используется для получения хэша блоков и проверки успешности раздачи)

#### Результат:

- `<string>` - Подпись транзакции в виде строки в кодировке base-58

#### Пример:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"requestAirdrop", "params":["83astBRguLMdt2h5U1Tpdq5tjFoJ6noeGwaY3mDLVcri", 50]}
'

```

Результат:
```json
{"jsonrpc":"2.0","result":"5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnbJLgp8uirBgmQpjKhoR4tjF3ZpRzrFmBV6UjKdiSZkQUW","id":1}
```

### sendTransaction

Выдает подписанную транзакцию кластеру для обработки.

Этот метод никоим образом не изменяет транзакцию; он передает транзакция, созданная клиентами для узла, как есть.

Если служба rpc узла получает транзакцию, этот метод немедленно успешно, не дожидаясь каких-либо подтверждений. Успешный ответ от этого метода не гарантирует, что транзакция будет обработана или подтверждена кластером.

Хотя служба rpc будет разумно пытаться отправить ее, транзакция может быть отклоненf, если транзакция ` latest_blockhash ` истекает до того, как она завершиться.

Используйте [`getSignatureStatuses`](jsonrpc-api.md#getsignaturestatuses) для обеспечения того, чтобы транзакция была обработана и подтверждена.

Перед отправкой выполняются следующие предварительные проверки:

1. Подписи транзакций проверены
2. Транзакция моделируется относительно банковского слота, указанного в предначальном обязательстве. При ошибке будет возвращена ошибка. При желании предварительные проверки могут быть отключены. Рекомендуется указать те же обязательства и предначальных обязательств во избежание путаницы.

Возвращенная подпись - это первая подпись в транзакции, которая используется для идентификации транзакции ([transaction id](../../terminology.md#transanction-id)). Этот идентификатор может быть легко извлечен из данных транзакции до отправки.

#### Параметры:

- `<string>` - полностью подписанная транзакция, как строка в кодировке
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - `skipPreflight: <bool>` - если включено, пропустите проверку транзакций preflight (по умолчанию: false)
  - `preflightCommitment: <string>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment) для использования в preflight (по умолчанию: `"max"`).
  - `encoding: <string>` - (необязательно) кодировка, используемая для данных транзакции. Либо `"base58"` (*slow*, **УСТАРЕЛО**), либо `"base64"`. (по умолчанию: `"base58"`).

#### Результат:

- `<string>` - Подпись первой транзакции, встроенная в транзакцию, как строка с исходной кодировкой ([id транзакции](../../terminology.md#transanction-id))

#### Пример:

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

Результат:
```json
{"jsonrpc":"2.0","result":"2id3YC2jK9G5Wo2phDx4gJVAew8DcY5NAojnVuao8rkxwPYPe8cSwE5GzhEgJA2y8fVjDEo6iR6ykBvDxrTQrtpb","id":1}
```

### simulateTransaction

Имитация отправки транзакции

#### Параметры:

- `<string>` - транзакция в виде закодированной строки. Транзакция должна иметь действительный блокхэш, но не требуется для подписания.
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - `sigVerify: <bool>` - если подлинность подписи транзакции будут проверены (по умолчанию: false)
  - `preflightCommitment: <string>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment) для использования в preflight (по умолчанию: `"max"`).
  - `encoding: <string>` - (необязательно) кодировка, используемая для данных транзакции. Либо `"base58"` (*slow*, **УСТАРЕЛО**), либо `"base64"`. (по умолчанию: `"base58"`).

#### Результат:

RpcResponse, содержащий объект TransactionStatus В результате будет объект RpcResponse JSON с значением `` задан объект JSON со следующими полями:

- `err: <object | string | null>` - ошибка, если транзакция не удалась, null, если транзакция прошла успешно. [ Определения TransactionError ](https://github.com/solana-labs/solana/blob/master/sdk/src/transaction.rs#L24)
- `logs: <array | null>` - Array of log messages the transaction instructions during execution, null если симуляция не удалась до возможности выполнения транзакции (например, из-за некорректного хэша блоков или проверки подписи)

#### Пример:

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

Результат:
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

### setLogFilter

Устанавливает фильтр журнала на валидатор

#### Параметры:

- `<string>` - новый фильтр журнала для использования

#### Результат:

- `<null>`

#### Пример:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"setLogFilter", "params":["solana_core=debug"]}
'
```

Результат:
```json
{"jsonrpc":"2.0","result":null,"id":1}
```

### validatorExit

Если валидатор загружается с включенным выходом RPC (`--enable-rpc-exit` parameter), этот запрос вызывает выход валидатора.

#### Параметры:

None

#### Результат:

- `<bool>` - успешная ли операция выхода валидатора

#### Пример:

```bash
curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
  {"jsonrpc":"2.0","id":1, "method":"validatorExit"}
'

```

Результат:
```json
{"jsonrpc":"2.0","result":true,"id":1}
```

## Subscription Websocket

После подключения к RPC PubSub websocket на `ws://<ADDRESS>/`:

- Отправлять запросы на подписку в веб-сокет методами ниже
- Несколько подписок могут быть активными сразу
- Многие подписки принимают опциональный параметр [`обязательства`](jsonrpc-api.md#configuring-state-commitment), определяющий, насколько завершен процесс изменения должен быть запущено уведомление. Для подписок, если обязательство не указано, значение по умолчанию - ` "singleGossip" `.

### accountSubscribe

Подпишитесь на аккаунт, чтобы получать уведомления, когда лэмпорт или данные для определенного аккаунта изменен публичный ключ

#### Параметры:

- `pubkey: < string>` - Pubkey аккаунта в виде строки в кодировке base-58
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - `< object>` - (опционально) [Обязательство](jsonrpc-api. md#configuring-state-commitment)
  - ` encoding: <string> ` - кодировка данных аккаунта, либо "base58" (* slow *), "base64", "base64 + zstd" или "jsonParsed". Кодировка "jsonParsed" пытается использовать программно-зависимые парсеры состояния для возврата более понятных и явных данных о состоянии аккаунта. Если запрошен "jsonParsed", но парсер не найден, поле возвращается в кодировку "base64", обнаруживается, когда поле `данных` типа `<string>`.

#### Результат:

- `< integer>` - Идентификатор Подписки \(необходим для отказа от подписки\)

#### Пример:

Запрос:
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

Результат:
```json
{"jsonrpc": "2.0","result": 23784,"id": 1}
```

#### Формат уведомления:

Кодировка Base58:
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
        "data": ["11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR", "base58"],
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

Кодировка-JSON:
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

Отписаться от уведомлений

#### Параметры:

- `< integer>` - id подписки для отмены

#### Результат:

- `<bool>` - сообщение об отмене подписки

#### Пример:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"accountUnsubscribe", "params":[0]}

```

Результат:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### logsSubscribe

Отписаться от журнала транзакции.  **НЕПОСТОЯННЫЙ**

#### Параметры:

- `фильтр: <string>|<object>` - фильтры для получения результатов по типу аккаунта; в настоящее время поддерживаются:
  - «all» - подписаться на все транзакции, кроме транзакций простого голосования
  - "allWithVotes" - подписаться на все транзакции, включая простые транзакции голосования
  - `{ "mentions": [ <string> ] }` - подписаться на все транзакции, в которых упоминается предоставленный Pubkey (как строка в кодировке base-58)
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

- `<integer>` - Id подписки для отмены \(необходим для отказа от подписки \)

#### Пример:

Запрос:
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

Результат:
```json
{"jsonrpc": "2.0","result": 24040,"id": 1}
```

#### Формат уведомления:

Кодировка Base58:
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

Отписаться от журнала транзакции

#### Параметры:

- `< integer>` - id подписки для отмены

#### Результаты:

- `< bool>` - сообщение об отмене подписки

#### Примеры:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"logsUnsubscribe", "params":[0]}

```

Результат:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### programSubscribe

Подпишитесь на программу, чтобы получать уведомления при изменении лэмпортов или данных для данного аккаунта, принадлежащей программе

#### Параметры:

- `pubkey: < string>` - Pubkey аккаунта в виде строки в кодировке base-58
- `<object>` - (опционально) объект конфигурации, содержащий следующие необязательные поля:
  - (опционально) [Обязательство](jsonrpc-api. md#configuring-state-commitment)
  - ` encoding: <string> ` - кодировка данных аккаунта, либо "base58" (* slow *), "base64", "base64 + zstd" или "jsonParsed". "jsonParsed" кодировка пытается использовать парсеры специфичных для программы состояний для возврата более читаемых и явных данных о состоянии аккаунта. Если запрошен "jsonParsed", но парсер не найден, поле возвращается в кодировку "base64", обнаруживается, когда поле `данных` типа `<string>`.
  - (опционное) `filters: <array>` - фильтровать результаты, используя различные [filter objects](jsonrpc-api.md#filters); аккаунт должен соответствовать всем критериям фильтрации для включения в результаты

#### Результат:

- `< integer>` - Идентификатор Подписки \(необходим для отказа от подписки\)

#### Пример:

Запрос:
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

Результат:
```json
{"jsonrpc": "2.0","result": 24040,"id": 1}
```

#### Формат уведомления:

Кодировка Base58:
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
          "data": ["11116bv5nS2h3y12kD1yUKeMZvGcKLSjQgX6BeV7u1FrjeJcKfsHPXHRDEHrBesJhZyqnnq9qJeUuF7WHxiuLuL5twc38w2TXNLxnDbjmuR", "base58"],
          "executable": false,
          "lamports": 33594,
          "owner": "11111111111111111111111111111111",
          "rentEpoch": 636
        },
      }
    },
    "subscription": 24040
  }
}
```

Кодировка-JSON:
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
        },
      }
    },
    "subscription": 24040
  }
}
```

### programUnsubscribe

Отписаться от уведомлений

#### Параметры:

- `< integer>` - id подписки для отмены

#### Результат:

- `<bool>` - сообщение об отмене подписки

#### Пример:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"programUnsubscribe", "params":[0]}

```

Результат:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### signatureSubscribe

Подпишитесь на подпись транзакции, чтобы получать уведомление при подтверждении транзакции. На `signatureNotification` подписка автоматически отменяется

#### Параметры:

- `<string>` - Подпись транзакции в виде строки в кодировке base-58
- `<object>` - (опционально) [Обязательство](jsonrpc-api.md#configuring-state-commitment)

#### Результат:

- `< integer>` - id подписки для отмены \(необходим для отказа от подписки \)

#### Пример:

Запрос:
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

Результат:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Формат уведомления:
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

Отписаться от уведомления о подтверждении подписи

#### Параметры:

- `<integer>` - id подписки для отмены

#### Результат:

- `<bool>` - сообщение об отмене подписки

#### Пример:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"signatureUnsubscribe", "params":[0]}

```

{"jsonrpc": "2.0","result": true,"id": 1}:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### slotSubscribe

Подпишитесь, чтобы получать уведомления в любое время, когда слот обрабатывается валидатором

#### Параметры:

None

#### Результат:

- `< integer>` - Идентификатор Подписки \(необходим для отказа от подписки\)

#### Пример:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"slotSubscribe"}

```

Результат:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Формат уведомления:

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

Отписаться от уведомлений о слотах

#### Параметры:

- `<integer>` - id подписки для отмены

#### Результат:

- `< bool>` - сообщение об отмене подписки

#### Пример:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"slotUnsubscribe", "params":[0]}

```

Результат:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### rootSubscribe

Подпишитесь, чтобы получать уведомления в любое время, когда слот обрабатывается валидатором.

#### Параметры:

None

#### Результат:

- `< integer>` - id подписки \(необходим для отказа от подписки \)

#### Пример:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"rootSubscribe"}

```

Результат:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Формат уведомления:

Результатом является последний номер корневого слота.

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

Отписаться от уведомлений о слотах

#### Параметры:

- `<integer>` - id подписки для отмены

#### Результат:

- `<bool>` - сообщение об отмене подписки

#### Примеры:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"rootUnsubscribe", "params":[0]}

```

Результат:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```

### voteSubscribe - Нестабильный, по умолчанию отключен

**Эта подписка нестабильна и доступна только в том случае, если валидатор был запущен с флагом ` --rpc-pubsub-enable-vote-subscription `.  Формат этой подписки может измениться в будущем**

Подпишитесь, чтобы получать уведомления в любое время, когда слот обрабатывается валидатором. Эти голоса являются предварительным консенсусом, поэтому нет гарантий, что эти голоса войдут в реестр.

#### Параметры:

None

#### Результат:

- `< integer>` - Идентификатор Подписки \(необходим для отказа от подписки\)

#### Пример:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"voteSubscribe"}

```

Результат:
```json
{"jsonrpc": "2.0","result": 0,"id": 1}
```

#### Формат уведомления:

Результатом является последнее голосование, содержащее хэш, список слотов для голосования, и необязательный временной штамп.

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

Отписаться от уведомлений

#### Параметры:

- `<integer>` - id подписки для отмены

#### Результат:

- `<bool>` - сообщение об отмене подписки

#### Пример:

Запрос:
```json
{"jsonrpc":"2.0", "id":1, "method":"voteUnsubscribe", "params":[0]}
```

Ответ:
```json
{"jsonrpc": "2.0","result": true,"id": 1}
```
