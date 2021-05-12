---
title: Бумажный кошелек
---

Этот документ описывает, как создавать и использовать файловый кошелек с инструментами Solana CLI.

> В этом разделе мы не описываем как _безопасно_ создавать и управлять бумажными кошельками. Пожалуйста, внимательно изучите проблемы безопасности.

## Обзор

Solana предоставляет инструмент для получения ключей из мнемонической фразы, соответствующей стандарту BIP39. Команды Solana CLI для запуска валидатора и стейкинга токенов - все поддерживают ввод ключ-пары с помощью мнемо-фразы.

Чтобы узнать больше о стандарте BIP39, посетите [репозиторий Bitcoin BIP на Github](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki).

## Использование бумажного кошелька

Команды Solana можно запускать без сохранения ключ-пары на устройстве. Если вы обеспокоены сохранением вашего приватного ключа прямо на устройстве, то вы в нужном месте.

> Даже при использовании этого безопасного метода ввода все еще существует вероятность, что ваш приватный ключ будет записан на диск во время незашифрованной подкачки памяти. Ответственность за защиту от этого сценария лежит на пользователе.

## Прежде чем начать

- [Установите иснтурменты командной строки Solana](../cli/install-solana-cli-tools.md)

### Проверьте установку

Чтобы проверить, что `solana-keygen` установленны правилно, запустите команду:

```bash
solana-keygen --version
```

## Создание бумажного кошелька

Используя инструмент `solana-keygen`, можно сгенерировать новую мнемоническую фразу, так же как и извлечь ключ-пару из уже существующей мнемо-фразы (с опциональным шифрованием паролем). Мнемо-фраза и пароль, могут использоваться вместе в качестве бумажного кошелька. До тех пор, пока вы храните мнемо-фразу и пароль в безопасности, вы можете использовать их для доступа к своему аккаунту.

> Для получения дополнительной информации о том, как работают мнемонические фразы, ознакомьтесь с [Вики-страницей Bitcoin](https://en.bitcoin.it/wiki/Seed_phrase).

### Генерация мнемо-фразы

Создание нового ключа может быть выполнена с помощью команды `solana-keygen new`. Команда сгенерирует случайную мнемоническую фразу, попросит вас ввести необязательный пароль, а затем отобразит производный открытый ключ и сгенерированную мнемо-фразу для вашего бумажного кошелька.

После того, как вы запишете и сохраните вашу мнемо-фразу, вы можете воспользоваться командой [public key derivation](#public-key-derivation), чтобы убедиться, что вы не допустили где-то ошибку.

```bash
solana-keygen new --no-outfile
```

> Если флаг `--no-outfile` **не указан**, по-умолчанию будет произведено создание ключ-пары, которая будет записана в файл `~/.config/solana/id.json`, что фактически является [файловым кошельком](file-system-wallet.md)

Результатом работы команды, станет строка вроде этой:

```bash
pubkey: 9ZNTfG4NyQgxy2SWjSiQoUyBPEvXT2xo7fKc5hPYYJ7b
```

Значение, показанное после `pubkey:` это и есть ваш _адрес кошелька_.

**Примечание:** При работе с бумажными кошельками и файловыми кошельками, термины "pubkey" и "адрес кошелька" являются взаимозаменяемыми.

> Чтобы повысить безопасность мнемо-фразы, можно увеличить количество содержащихся слов. Делается это с помощью аргумента `--word-count`

Для вывода дополнительной информации, запустите:

```bash
solana-keygen new --help
```

### Извлечение публичного ключа

Публичный ключ может быть извлечен из мнемо-фразы и пароля, если последний был использован. This is useful for using an offline-generated seed phrase to derive a valid public key. The `solana-keygen pubkey` command will walk you through how to use your seed phrase (and a passphrase if you chose to use one) as a signer with the solana command-line tools using the `ask` uri scheme.

```bash
solana-keygen pubkey prompt://
```

> Заметьте, что вы можете потенциально использовать различные пароли для одной и той же seed фразы. Каждый уникальный пароль создает разную ключ-пару.

Интсрументы `solana-keygen` используют тот же стандарт BIP39 со стандартным набором английских слов, который используется для генерации мнемонических фраз. Если ваша исходная мнемо-фраза была сгенерирована с помощью других инструментов, которые используют другой стандарт, вы все равно можете использовать `solana-keygen`, но вам нужно будет передать аргумент `--skip-seed-phrase-validation`, чтобы отменить проверку на валидность соответствие стандарту BIP39.

```bash
solana-keygen pubkey prompt:// --skip-seed-phrase-validation
```

After entering your seed phrase with `solana-keygen pubkey prompt://` the console will display a string of base-58 character. This is the base _wallet address_ associated with your seed phrase.

> Скопируйте полученный адрес, например на USB-накопитель, для легкого использования на сетевых компьютерах

> Обычно, следующим шагом является [проверка баланса](#checking-account-balance) аккаунта, связанного с публичным ключом

Для вывода дополнительной информации, запустите:

```bash
solana-keygen pubkey --help
```

### Hierarchical Derivation

The solana-cli supports [BIP32](https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki) and [BIP44](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki) hierarchical derivation of private keys from your seed phrase and passphrase by adding either the `?key=` query string or the `?full-path=` query string.

By default, `prompt:` will derive solana's base derivation path `m/44'/501'`. To derive a child key, supply the `?key=<ACCOUNT>/<CHANGE>` query string.

```bash
solana-keygen pubkey prompt://?key=0/1
```

To use a derivation path other than solana's standard BIP44, you can supply `?full-path=m/<PURPOSE>/<COIN_TYPE>/<ACCOUNT>/<CHANGE>`.

```bash
solana-keygen pubkey prompt://?full-path=m/44/2017/0/1
```

Because Solana uses Ed25519 keypairs, as per [SLIP-0010](https://github.com/satoshilabs/slips/blob/master/slip-0010.md) all derivation-path indexes will be promoted to hardened indexes -- eg. `?key=0'/0'`, `?full-path=m/44'/2017'/0'/1'` -- regardless of whether ticks are included in the query-string input.

## Проверка ключ-пары

Чтобы убдиться, что вы владеете приватным ключом от какого либо адреса, используйте команду `solana-keygen verify`:

```bash
solana-keygen verify <PUBKEY> prompt://
```

where `<PUBKEY>` is replaced with the wallet address and the keyword `prompt://` tells the command to prompt you for the keypair's seed phrase; `key` and `full-path` query-strings accepted. Note that for security reasons, your seed phrase will not be displayed as you type. After entering your seed phrase, the command will output "Success" if the given public key matches the keypair generated from your seed phrase, and "Failed" otherwise.

## Проверка баланса аккаунта

Всё что необходимо для проверки баланса аккаунта - это публичный ключ аккаунта его адрес. Чтобы безопасно получить публичный ключи из бумажного кошелька, следуйте инструкции по [извлечению публичного ключа](#public-key-derivation) на [физически изолированном устройстве](<https://en.wikipedia.org/wiki/Air_gap_(networking)>). Публичные ключи затем можно ввести вручную или передать через USB-накопитель на компьютер подключенный к сети.

Далее, настройте инструменты `Solana CLI` для работы с [определенным кластером](../cli/choose-a-cluster.md):

```bash
solana config set --url <CLUSTER URL> # (i.e. https://api.mainnet-beta.solana.com)
```

Наконец, чтобы проверить баланс, выполните следующую команду:

```bash
solana balance <PUBKEY>
```

## Создание нескольких бумажных кошельков

Вы можете создать столько кошельков, сколько захотите. Просто повторите шаги из раздела [Генерация мнемо-фразы](#seed-phrase-generation) или [Извлечение публичного ключа](#public-key-derivation), чтобы создать новый адрес. Несколько кошельков могут пригодится в случае, если вы хотите хранить ваши токены на кошельках, которые предназначены для разных целей.

## Поддержка

Для получения дополнительной помощи посетите страницу [Поддержка / Устранение проблем](support.md).
