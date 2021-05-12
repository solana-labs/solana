---
title: Управление stake-аккаунтами
---

Если вы хотите делегировать стейк нескольким различным валидаторам, то для каждого из них необходимо создать отдельный stake-аккаунт. Если создать stake-аккаунты так, чтобы первый из них начинался с seed-числа "0", второй - с "1", третий - с "2" и так далее, то утилита `solana-stake-accounts` сможет работать со всеми аккаунтами сразу. Это может пригодиться для подсчета балансов всех аккаунтов, перемещения аккаунтов в новый кошелек или для привязки новых ключей авторизации.

## Использование

### Создание stake-аккаунта

Для создания и пополнения нового stake-аккаунта выполните следующую команду, используя публичный ключ владельца стейка:

```bash
solana-stake-accounts new <FUNDING_KEYPAIR> <BASE_KEYPAIR> <AMOUNT> \
    --stake-authority <PUBKEY> --withdraw-authority <PUBKEY> \
    --fee-payer <KEYPAIR>
```

### Подсчет количества аккаунтов

Для подсчета количества созданных аккаунтов выполните следующую команду:

```bash
solana-stake-accounts count <BASE_PUBKEY>
```

### Получение балансов stake-аккаунтов

Для подсчета баланса всех созданных stake-аккаунтов выполните следующую команду:

```bash
solana-stake-accounts balance <BASE_PUBKEY> --num-accounts <NUMBER>
```

### Получение адресов stake-аккаунтов

Для получения списка адресов всех stake-аккаунтов, созданных для указанного публичного ключа выполните следующую команду:

```bash
solana-stake-accounts addresses <BASE_PUBKEY> --num-accounts <NUMBER>
```

### Привязка новых ключей

Для привязки новых ключей ко всем созданным stake-аккаунтам выполните следующую команду:

```bash
solana-stake-accounts authorize <BASE_PUBKEY> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```

### Перемещение stake-аккаунтов

Для перемещения stake-аккаунтов выполните следующую команду:

```bash
solana-stake-accounts rebase <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --num-accounts <NUMBER> \
    --fee-payer <KEYPAIR>
```

Для перемещения и авторизации каждого stake-аккаунта по отдельности используйте команду 'move':

```bash
solana-stake-accounts move <BASE_PUBKEY> <NEW_BASE_KEYPAIR> \
    --stake-authority <KEYPAIR> --withdraw-authority <KEYPAIR> \
    --new-stake-authority <PUBKEY> --new-withdraw-authority <PUBKEY> \
    --num-accounts <NUMBER> --fee-payer <KEYPAIR>
```
