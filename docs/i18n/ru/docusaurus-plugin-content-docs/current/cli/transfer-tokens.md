---
title: Отправка и получение токенов
---

Эта страница расширяет способ получения и отправки SOL токенов с помощью инструментов командной строки с кошельком командной строки, таких как [бумажный кошелёк](../wallet-guide/paper-wallet.md), файловый кошелёк [](../wallet-guide/file-system-wallet.md), или [аппаратный кошелёк](../wallet-guide/hardware-wallets.md). Прежде чем начать, убедитесь что Вы создали кошелёк и имеете доступ к его адресу (pubkey) и ключу подписи. Ознакомьтесь с нашими [соглашениями о вводе ключей для различных типов кошельков](../cli/conventions.md#keypair-conventions).

## Проверка Вашего кошелька

Перед тем, как поделиться Вашим открытым ключом с другими, Вы можете сначала убедиться, что ключ действителен и у Вас действительно есть соответствующий приватный ключ.

В этом примере мы создадим второй кошелёк в дополнение к первому кошельку, и затем передадим несколько токенов на него. Это подтвердит, что вы можете отправлять и получать токены на Ваш выбранный типа кошелька.

В этом тестовом примере используется наш тестовый Testnet, называемый devnet. Выпущенные токенов в devnet не имеют значения ** no **, поэтому не беспокойтесь, если Вы их потеряете.

#### Возьмите некоторые токены для начала

Во-первых, _airdrop_ вы немного играете на devnet.

```bash
solana airdrop 1 <RECIPIENT_ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

, где Вы замените текст `<RECIPIENT_ACCOUNT_ADDRESS>` на Ваш base58-закодированный публичный ключ/адрес кошелька.

#### Проверьте свой баланс

Подтвердите, что airdrop был успешно зачислен на баланс аккаунта. Он должен вывести `1 SOL`:

```bash
solana airdrop <ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

#### Создать второй адрес кошелька

Нам понадобится новый адрес для получения токенов. Создайте вторую клавишу и запишите свой pubkey:

```bash
solana-keygen new --no-passphrase --no-outfile
```

Вывод будет содержать адрес после текста `pubkey:`. Скопируйте адрес. Мы будем использовать его на следующем этапе.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

Вы также можете создать второй (или более) кошелек любого типа: [бумага](../wallet-guide/paper-wallet#creating-multiple-paper-wallet-addresses), [файловая система](../wallet-guide/file-system-wallet.md#creating-multiple-file-system-wallet-addresses), или [оборудование](../wallet-guide/hardware-wallets.md#multiple-addresses-on-a-single-hardware-wallet).

#### Передача токенов из Вашего первого кошелька на второй

Далее докажите, что Вы владеете токенами, передавая их. Кластер Solana принимает перевод только в том случае, если Вы подпишете транзакцию с приватным ключом, соответствующим публичному ключу отправителя в транзакции.

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> 0.5 --allow-unfunded-recipient --url https://devnet.solana.com --fee-payer <KEYPAIR>
```

где вы заменили `<KEYPAIR>` на путь к клавиатуре в Вашем первом кошельке, и замените `<RECIPIENT_ACCOUNT_ADDRESS>` адресом Вашего второго кошелька.

Подтвердите обновленные балансы на `solana балансе`:

```bash
solana airdrop <ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

где `<ACCOUNT_ADDRESS>` является либо публичным ключом, либо публичным ключом получателя.

#### Полный пример тестовой передачи

```bash
$ solana-keygen new --outfile my_solana_wallet.json # Создание моего первого кошелька, кошелька с файловой системой
Создание новой пары ключей
Для дополнительной безопасности введите парольную фразу (пусто, если парольная фраза отсутствует):
Записал новую пару ключей в my_solana_wallet.json
================================================== ========================
pubkey: DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK # Вот адрес первого кошелька
================================================== ========================
Сохраните эту исходную фразу, чтобы восстановить новую пару ключей:
width enhance concert vacant ketchup eternal spy craft spy guard tag punch # Если бы это был настоящий кошелек, никогда не делитесь этими словами в Интернете вот так!
==========================================================================

$ solana airdrop 1 DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com  # Airdropping 1 SOL to my wallet's address/pubkey
Requesting airdrop of 1 SOL from 35.233.193.70:9900
1 SOL

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com # Check the address's balance
1 SOL

$ solana-keygen new --no-outfile  # Creating a second wallet, a paper wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
====================================================================
pubkey: 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv                   # Here is the address of the second, paper, wallet.
============================================================================
Сохрани эту исходную фразу, чтобы восстановить Ваш новый тип:
clump panic cousin hurt coast charge engage fall eager urge win love   # # Если бы это был настоящий кошелёк, никогда не делитесь этими словами в Интернете так!
====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 0.5 --allow-unfunded-recipient --url https://devnet.solana.com --fee-payer my_solana_wallet.json  # Transferring tokens to the public address of the paper wallet
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # This is the transaction signature

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com
0.499995 SOL  # The sending account has slightly less than 0.5 SOL remaining due to the 0.000005 SOL transaction fee payment

$ solana balance 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv --url https://devnet.solana.com
0.5 SOL  # The second wallet has now received the 0.5 SOL transfer from the first wallet

```

## Получить токены

Чтобы получать токены, вам понадобится адрес для отправки токенов другим. В Solana, адрес кошелька является публичным ключом клавиатуры. Существует множество методов генерации клавиш. Выбранный вами метод будет зависеть от того, как вы выбираете хранить пары ключей. Пары ключей хранятся в кошельках. Перед получением токенов вам нужно [создать кошелёк](../wallet-guide/cli.md). После завершения у вас должен быть открытый ключ для каждого генерируемой вами пары. Публичный ключ является длинной строкой из символов base58. Его длина составляет от 32 до 44 символов.

## Отправить токены

Если вы уже удерживаете SOL и хотите отправлять токены кому-то, вам понадобится путь к вашему ключевому слову, свой открытый ключ base58-кодированный и ряд токенов для передачи. После того, как вы собрали, вы можете передать токены с помощью команды `Solana transfer`:

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> <AMOUNT> --fee-payer <KEYPAIR>
```

Проверить обновленное состояние баланса можно с помощью команды `solana balance`:

```bash
solana balance <ACCOUNT_ADDRESS>
```
