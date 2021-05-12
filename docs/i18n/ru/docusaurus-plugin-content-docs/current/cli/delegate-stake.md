---
title: Делегирование ставки
---

После того, как вы [ получили SOL ](transfer-tokens.md), вы можете подумать о том, чтобы их использовать, делегировав _ ставку _ валидатору. Ставка — это то, что мы называем токены в _аккаунте_. Solana взвешивает голоса валидаторов по размеру ставки делегированы им, что дает этим валидаторам больше влияния при определении следующего действительного блока транзакций в цепочке блоков. Затем Solana периодически генерирует новые SOL, чтобы вознаграждать делегировавших ставки и валидаторов. Вы получаете больше наград чем больше делегируете.

## Создание аккаунта для ставки

Чтобы делегировать ставку, вам нужно будет перенести некоторое количество токенов на аккаунт ставки. Для созданияаккаунта вам понадобится ключевые пары. Публичный ключ будет использован как [адрес аккаунта ставки ](../staking/stake-accounts.md#account-address). Здесь нет необходимости в пароле или шифровании; эта пара ключей будет удалена сразу после создания аккаунта ставки.

```bash
solana-keygen new --no-passphrase -o stake-account.json
```

Вывод будет содержать адрес после текста `pubkey:`.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV
```

Скопируйте публичный ключ и храните его для в надежном месте. Вам это понадобится в любое время, когда вы хотите выполнить действие над аккаунтом ставки, которую вы создадите следующей.

Создание аккаунта для ставки:

```bash
solana create-stake-account --from <KEYPAIR> stake-account.json <AMOUNT> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --fee-payer <KEYPAIR>
```

`<AMOUNT>` токены передаются с аккаунта "from" `<KEYPAIR>` на новый аккаунт на публичном ключе stake-account.json.

Теперь файл stack-account.json можно удалить. Чтобы авторизовать дополнительные действия, вы будете использовать пару ключей `--stake-authority` или `--withdraw-authority`, не stake-account.json.

Просмотреть ставку на новом аккаунте можно с помощью `solana stake-account` команда:

```bash
solana stake-account <STAKE_ACCOUNT_ADDRESS>
```

Результат будет выглядеть примерно так:

```text
Total Stake: 5000 SOL
Stake account is undelegated
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

### Установка Ставки и Вывода с помощью авторизации

[ Ставка и вывод с помощью авторизации](../staking/stake-accounts.md#understanding-account-authorities) можно установить при создании аккаунта через Параметры `--stake-authority ` и `--withdraw-author` или после этого с помощью параметра `solana stake-authorize` команда. Например, чтобы установить авторизацию доступа, запустите:

```bash
solana stake-authorize <STAKE_ACCOUNT_ADDRESS> \
    --stake-authority <KEYPAIR> --new-stake-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

Это будет использовать существующие полномочия на ставку `<KEYPAIR>` для авторизации новой ставки `<PUBKEY>` на аккаунте для ставки `<STAKE_ACCOUNT_ADDRESS>`.

### Дополнительно: Адреса аккаунта для получения ставки

Когда вы делегируете ставку, вы делегируете все токены в аккаунте на счет одному валидатору. Чтобы делегировать нескольким валидаторам, вам потребуется несколько аккаунтов. Создание новой пары ключей для каждого аккаунта и управление этими адресами могут быть неудобными. К счастью, вы можете получить адреса ставок, используя параметр `--seed`:

```bash
solana create-stake-account --from <KEYPAIR> <STAKE_ACCOUNT_KEYPAIR> --seed <STRING> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> --fee-payer <KEYPAIR>
```

`<STRING>` - произвольная строка длиной до 32 байтов, но обычно это номер, соответствующий производной аккаунта. Первый аккаунт мог бы быть "0", затем "1" и так далее. Публичный ключ `<STAKE_ACCOUNT_KEYPAIR>` выполняет функцию в качестве базового адреса. Команда получает новый адрес с базового адреса и инициализации строки. Чтобы увидеть, какую команду будет выдать команда, используйте `solana create-address-with-seed`:

```bash
solana create-address-s-s-seed --from <PUBKEY> <SEED_STRING> STAKE
```

`<PUBKEY>` является публичным ключом `<STAKE_ACCOUNT_KEYPAIR>` передан `solana create-stake-account`.

Команда выведет полученный адрес, который может быть использован для аргумента `<STAKE_ACCOUNT_ADDRESS>` при операций со ставкой.

## Делегирование

Чтобы делегировать свою ставку валидатору, вам понадобится его адрес для голосования. Найдите его, запросив кластер по списку всех валидаторов и их голоса аккаунтов с помощью команды `solana validators`:

```bash
solana validators
```

Первый столбец каждой строки содержит идентификатор валидатора, а второй - адрес аккаунта для голосования. Выберите валидатор и используйте свой аккаунт для голосования в `solana delegate-stake`:

```bash
solana delegate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <VOTE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

Права на управления ставкой `<KEYPAIR>` разрешает операцию над аккаунтом с адресом `<STAKE_ACCOUNT_ADDRESS>`. Ставка делегируется на счет для голосования с адресом `<VOTE_ACCOUNT_ADDRESS>`.

После делегирования ставки, используйте `Solana учетную запись`, чтобы наблюдать за изменениями в аккаунте на долях:

```bash
solana stake-account <STAKE_ACCOUNT_ADDRESS>
```

Вы увидите новые поля "Делегированная ставка" и "Делегированный адрес аккаунта голосования" в выводе. Результат будет выглядеть примерно так:

```text
Total Stake: 5000 SOL
Credits Observed: 147462
Delegated Stake: 4999.99771712 SOL
Delegated Vote Account Address: CcaHc2L43ZWjwCHART3oZoJvHLAe9hzT2DJNUpBzoTN1
Stake activates starting from epoch: 42
Stake Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
Withdraw Authority: EXU95vqs93yPeCeAU7mPPu6HbRUmTFPEiGug9oCdvQ5F
```

## Деактивация ставки

После делегирования вы можете отменить участие в акции `deactivate-coke` команда:

```bash
solana deactivate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> \
    --fee-payer <KEYPAIR>
```

Права на управления ставкой `<KEYPAIR>` разрешает операцию над аккаунтом с адресом `<STAKE_ACCOUNT_ADDRESS>`.

Обратите внимание, что ставка занимает несколько эпох, чтобы "охладиться". Попытки делегировать долю в период охлаждения не удаются.

## Вывод

Перевод токенов вне аккаунта с помощью команды вывода `solana`:

```bash
solana delegate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <RECIPIENT_ADDRESS> \
    --fee-payer <AMOUNT>
```

`<STAKE_ACCOUNT_ADDRESS>` это существующий аккаунт управляет ставкой `<KEYPAIR>` имеет право на вывод средств, и `<AMOUNT>` это количество токенов для передачи `<RECIPIENT_ADDRESS>`.

## Разбить ставку

Вы можете делегировать ставку дополнительным валидаторам, в то время как ваша существующая ставка не подлежит снятию. Это может быть неприемлемо, потому что это в настоящее время ставка "остывает" или заблокирована. Чтобы перенести токены из существующего аккаунта ставки в новый, используйте команду `solana split-stack`:

```bash
solana delegate-stake --stake-authority <KEYPAIR> <STAKE_ACCOUNT_ADDRESS> <NEW_STAKE_ACCOUNT_KEYPAIR> \
    --fee-payer <AMOUNT>
```

`<STAKE_ACCOUNT_ADDRESS>` - существующий аккаунт для ставки,с правами управления ставкой `<KEYPAIR>` - это пара ключей с правами на ставку, `<NEW_STAKE_ACCOUNT_KEYPAIR>` - это пара ключей для нового аккаунта, а `<AMOUNT>` - количество токенов для передачи для нового аккаунта.

Чтобы разделить аккаунт на полученный адрес аккаунта, используйте параметр `--seed`. Смотрите [Получить адрес аккаунта для ставки](#advanced-derive-stake-account-addresses) для информации.
