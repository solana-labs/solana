---
title: Интеграция Solana с вашей биржей
---

В этом руководстве описано, как добавить поддержку нативных SOL токенов в вашей криптовалютной бирже.

## Настройка ноды

Мы настоятельно рекомендуем настраивать минимум две ноды, используя качественное оборудование или облачные инстансы, регулярно обновлять ПО и следить за состоянием работы сервисов с помощью поставляемых инструментов мониторинга.

Такой подход позволит вам:

- иметь надежный шлюз к кластеру основной сети Solana mainnet-beta для получения данных и отправки транзакций вывода
- иметь полный контроль над объемом хранимых данных истории блокчейна
- поддерживать работу сервиса, даже если одни из узлов выйдет из строя

Узлы Solana требуют относительно высокой вычислительной мощности, чтобы работать с нашими быстрыми блоками и высоким рейтом TPS. Детальная информация по требованиям к оборудованию на странице [Требования к валидатору](../running-validator/validator-reqs.md).

Запуск API-ноды:

1. [Установите инструменты командной строки Solana](../cli/install-solana-cli-tools.md)
2. Запустите валидатор по крайней мере со следующими параметрами:

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

Передайте в качестве параметра `--ledger` желаемое место хранения реестра в вашей файловой системе. Аргумент `--rpc-port` порт, который будет открыт для взаимодействия с нодой.

Параметры `--entrypoint` и `--expected-genesis-hash` являются специфичными для каждого конкретного кластера. [Текущие параметры для работы в кластере Mainnet Beta](../clusters.md#example-solana-validator-command-line-2)

Параметр `--limit-ledger-size` позволяет указать какое число [шредов](../terminology.md#shred) будет сохраняться на диске. Без этого параметра валидатор будет поддерживать сохранение всего реестра до тех пор, пока не закончится место на диске. Значение по умолчанию ограничивает размер реестра до 500ГБ. Если это необходимо, можно запросить большее или меньшее пространства, добавив аргумент в параметр `--limit-ledger-size`. С помощью команды `solana-validator --help` можно посмотреть значение по-умолчанию для параметра `--limit-ledger-size`. Больше информации о подборе лимита выделяемого пространства для хранения реестра можно найти [тут](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

Параметр `--trusted-validator` позволяет защитить вашу ноду от синхронизации истории реестра и генезис блока с вредоносными узлами. [Подробнее о значении доверенных узлов](../running-validator/validator-start.md#trusted-validators)

Необязательные параметры:

- `--private-rpc` препятствует расскрытию вашего RPC порта для использования другими нодами
- `--rpc-bind-address` позволяет указать другой IP адрес для привязки RPC порта

### Автоматический перезапуск и мониторинг

Мы рекомендуем настроить каждый из ваших узлов на автоматическую перезагрузку при выходе из строя, чтобы быть уверенным, что вы пропустили как можно меньше данных. Запуск ноды Solana в качестве системной службы является хорошим решением.

Для мониторинга за состоянием ноды, мы предоставляем [`solana-watchtower`](https://github.com/solana-labs/solana/blob/master/watchtower/README.md), который способен следить за процессами `solana-validator` и определять, если что-то работает некорректно. Вы можете настроить его для оповещений прямо через Slack, Telegram, Discord или Twillio. Для получения подробной информации запустите `solana-watchtower --help`.

```bash
solana-watchtower --validator-identity <YOUR VALIDATOR IDENTITY>
```

#### Новые релизы

Мы выпускаем новые релизы довольно часто (приблизительно 1 релиз в неделю). Иногда, новые версии включают в себя несовместимые изменения протокола, которые требуют своевременного обновления ПО на узлах, чтобы избежать ошибок в обработке блоков.

Наши официальные анонсы по всем типам релизов (обычные и связанные с безопасностью) распростроняются через канал в Discord [`#mb-announcement`](https://discord.com/channels/428295358100013066/669406841830244375) (`mb` означает `mainnet-beta`).

Как и в случае с валидаторами, мы ожидаем, что любые узлы, работающие с биржой, будут обновлены при первой же возможности в течение одного или двух рабочих дней после анонса нового релиза. Для релизов связанных с безопасностью может потребоваться принятие более срочных действий.

### Целостность реестра

По-умолчанию, каждая ваша нода будет загружать снепшот предоставляемый одним из доверенных валидаторов. Этот снепшот отражает текущее состояние блокчейна, но не содержит всей истории реестра. Если у одного из ваших узлов произойдет сбой и при перезагрузке он загрузит новый снепшот, может получиться так, что в реестре окажутся пропуски в последовательности блоков. Чтобы предотвратить подобный исход, воспользуйтесь параметром `--no-snapshot-fetch` при запуске команды `solana-validator`, так вы получите все исторические данные реестра вместо снепшота.

Не передавайте параметр `--no-snapshot-fetch` при начальном старте узла, потому как загрузить все исторические данные от генезис-блока невозможно. Вместо этого, запустите узел сначала используя данные снепшота, а зате перезагрузите с параметром `--no-snapshot-fetch`.

Важно отметить, что объем исторических данных реестра, предоставляемых вашему узлу другими участниками сети, ограничен в любой момент времени. Если ваши валидаторы испытывают значительное время простоя после ввода в эксплуатацию, они, возможно, не смогут подключиться к сети, и им потребуется загрузить новый снепшот от доверенного валидатора. При этом ваши валидаторы будут иметь пробелы в исторических данных реестра, которые невозможно заполнить.

### Ограничение доступности валидатора

Валидатор требует, чтобы различные UDP и TCP порты были открыты для входящего трафика от остальных валидаторов Solana. Хотя это и наиболее эффективный режим работы, использовать который настоятельно рекомендуется, можно ограничить работу валидатор, чтобы он пропускал входящий трафик только от одного валидатора.

Сначала добавьте аргумент `--restricted-repair-only-pair-mode`. Это заставит валидатор работать в ограниченном режиме, в котором он не будет получать данных от остальных валидаторов, а вместо этого должен будет постоянно опрашивать другие валидаторы на предмет наличия новых блоков. Валидатор будет передавать только UDP пакеты используя _Gossip_ и _Repair_ ("serve repair") порты, и также будет получать только UDP пакеты на свои собственные _Gossip_ и _Repair_ порты.

_Gossip_ порт является двунаправленным и позволяет вашему валидатору оставаться в контакте с остальным кластером. Ваш валидатор передает на _ServeR_ запросы для получения пропущенных блоков у остальной сети, поскольку механизм распространения блоков Turbine теперь отключен. Затем ваш валидатор получит ответы от остальных валидаторов на тот же _RepaiR_ порт.

Чтобы еще больше ограничить работу валидатора и запрашивать блоки только у одного или нескольких определенных узлов, сначала определите идентификатор публичного ключа для этого узла и добавьте аргументы `--gossip-pull-validator PUBKEY --repair-validator PUBKEY` для каждого публичного ключа. Это приведет к тому, что ваш валидатор будет отнимать ресурсы у каждого добавленного узла, поэтому, пожалуйста, делайте это осторожно и только после консультации с владельцем целевого узла.

Теперь ваш валидатор будет взаимодействовать только с указанными узлами сети, используя только _Gossip_, _Repair_ and _ServeR_ порты.

## Настройка депозитных счетов

Система аккаунтов Solana не требует каких либо дополнительных ончейн инициализаций, как только на счету аккаунта появляются SOL он считается аткивным. Для создания депозитного счета вашей биржи, просто сгенерируйте ключ-пару, используя любой из доступных [инструментров](../wallet-guide/cli.md).

Мы рекомендуем использовать уникальный депозитный аккаунт для каждого вашего пользователя.

Аккаунты Solana требует отчисления незначительной [комиссии](developing/programming-model/accounts.md#rent) во время создания и далее каждый раз при смене эпохи, но их можно освободить от уплаты этих сборов, если держать на счету такого аккаунта запас монет SOL, равный сумме комиссии, уплаченной в ином случае, за период в два года. Чтобы узнать баланс необходимый для выполнения данного условия, необходимо выполнить запрос на [`getMinimumBalanceForRentExemption`ендпоинт](developing/clients/jsonrpc-api.md#getminimumbalanceforrentexemption):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[0]}' localhost:8899

{"jsonrpc":"2.0","result":890880,"id":1}
```

### Оффлайн аккаунты

Вы возможно захотите держать ключи для одного и более аккаунтов оффлайн, для большей безопасности. В таком случае, вам понадобиться перемещать SOL на горячие аккаунты, используя [оффлайн подписи](../offline-signing.md).

## Ожидание депозита

Когда пользователь хочет внести SOL на вашу биржу, попросите его отправить перевод на соответствующий адрес.

### Проверка блоков

Чтобы отслеживать все депозитные счета вашей биржи, опрашивайте каждый подтвержденный блок и проверяйте интересующие адреса, используя сервис JSON-RPC вашей Solana API ноды.

- Чтобы определить, какой блок сейчас доступен, отправьте [`getConfirmedBlocks` запрос](developing/clients/jsonrpc-api.md#getconfirmedblocks), передав значение последнего блока, который вы уже обработали в качестве параметра стартового слота:

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlocks","params":[5]}' localhost:8899

{"jsonrpc":"2.0","result":[5,6,8,9,11],"id":1}
```

Не каждый слот производит блок, поэтому могут быть пробелы в последовательности чисел.

- Получить содержимое каждого блока можно с помощью [запроса `getConfirmedBlock`](developing/clients/jsonrpc-api.md#getconfirmedblock):

```bash
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0","id":1,"method":"getConfirmedBlock","params":[5, "json"]}' localhost:8899

{
  "jsonrpc": "2.0",
  "result": {
    "blockhash": "2WcrsKSVANoe6xQHKtCcqNdUpCQPQ3vb6QTgi1dcE2oL",
    "parentSlot": 4,
    "previousBlockhash": "7ZDoGW83nXgP14vnn9XhGSaGjbuLdLWkQAoUQ7pg6qDZ",
    "rewards": [],
    "transactions": [
      {
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
        },
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
        }
      }
    ]
  },
  "id": 1
}
```

Поля `preBalances` и `postBalances` позволяют отслеживать изменения баланса для каждого аккаунта без необходимости парсинга всей транзакции. Они отображают начальные и конечные балансы [лэмпортах](../terminology.md#lamport). для каждого аккаунта перечисленного списке `accountKeys` в соответствии с индексом. Например, если депозитный счёт находится по адресу `47Sbuv6jL7CViK9F2NMW51aQGhfdpUu7WNvKyH645Rfi`, данная транзакция является отображением перевода 218099990000 - 207099990000 = 11000000000 лэмпортов = 11 SOL

Если вам нужна дополнительная информация о типе транзакции или других свойствах, вы можете запросить блок из RPC в двоичном формате и проанализировать его, используя наши [Rust SDK](https://github.com/solana-labs/solana), либо [Javascript SDK](https://github.com/solana-labs/solana-web3.js).

### История транзакций аккаунта

Вы также можете запросить историю транзакций определенного адреса. Обычно, это _не самый_ приемлемый метода для отслежевания всех ваших депозитных адресов, но может быть волне пригодным для изучения нескольких адресов за определенный период времени.

- Отправьте [`getConfirmedSignaturesForAddress2`](developing/clients/jsonrpc-api.md#getconfirmedsignaturesforaddress2) запрос к API ноде:

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

- Для каждой подписи, полученной в ответе, можно получить детальную информацию с помощью [`getConfirmedTransaction`](developing/clients/jsonrpc-api.md#getconfirmedtransaction) запроса:

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

## Вывод средств

Чтобы обеспечить выполнение запроса пользователя на вывод SOL, вы должны сгенерировать обычную транзакцию Solana и отправить её в API ноду, чтобы она дальше была отправлена в кластер.

### Синхронизированный перевод

Существует способ отправка транзакции, который позволят сразу же убедиться была ли она выполнена успешно и подтверждена кластером.

Инструменты командной строки Solana предоставляют такую команду: `solana transfer` генерирует, отправляет на API ноду и проверяет состояние транзакции. По-умолчанию, эта команда ждет и следит за прогрессом выполнения в потоке вывода ошибок stderr до тех пор, пока транзакция не будет подтверждена кластером. Если транзакция завершится неудачей, метод сообщит, передав в поток полученную ошибку.

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --allow-unfunded-recipient --keypair <KEYPAIR> --url http://localhost:8899
```

[Solana Javascript SDK](https://github.com/solana-labs/solana-web3.js) предоставляет похожий подход для экосистемы JS. Используйте `SystemProgram` для генерации транзакции, а `sendAndConfirmTransaction` для её отправки в сеть.

### Асинхронный перевод

Для большей гибкости можно асинхронно отправлять переводы. В таком случае, на вашей стороне остается ответственность за проверку состояния транзакции в кластере, была ли она успешно финализирована и записана в блок.

**Примечание:** Каждая транзакция содержит значение последнего [blockhash](developing/programming-model/transactions.md#blockhash-format), что обеспечивает валидность транзакции в течении некоторого периода времени ~2 мин на время написания. **Критически важно** дождаться пока валидность предыдущего blockhash истечет, прежде чем повторять попытку отправки транзакции на снятие средств, если предыдущая транзакция не была подтверждена и финализирована кластером. В противном случае, существует возможность произвести двойную трату. Более детальная информация на странице [срок валидности blockhash](#blockhash-expiration).

Для начала, давайте получим последний валидный blockhash, используя [`getFees` ендпоинт](developing/clients/jsonrpc-api.md#getfees) или используя инструменты CLI:

```bash
solana fees --url http://localhost:8899
```

В инструментах командной строки используйте `--no-wait` аргумент, чтобы отправить транзакцию асинхронно, и включить значение последнего blockhashe в аргумент `--blockhash`:

```bash
solana transfer <USER_ADDRESS> <AMOUNT> --no-wait --allow-unfunded-recipient --blockhash <RECENT_BLOCKHASH> --keypair <KEYPAIR> --url http://localhost:8899
```

Конечно, вы также можете собрать, подписать и сериализовать транзакцию вручную, а затем отправить её в кластер, используя JSON-RPC [ендпоинт `sendTransaction`](developing/clients/jsonrpc-api.md#sendtransaction).

#### Подтверждение транзакции & Финализация

Чтобы получить статус сразу нескольких транзакций, используйте [JSON-RPC эндпоинт `getSignatureStatuses`](developing/clients/jsonrpc-api.md#getsignaturestatuses). Поле `confirmations` сообщает, как много [подтвержденных блоков](../terminology.md#confirmed-block) было добыто, с тех пор как транзакция была обработана. Если значение `confirmations: null`, значит транзакция [подтвержденна и финализирована](../terminology.md#finality), т.е. записана в блок и не может быть отменена.

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

#### Срок валидности Blockhash

You can check whether a particular blockhash is still valid by sending a [`getFeeCalculatorForBlockhash`](developing/clients/jsonrpc-api.md#getfeecalculatorforblockhash) request with the blockhash as a parameter. If the response value is `null`, the blockhash is expired, and the withdrawal transaction using that blockhash should never succeed.

### Проверка пользовательских аккаунтов для вывода средств

Поскольку снятие средств необратимо, может быть хорошей практикой проверить указанный пользователем адрес учетной записи прежде чем санкционировать снятия, чтобы предотвратить случайную потерю средств пользователя.

#### Basic verfication

Solana addresses a 32-byte array, encoded with the bitcoin base58 alphabet. This results in an ASCII text string matching the following regular expression:

```
[1-9A-HJ-NP-Za-km-z]{32,44}
```

This check is insufficient on its own as Solana addresses are not checksummed, so typos cannot be detected. To further validate the user's input, the string can be decoded and the resulting byte array's length confirmed to be 32. However, there are some addresses that can decode to 32 bytes despite a typo such as a single missing character, reversed characters and ignored case

#### Advanced verification

Due to the vulnerability to typos described above, it is recommended that the balance be queried for candidate withdraw addresses and the user prompted to confirm their intentions if a non-zero balance is discovered.

#### Valid ed25519 pubkey check

Адрес типичного аккаунта Solana является строкой в кодировке Base58 c 256-битным публичным ключом стандарта Ed25519. Не все битовые комбинации являются валидными публичными ключами в схеме Ed25519, что дает возможность проверить предоставляемый пользователем адрес хотя бы на соответствие стандарту Ed25519.

#### Java

Пример такой проверки с использованием языка Java:

Следующий пример кода предполагает, что вы используете Maven для сборки пакетов.

`pom.xml`:

```xml
<repositories>
  ...
  <repository>
    <id>spring</id>
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

## Поддержка токенов SPL стандарта

[SPL токены](https://spl.solana.com/token) это стандарт для создания и обмена обернутых/синтетических токенов в блокчейне Solana.

Принцип работы с SPL токенами практически не отличается от такового с нативным SOL токеном, но есть несколько отличий, о которых мы поговорим в этом разделе.

### Выпуск токенов

Each _type_ of SPL Token is declared by creating a _mint_ account. В этом аккаунте хранятся метаданные, описывающие функции токенов, такие как эмиссия, количество знаков после запятой и адреса аккаунтов с правом контроля над эмитентом. Каждый SPL токен ссылается на связанный с ним эмитент аккаунт и может взаимодействовать только с токенами такого же типа.

### Установка CLI инструмента `spl-token`

Аккаунты SPL токенов запрашиваются и изменяются с помощью утилиты командной строки `spl-token`. Приведенные в этом разделе примеры, зависят от того, установлен ли данный инструмент в вашей локальной системе.

`spl-token` распространяется с помощью [crates.io](https://crates.io/crates/spl-token) посредством пакетного менеджера Rust `cargo`. Последняя актуальная версия `cargo`, может быть установленна на вашу платформу с помощью одной простой строчки доступной на [rustup.rs](https://rustup.rs). Как только `cargo` будет установлен, утилиту `spl-token` можно установить с помощью следующей команды:

```
cargo install spl-token-cli
```

Затем, вы можете проверить установленную версию командой

```
spl-token --version
```

Которая должна вернуть что-то подобное

```text
spl-token-cli 2.0.1
```

### Создание аккаунта

Аккаунты SPL токенов содержат дополнительные требования, которых не имеют нативные токены:

1. Аккаунт для SPL токенов должен быть создан прежде, чем на него можно будет отправить монеты. SPL аккаунт может быть создан явно с помощью команды `spl-token create-account`, или же неявно, используя команду `spl-token transfer --fund-recipient ...`.
1. Аккаунты SPL токенов должны удовлетворять требованию [rent-exempt](developing/programming-model/accounts.md#rent-exemption) в течении периода их использования, следовательно их создание требует небольшого количетсва нативных SOL токенов, взымаемых в качестве комисии. Для SPL токенов стандарта v2, это количество равняеется 0.00203928 SOL (2,039,280 лэмпортов).

#### Командная строка

Чтобы создать аккаунт для SPL токена со следующими свойствами:

1. Ассоциирован с переданным эмитент-аккаунтом
1. Принадлежит субсидирующему аккаунту из указанной ключ-пары

```
spl-token create-account <TOKEN_MINT_ADDRESS>
```

#### Образец

```
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

Пример создания аккаунта для SPL токенов с определенной ключ-парой:

```
$ solana-keygen new -o token-account.json
$ spl-token create-account AkUFCWTXb3w9nY2n6SFJvBV6VwvFUCe4KBMCcgLsa2ir token-account.json
Creating account 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 4JsqZEPra2eDTHtHpB4FMWSfk3UgcCVmkKkP7zESZeMrKmFFkDkNd91pKP3vPVVZZPiu5XxyJwS73Vi5WsZL88D7
```

### Проверка баланса аккаунта

#### Командная строка

```
spl-token balance <TOKEN_ACCOUNT_ADDRESS>
```

#### Образец

```
$ solana balance 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
0
```

### Отправка токенов

Исходный аккаунт с которого происходит отправка, это тот аккаунт где находятся SPL токены.

Адрес получателя, однако, может соответствовать нативному SOL аккаунту. Если у текущего аккаунта нет ассоциированного с эмитентом данного SPL токена адреса кошелька, он может быть создан во время транзакции отправки, только если каманде был передан аргумент `--fund-recipient`. Обратите внимание, что комиссия за создание адреса в таком случае будет изъята с отправителя.

#### Командная строка

```
spl-token transfer <SENDER_ACCOUNT_ADDRESS> <AMOUNT> <RECIPIENT_WALLET_ADDRESS> --fund-recipient
```

#### Образец

```
$ spl-token transfer 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN 1 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Transfer 1 tokens
  Sender: 6B199xxzw3PkAm25hGJpjj3Wj3WNYNHzDAnt1tEqg5BN
  Recipient: 6VzWGL51jLebvnDifvcuEDec17sK6Wupi4gYhm5RzfkV
Signature: 3R6tsog17QM8KfzbcbdP4aoMfwgo6hBggJDVy7dZPVmH2xbCWjEj31JKD53NzMrf25ChFjY7Uv2dfCDq4mGFFyAj
```

### Пополнение

Поскольку для каждой пары `(пользовательский аккаунт, эмитент аккаунт)` требуется отдельная учетная запись в цепочке, рекомендуется, чтобы биржа заранее создавала пакеты учетных записей для SPL токенов и назначала их пользователям по запросу. При этом, все эти аккаунты должны управляться аккаунтами, приватные ключи которых принадлежат бирже.

Для мониторинга входящих транзакций можно использовать метод [проверки блоков](#poll-for-blocks), описанный выше. Каждый новый блок, можно отсканировать на наличие успешных транзакций выдачи SPL токенов [Transfer](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L92) или [Transfer2](https://github.com/solana-labs/solana-program-library/blob/096d3d4da51a8f63db5160b126ebc56b26346fc8/token/program/src/instruction.rs#L252) с инструкциями отсылающими к пользовательским адресам, а затем запросить обновления [баланса токенов на счету аккаунта](developing/clients/jsonrpc-api.md#gettokenaccountbalance).

В настоящее время [рассматривается](https://github.com/solana-labs/solana/issues/12318) возможность расширения `preBalance` и `postBalance` полей в метаданных статуса транзакции, для включения туда изменений балансов с SPL токенами.

### Вывод средств

Адресс для снятия средств, предоставляемый пользователем, должен быть тем же, что используется для снятия обычных SOL токенов.

Перед тем как выполнить [перевод](#token-transfers) для снятия средств с баланса, биржа должна убедиться, что адрес является валидным как [описано выше](#validating-user-supplied-account-addresses-for-withdrawals).

Благодаря адресу, на который выводятся средства, определяется аккаунт для SPL токенов ассоциированный с тем же эмитент аккаунтом, что и адрес, откуда снимаются средства, туда и отправится переводимые SPL токены. Обратите внимание, что данный, ассоциированный аккаунт, может ещё не существовать, по этой причине бирже должна пополнять счёт от имени пользователя. Для аккаунтов SPL токенов v2, такая сумма составит 0.00203928 SOL (2,039,280 лэмпортов).

Шаблон команды `spl-token transfer` для снятия средств:

```
$ spl-token transfer --fund-recipient <exchange token account> <withdrawal amount> <withdrawal address>
```

### Другие рекомендации

#### Заморозка аккаунтов

For regulatory compliance reasons, an SPL Token issuing entity may optionally choose to hold "Freeze Authority" over all accounts created in association with its mint. Это позволяет по желанию [замораживать](https://spl.solana.com/token#freezing-accounts) средства, принадлежащие ассоциированному аккаунту, делая его непригодным для использования до тех пор, пока они не будут вновь разморожены. Если эта функция используется, публичный ключ регулирующего аккаунта, будет зарегистрирован в эмитент аккаунте выбранного SPL токена.

## Тестирование интеграции

Обязательно протестируйте все ваши процессы на [кластерах](../clusters.md) Devnet и Testnet прежде чем переносить их в кластер Mainnet-Beta. Devnet является наиболее открытым и гибким, и идеально подходит для начальной разработки, в то время как Testnet предлагает конфигурацию кластера приближенную к основной сети. Both devnet and testnet support a faucet, run `solana airdrop 1` to obtain some devnet or testnet SOL for developement and testing.
