---
title: Совершение оффлайн-транзакций
---

По некоторые соображениям безопасности, хранение приватных ключей, и следовательно произведения процесса подписи, может осуществляться отдельно от создания транзакции и отправки её в сеть. Например:

- Сбор подписей от сторон, которые географически удалены друг от друга, как в случае с [мультиподписью](cli/usage.md#multiple-witnesses)
- Подписание транзакций с помощью устройства с [физической изоляцией от сетей](<https://en.wikipedia.org/wiki/Air_gap_(networking)>)

В этом документе описывается использование интерфейса командной строки (CLI) Solana для раздельного осуществления подписи и отправки транзакции.

## Команды поддерживающие оффлайн-подпись

В настоящее время следующие команды поддерживают оффлайн-подпись:

- [`create-stake-account`](cli/usage.md#solana-create-stake-account)
- [`deactivate-stake`](cli/usage.md#solana-deactivate-stake)
- [`delegate-stake`](cli/usage.md#solana-delegate-stake)
- [`split-stake`](cli/usage.md#solana-split-stake)
- [`stake-authorize`](cli/usage.md#solana-stake-authorize)
- [`stake-set-lockup`](cli/usage.md#solana-stake-set-lockup)
- [`transfer`](cli/usage.md#solana-transfer)
- [`withdraw-stake`](cli/usage.md#solana-withdraw-stake)

## Подпись транзакций оффлайн

Чтобы подписать транзакцию в оффлайн режиме, передайте следующие аргументы в командной строке

1. `--sign-only`, предотвращает отправку клиентом подписанной транзакции в сеть. Вместо этого пары pubkey/signature выводятся в стандартный поток вывода.
2. `--blockhash BASE58_HASH`, позволяет вызывающей стороне указать значение, используемое для заполнения поля `blockhash` в теле транзакции. Это служит нескольким целям, а именно: _ устраняет необходимость подключаться к сети и запрашивать хэш блока через RPC _ позволяет подписывающим сторонам координировать хеширование блоков в схеме с несколькими подписями

### Пример: подписание оффлайн-платежа

Команда

```bash
solana@offline$ solana pay --sign-only --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    recipient-keypair.json 1
```

Результат

```text

Blockhash: 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF
Signers (Pubkey=Signature):
  FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN

{"blockhash":"5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF","signers":["FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN"]}'
```

## Отправка подписанных оффлайн-транзакций в сеть

Чтобы отправить транзакцию, которая была подписана оффлайн, передайте следующие аргументы в командной строке

1. `--blockhash BASE58_HASH`, значение должно быть тем же, что использовалось для подписи
2. `--signer BASE58_PUBKEY=BASE58_SIGNATURE`, по одному для каждого оффлайн подписчика. Это включает пары pubkey / signature непосредственно в транзакцию, а не подписывает ее с помощью какой-либо локальной пары ключей

### Пример: отправка оффлайн-платежа

Команда

```bash
solana@online$ solana pay --blockhash 5Tx8F3jgSHx21CbtjwmdaKPLM5tWmreWAnPrbqHomSJF \
    --signer FhtzLVsmcV7S5XqGD79ErgoseCLhZYmEZnz9kQg1Rp7j=4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
    recipient-keypair.json 1
```

Результат

```text
4vC38p4bz7XyiXrk6HtaooUqwxTWKocf45cstASGtmrD398biNJnmTcUCVEojE7wVQvgdYbjHJqRFZPpzfCQpmUN
```

## Оффлайн-подпись в течении нескольких сессий

Создание оффлайн-подписи так же может осуществляться в течении нескольких сессий. В этом сценарии передайте открытый ключ отсутствующей стороны, полученный в паре pubkey/signature, для каждой роли. Все ключи pubkeys, которые были указаны, но для которых не была сгенерирована подпись, будут указаны как отсутствующие в выходных данных оффлайн-подписи

### Пример: Перевод с двумя оффлайн-подписями

Команда (Сессия #1)

```text
solana@offline1$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair fee_payer.json \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

Результат (Сессия #1)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
Absent Signers (Pubkey):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL
```

Команда (Сессия #2)

```text
solana@offline2$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --sign-only \
    --keypair from.json \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

Результат (Сессия #2)

```text
Blockhash: 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc
Signers (Pubkey=Signature):
  674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ
Absent Signers (Pubkey):
  3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy
```

Команда (Отправка онлайн, тот же пример)

```text
solana@online$ solana transfer Fdri24WUGtrCXZ55nXiewAj6RM18hRHPGAjZk3o6vBut 10 \
    --blockhash 7ALDjLv56a8f6sH6upAZALQKkXyjAwwENH9GomyM8Dbc \
    --from 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL \
    --signer 674RgFMgdqdRoVtMqSBg7mHFbrrNm1h1r721H1ZMquHL=3vJtnba4dKQmEAieAekC1rJnPUndBcpvqRPRMoPWqhLEMCty2SdUxt2yvC1wQW6wVUa5putZMt6kdwCaTv8gk7sQ \
    --fee-payer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy \
    --signer 3bo5YiRagwmRikuH6H1d2gkKef5nFZXE3gJeoHxJbPjy=ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

Результат (Отправка онлайн)

```text
ohGKvpRC46jAduwU9NW8tP91JkCT5r8Mo67Ysnid4zc76tiiV1Ho6jv3BKFSbBcr2NcPPCarmfTLSkTHsJCtdYi
```

## Увеличение времени для подписи

Как правило, транзакция Solana должна быть подписана и принята сетью в пределах определенного количества слотов, в соответствии с блокхэшем в поле `recent_blockhash` (~2 минуты на момент написания). Если создание подписи у вас заняло больше времени, то [транзакции с использованием Durable Nonce](offline-signing/durable-nonce.md) помогут решить эту проблему.
