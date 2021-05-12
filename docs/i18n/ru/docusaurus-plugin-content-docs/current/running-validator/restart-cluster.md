## Перезапуск кластера

### Шаг 1. Определить слот, в котором кластер будет перезапущен

Самый высокий оптимально подтвержденный слот - это лучший слот для начала, который можно найти, найдя его в [этой](https://github.com/solana-labs/solana/blob/0264147d42d506fb888f5c4c021a998e231a3e74/core/src/optimistic_confirmation_verifier.rs#L71) метрике datapoint. В противном случае используйте последний root.

Запустите slot `SLOT_X`

### Шаг 2. Остановить валидатор(ы)

### Шаг 3. При необходимости установить новую версию solana

### Шаг 4. Создайте новый снимок для slot `SLOT_X` с хардфорком в slot `SLOT_X`

```bash
$ solana-ledger-tool -l ledger create-snapshot SLOT_X ledger --hard-fork SLOT_X
```

Каталог ledger теперь должен содержать новый снимок. `solana-ledger-tool create-snapshot` will also output the new shred version, and bank hash value, call this NEW_SHRED_VERSION and NEW_BANK_HASH respectively.

Исправьте аргументы вашего валидатора:

```bash
 --wait-for-supermajority SLOT_X
 --expected-bank-hash NEW_BANK_HASH
```

Затем перезапустите валидатор.

Убедитесь в логах, что валидатор загрузился и теперь находится в `SLOT_X`, ожидая супер большинства.

### Шаг 5. Объявить о перезапуске Discord:

Опубликовать что-то вроде в #announcements (изменить текст при необходимости):

> Hi @Validators,
>
> We've released v1.1.12 and are ready to get testnet back up again.
>
> Steps:
>
> 1. Install the v1.1.12 release: https://github.com/solana-labs/solana/releases/tag/v1.1.12
> 2. a. Preferred method, start from your local ledger with:
>
> ```bash
> solana-validator
>   --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --hard-fork SLOT_X                  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --no-snapshot-fetch                 # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
>   --entrypoint entrypoint.testnet.solana.com:8001
>   --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
>   --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
>   --no-untrusted-rpc
>   --limit-ledger-size
>   ...                                # <-- your other --identity/--vote-account/etc arguments
> ```

````

b. If your validator doesn't have ledger up to slot SLOT_X or if you have deleted your ledger, have it instead download a snapshot with:

```bash
solana-validator
  --wait-for-supermajority SLOT_X     # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --expected-bank-hash NEW_BANK_HASH  # <-- NEW! IMPORTANT! REMOVE AFTER THIS RESTART
  --entrypoint entrypoint.testnet.solana.com:8001
  --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on
  --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY
  --no-untrusted-rpc
  --limit-ledger-size
  ...                                # <-- your other --identity/--vote-account/etc arguments
````

     You can check for which slots your ledger has with: `solana-ledger-tool -l path/to/ledger bounds`

3. Wait until 80% of the stake comes online

To confirm your restarted validator is correctly waiting for the 80%: a. Look for `N% of active stake visible in gossip` log messages b. Ask it over RPC what slot it's on: `solana --url http://127.0.0.1:8899 slot`. It should return `SLOT_X` until we get to 80% stake

Thanks!

### Шаг 7. Подождать и слушать

Отслеживать валидаторы при их перезапуске. Отвечать на вопросы и помогать с перезапуском
