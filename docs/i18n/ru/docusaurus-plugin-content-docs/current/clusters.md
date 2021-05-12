---
title: Кластер Solana
---

Solana одновременно поддерживает несколько кластеров с различными назначениями.

Перед началом убедитесь, что у вас установленны [инструменты командной строки Solana](cli/install-solana-cli-tools.md)

Обозреватели:

- [http://explorer.solana.com/](https://explorer.solana.com/).
- [http://solanabeach.io/](http://solanabeach.io/).

## Devnet

- Devnet служит песочницей для каждого, кто хочет опробовать блокчейн Solana в роли пользователя, холдера монет, разработчика приложений или валидатора.
- Разработчики приложений в первую очередь должны использовать Devnet.
- Потенциальные валидаторы также должны сначала использовать кластер Devnet.
- Ключевые различия между Devnet и Mainnet Beta:
  - Токены в кластере Devnet **не настоящие** (это тестовые монеты, предназначенные исключительно для тестовых целей)
  - Devnet предоставляет кран (spl-faucet), где можно получить spl-токены для тестирования приложений
  - Блокчейн в кластере Devnet может быть сброшен до генезис-блока
  - Как правило, в Devnet используется более новая версия программного обеспечения, чем в Mainnet Beta
- Точка входа для Gossip сервиса в кластере Devnet: `entrypoint.devnet.solana.com:8001`
- Переменная метрик для Devnet:
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=devnet,u=scratch_writer,p=topsecret"
```
- RPC URL для Devnet: `https://devnet.solana.com`

##### Пример конфигурации командной строки `solana` для Devnet

```bash
solana config set --url https://devnet.solana.com
```

##### Пример подключения валидатора к кластеру Devnet, используя `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator dv1LfzJvDF7S1fBKpFgKoKXK5yoSosmkAdfbxBo1GqJ \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.devnet.solana.com:8001 \
    --expected-genesis-hash EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

`--trusted-validator`s находятся под управлением Solana

## Testnet

- Testnet — это место, где мы проводим стресс-тесты новых возможностей Solana на кластере в реальном времени; основной упор делается на производительность сети, устойчивость и поведение валидирующих нод.
- [Tour de SOL](tour-de-sol.md) — это программа поощрений в кластере Testnet, где приветствуется злонамеренное поведение и атаки на сеть, чтобы помочь нам найти и устранить ошибки и уязвимости.
- Токены в кластере Testnet **не настоящие** (это тестовые монеты, предназначенные исключительно для тестовых целей)
- Блокчейн в кластере Testnet может быть сброшен до генезис-блока.
- Testnet предоставляет кран (spl-faucet), где можно получить spl-токены для тестирования приложений
- Как правило, в Testnet используется самая свежая версия программного обеспечения, новее чем в кластерах Devnet и Mainnet Beta
- Точка входа для Gossip сервиса в кластере Testnet: `entrypoint.testnet.solana.com:8001`
- Переменная метрик для Testnet:
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=tds,u=testnet_write,p=c4fa841aa918bf8274e3e2a44d77568d9861b3ea"
```
- RPC URL для Testnet: `https://testnet.solana.com`

##### Пример конфигурации командной строки `solana` для Testnet

```bash
solana config set --url https://testnet.solana.com
```

##### Пример подключения валидатора к кластеру Testnet, используя `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --trusted-validator ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T \
    --trusted-validator Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN \
    --trusted-validator 9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.testnet.solana.com:8001 \
    --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

Идентификаторы `--trusted-validator`s для Testnet:

- `5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on` - testnet.solana.com (Solana)
- `ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T` - Break RPC node (Solana)
- `Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN` - Certus One
- `9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv` - Algo|Stake

## Mainnet Beta

Публичный кластер с неотменяемой историей блокчейна для дрежателей монет и партнёров. В настоящий момент, награды и инфляция отключены.

- Токены, выпущенные в Mainnet Beta, **являются реальными** SOL
- Если вы оплатили покупку токенов, например, через наш аукцион на CoinList, эти токены будут переведены в Mainnet Beta.
  - Примечание: Если вы используете кошелек без поддержки командной строки, например такой как [Solflare](wallet-guide/solflare.md), то кошелек всегда будет подключаться к Mainnet Beta.
- Точка входа для Gossip сервиса в кластере Mainnet Beta: `entrypoint.mainnet-beta.solana.com:8001`
- Переменная метрик для Mainnet Beta:
```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=mainnet-beta,u=mainnet-beta_write,p=password"
```
- URL RPC для Mainnet Beta: `https://api.mainnet-beta.solana.com`

##### Пример конфигурации командной строки `solana` для Mainnet Beta

```bash
solana config set --url https://api.mainnet-beta.solana.com
```

##### Пример подключения валидатора к кластеру Mainnet Beta, используя `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 \
    --trusted-validator GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ \
    --trusted-validator DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ \
    --trusted-validator CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --private-rpc \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint2.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint3.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint4.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint5.mainnet-beta.solana.com:8001 \
    --expected-genesis-hash 5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

Все четыре `--trusted-validator`s находятся под управлением Solana
