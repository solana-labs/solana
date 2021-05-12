---
title: Публикация информации о валидаторе
---

Вы можете опубликовать информацию валидаторе в блокчейне, которая будет доступна всем другим пользователям.

## Solana validator-info

Запустите solana CLI для заполнения информации о валидаторе:

```bash
solana validator-info publish --keypair ~/validator-keypair.json <VALIDATOR_INFO_ARGS> <VALIDATOR_NAME>
```

Для получения подробной информации о дополнительных полях для VALIDATOR_INFO_ARGS:

```bash
solana validator-info publish --help
```

## Примеры команд

Пример команды публикации:

```bash
solana validator-info publish "Elvis Validator" -n elvis -w "https://elvis-validates.com"
```

Пример команды запроса:

```bash
solana validator-info get
```

ожидаемый вывод

```text
Validator info from 8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFizayqziDc9ah
  Validator pubkey: 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTTyj4xmyDMWo
  Info: {"keybaseUsername":"elvis","name":"Elvis Validator","website":"https://elvis-validates.com"}
```

## Keybase

Имя пользователя Keybase позволяет клиентским приложениям (например Solana Network Explorer) автоматически получить доступ в ваш публичный профиль, включая криптографические доказательства, идентификацию бренда и т. д. Чтобы подключить ваш валидатор с помощью Keybase:

1. Зарегистрируйтесь на [https://keybase.io/](https://keybase.io/) и заполните профиль
2. Добавьте ваш валидатор validator **identity pubkey** в Keybase:

   - Создайте пустой файл на локальном компьютере с названием `validator-<PUBKEY>`
   - В Keybase перейдите в раздел Files и загрузите ваш pubkey в

     подкаталог `solana` в вашей публичной папке: `/keybase/public/<KEYBASE_USERNAME>/solana`

   - Чтобы проверить ваш pubkey, убедитесь, что вы можете успешно перейти на

     `https://keybase.pub/<KEYBASE_USERNAME>/solana/validator-<PUBKEY>`

3. Добавьте или обновите ваш `solana validator-info` с вашим логином Keybase. The

   CLI will verify the `validator-<PUBKEY>` file
