---
title: Транзакции с использованием Durable Nonce
---

Durable Nonce — механизм который позволяет обойти ограничение срока валидности значения [`recent_blockhash`](developing/programming-model/transactions.md#recent-blockhash) в подписанной транзакции. Данное решение было имплементировано как Solana Program, о его принципах работы вы можете подробнее почитать в [предложении об улучшении](../implemented-proposals/durable-tx-nonces.md).

## Примеры использования

Полную информацию об использовании durable nonce с инструментами командной строки можно найти в разделе [Использование CLI](../cli/usage.md).

### Владелец одноразового аккаунта

Значение Durable Nonce хранится в специальном одноразовом аккаунте, право упавления которым может быть передано от одного владельца к другому. При этом, новый владелец наследует полный котроль над одноразовым аккаунтом от предыдущего владельца, включая и создателя аккаунта. Эта функция позволяет создавать более сложные схемы владения и получать производные адреса, которые не связаны с ключ-парой. Чтобы обозначить такой аккаунт используется аргумент `--nonce-authority <AUTHORITY_KEYPAIR>`, который поддерживается следующими командами:

- `create-nonce-account`
- `new-nonce`
- `withdraw-from-nonce-account`
- `authorize-nonce-account`

### Создание одноразового аккаунта

Механизм Durable Nonce использует специальный аккаунт для хранения следующего значения Nonce. Такой одноразовый аккаунт должен соответствовать критерию [rent-exempt](../implemented-proposals/rent.md#two-tiered-rent-regime), поэтому для этого требуется наличие минимального баланса.

Одноразовый аккаунт создается путём генерации новой ключ-пары и её инициализации в кластере

- Команда

```bash
solana-keygen new -o nonce-keypair.json
solana create-nonce-account nonce-keypair.json 1
```

- Результат

```text
2SymGjGV4ksPdpbaqWFiDoBz8okvtiik4KE9cnMQgRHrRLySSdZ6jrEcpPifW4xUpp4z66XM9d9wM48sA7peG2XL
```

> Если вы хотите хранить ключ-пару оффлайн, используйте для этого [бумажный кошелёк](wallet-guide/paper-wallet.md), а тут можно узнать больше информации о [генерации ключ-пары](wallet-guide/paper-wallet.md#seed-phrase-generation)

> [Более подробная инструкция в разделе CLI](../cli/usage.md#solana-create-nonce-account)

### Получение значения Durable Nonce

При подписании и отправке транзакции с использованием Durable Nonce, требуется передать хранимое в одноразовом аккаунте значения nonce в качестве значения для аргумента `--blockhash`. Получить текущее хранимое значение можно используя

- Команда

```bash
solana nonce nonce-keypair.json
```

- Результат

```text
8GRipryfxcsxN8mAGjy8zbFo9ezaUsh47TsPzmZbuytU
```

> [Более подробная инструкция в разделе CLI](../cli/usage.md#solana-get-nonce)

### Обновление хранимого значения Nonce

Хотя обычно это не требуется, значение nonce может быть обновлено

- Команда

```bash
solana new-nonce nonce-keypair.json
```

- Результат

```text
44jYe1yPKrjuYDmoFTdgPjg8LFpYyh1PFKJqm5SC1PiSyAL8iw1bhadcAX1SL7KDmREEkmHpYvreKoNv6fZgfvUK
```

> [Более подробная инструкция в разделе CLI](../cli/usage.md#solana-new-nonce)

### Вывод информации об одноразовом аккаунте

Получить информацию об одноразовом аккаунте в более человекопонятной форме можно используя

- Команда

```bash
solana nonce-account nonce-keypair.json
```

- Результат

```text
balance: 0.5 SOL
minimum balance required: 0.00136416 SOL
nonce: DZar6t2EaCFQTbUP4DHKwZ1wT8gCPW2aRfkVWhydkBvS
```

> [Более подробная инструкция в разделе CLI](../cli/usage.md#solana-nonce-account)

### Вывод средств с одноразового аккаунта

Вывести отправленные средства с одноразовго аккаунта можно с помощью

- Команда

```bash
solana withdraw-from-nonce-account nonce-keypair.json ~/.config/solana/id.json 0.5
```

- Результат

```text
3foNy1SBqwXSsfSfTdmYKDuhnVheRnKXpoPySiUDBVeDEs6iMVokgqm7AqfTjbk7QBE8mqomvMUMNQhtdMvFLide
```

> После вывода всех средств, одноразовый аккаунт перестает удовлетворять требованиям и будет закрыт

> [Более подробная инструкция в разделе CLI](../cli/usage.md#solana-withdraw-from-nonce-account)

### Переопределение владельца

Владельца одноразового аккаунта можно переопределить

- Команда

```bash
solana authorize-nonce-account nonce-keypair.json nonce-authority.json
```

- Результат

```text
3F9cg4zN9wHxLGx4c3cUKmqpej4oa67QbALmChsJbfxTgTffRiL3iUehVhR9wQmWgPua66jPuAYeL1K2pYYjbNoT
```

> [Более подробная инструкция в разделе CLI](../cli/usage.md#solana-authorize-nonce-account)

## Остальные команды с поддержкой Durable Nonce

Любые другие команды, которые поддерживают два следующих аргумента, могут работать с Durable Nonce.

- `--nonce`, указывает одноразовый аккаунт, где хранится значение Durable Nonce
- `--nonce-authority`, указывает на [владельца одноразового аккаунта](#nonce-authority), опционально

К настоящему времени, перечисленные команды поддерживают работу с Durable Nonce

- [`pay`](../cli/usage.md#solana-pay)
- [`delegate-stake`](../cli/usage.md#solana-delegate-stake)
- [`deactivate-stake`](../cli/usage.md#solana-deactivate-stake)

### Пример использования команды pay с использованием Durable Nonce

Тут продемонстрированна оплата 1 SOL Бобу со сторны Алисы, где Алиса использует Durable Nonce. Алгоритм действий идентичен для любых комманд с поддержкой Durable Nonce

#### - Создание аккаунтов

Сначала нам нужно создать ключ-пару для аккаунтов Алисы, одноразового аккаунта, которым будет владеть Алиса и аккаунт для Боба

```bash
$ solana-keygen new -o alice.json
$ solana-keygen new -o nonce.json
$ solana-keygen new -o bob.json
```

#### - Пополнение счёта аккаунта Алисы

Для создания одноразового аккаунта требуется наличие минимальной суммы на счёту для вызова контракта. Также нам необходимы токены для отправки Бобу и оплаты транзакций. Отправим ей путем аирдропа немного SOL

```bash
$ solana airdrop -k alice.json 10
10 SOL
```

#### - Создадим одноразовый

Нам необходим одноразовый аккаунт, которым будет владеть только Алиса. Мы уже создали для него ключ-пару, осталось только проинициализировать его вызвав контракт

> В данном случае, мы не указываем [владельца одноразового аккаунта](#nonce-authority), поэтому Алиса с ключ-парой `alice.json` имеет полный контроль над ним

```bash
$ solana create-nonce-account -k alice.json nonce.json 1
3KPZr96BTsL3hqera9up82KAU462Gz31xjqJ6eHUAjF935Yf8i1kmfEbo6SVbNaACKE5z6gySrNjVRvmS8DcPuwV
```

#### - Первый блин комом

Алиса попыталась отправить Бобу средства, но подпись заняла слишком много времени. Срок валидности указанного blockhash истек и транзакция была отменена сетью

```bash
$ solana pay -k alice.json --blockhash expiredDTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 bob.json 1
[2020-01-02T18:48:28.462911000Z ERROR solana_cli::cli] Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
Error: Io(Custom { kind: Other, error: "Transaction \"33gQQaoPc9jWePMvDAeyJpcnSPiGUAdtVg8zREWv4GiKjkcGNufgpcbFyRKRrA25NkgjZySEeKue5rawyeH5TzsV\" failed: None" })
```

#### - Durable Nonce для спасения!

В этот раз Алиса делает повторную попытку, но указывает свой одноразовый аккаунт и значение blockhash, которое там храниться

> Помните, что в данном случае Алиса, через свою ключ-пару `alice.json` подтверждает, что она является [владельцем](#nonce-authority) одноразового аккаунта

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.00136416 SOL
nonce: F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7
```

```bash
$ solana pay -k alice.json --blockhash F7vmkY3DTaxfagttWjQweib42b6ZHADSx94Tw8gHx3W7 --nonce nonce.json bob.json 1
HR1368UKHVZyenmH7yVz5sBAijV6XAPeWbEiXEGVYQorRMcoijeNAbzZqEZiH8cDB8tk65ckqeegFjK8dHwNFgQ
```

#### - Успех!

Транзакция прошла успешно! Боб получает на свой счёт 1 SOL, а хранимое значение nonce в одноразовом аккаунте, пренадлежащего Алисе, обновляется и готово для использования в следующей транзакции

```bash
$ solana balance -k bob.json
1 SOL
```

```bash
$ solana nonce-account nonce.json
balance: 1 SOL
minimum balance required: 0.00136416 SOL
nonce: 6bjroqDcZgTv6Vavhqf81oBHTv3aMnX19UTB51YhAZnN
```
