---
title: Мониторинг валидатора
---

## Проверка Gossip

Убедитесь, что IP-адрес и **идентификационный ключ** вашего валидатора видны в gossip, выполнив:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

## Проверка баланса

Ваш баланс должен быть уменьшен на сумму комиссии, которую ваш валидатор тратит на голоса и увеличивается после того, как он будет выступать в качестве лидера. Выполните с `--lamports` для просмотра в более мелких деталях:

```bash
solana balance --lamports
```

## Проверка Vote Activity

Команда `solana vote-account` отображает недавнюю активность голосования вашего валидатора:

```bash
solana vote-account ~/vote-account-keypair.json
```

## Получение информации о кластере

Есть несколько полезных JSON-RPC endpoints для мониторинга вашего валидатора в кластере, а также для просмотра состояния кластера:

```bash
# Похоже на solana-gossip, вы должны увидеть валидатор в списке кластерных узлов
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2. ","id":1, "метод":"getClusterNodes"}" http://devnet.solana.com
# Если ваш валидатор правильно голосовал, он должен появиться в списке `current`. Если имеется `stake`, должно быть > 0
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getVoteAccounts"}" http://devnet.solana. om
# Возвращает текущий график лидеров
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2. ","id":1, "method":"getLeaderSchedule"}' http://devnet.solana.com
# Возвращает информацию о текущем эпохе. slotIndex должен увеличиваться.
curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1, "method":"getEpochInfo"}' http://devnet.solana.com
```
