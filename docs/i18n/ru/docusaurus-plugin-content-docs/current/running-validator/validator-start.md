---
title: Запуск валидатора
---

## Настройка Solana CLI

Solana cli включает в себя `get` и `set` команды для автоматического установки `--url` аргумента для команд cli. Например:

```bash
solana config set --url http://devnet.solana.com
```

Хотя в этом разделе показано, как подключаться к кластеру Devnet, шаги также похожи на другие [Кластеры Solana](../clusters.md).

## Проверка доступности кластера

Перед тем как запускать узел валидатора, проверьте, что кластер доступен, запросив количество транзакций:

```bash
solana transaction-count
```

Посмотрите [метрику](https://metrics.solana.com:3000/d/monitor/cluster-telemetry) для более детальной информации по кластеру.

## Начало установки

Попробуйте выполнить следующую команду, чтобы подключиться к сети gossip и просмотреть все остальные узлы кластера:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
# Press ^C to exit
```

## Включение CUDA

Если на вашей машине установлен GPU с CUDA \(только для Linux в данный момент\), добавте аргумент `--cuda` для `solana-validator`.

Когда ваш валидатор будет запущен, в логах будет отображаться следующее сообщение, указывающее что CUDA включен: `"[<timestamp> solana::validator] CUDA is enabled"`

## Настройка системы

### Linux

#### Автоматическая настройка

Репозиторий solana включает демон для настройки системных параметров для оптимизации производительности (то есть увеличения UDP буфера ОС и ограничения сопоставления файлов).

Демон (`solana-sys-tuner`) включен в бинарный релиз solana. Перезапустите его, _перед_ перезапуском валидатора после каждого обновления ПО, чтобы убедиться, что применяются последние рекомендуемые параметры.

Чтобы запустить его:

```bash
sudo solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
```

#### Ручная настройка

Если вы хотите управлять системными настройками самостоятельно, вы можете сделать это следующими командами.

##### **Увеличить UDP-буфер**

```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-udp-buffers.conf <<EOF
# Increase UDP buffer size
net.core.rmem_default = 134217728
net.core.rmem_max = 134217728
net.core.wmem_default = 134217728
net.core.wmem_max = 134217728
EOF"
```

```bash
sudo sysctl -p /etc/sysctl.d/20-solana-udp-buffers.conf
```

##### **Увеличенный лимит сопоставленных файлов памяти**

```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-mmaps.conf <<EOF
# Increase memory mapped files limit
vm.max_map_count = 500000
EOF"
```

```bash
sudo sysctl -p /etc/sysctl.d/20-solana-mmaps.conf
```

Добавьте

```
LimitNOFILE=500000
```

в раздел `[Service]` службы вашего systemd файла, если вы используете его. Или добавьте

```
DefaultLimitNOFILE=500000
```

в раздел `[Manager]` файла `/etc/systemd/system.conf`.

```bash
sudo systemctl daemon-reload
```

```bash
sudo bash -c "cat >/etc/security/limits.d/90-solana-nofiles.conf <<EOF
# Increase process file descriptor count limit
* - nofile 500000
EOF"
```

```bash
### Close all open sessions (log out then, in again) ###
```

## Создание идентификационного ключа

Сгенерируйте идентификационный ключ для вашего валидатора, запустив:

```bash
solana-keygen new -o ~/validator-keypair.json
```

Публичный идентификационный ключ можно просмотреть, запустив:

```bash
solana-keygen pubkey ~/validator-keypair.json
```

> Примечание: Файл "validator-keypair.json" также является приватным ключом \(ed25519\).

### Бумажный кошелек

Вы можете создать бумажный кошелек для файла идентификации вместо того, чтобы записывать файл с ключом на диск с помощью:

```bash
solana-keygen new --no-outfile
```

Публичный идентификационный ключ можно просмотреть, запустив:

```bash
solana-keygen pubkey ASK
```

, а затем введите вашу seed фразу.

Смотрите [Бумажный Кошелек](../wallet-guide/paper-wallet.md) для получения дополнительной информации.

---

### Персонализированный идентификационный ключ

С помощью solana-keygen, вы можете сгенерировать персонализированный идентификационный ключ. Например:

```bash
solana-keygen grind --starts-with e1v1s:1
```

В зависимости от строки, поиск совпадений может занять несколько дней...

---

Ваш идентификационный ключ валидатора уникально идентифицирует ваш валидатор в сети. **Крайне важно сделать резервную копию этой информации.**

Идентификационный ключ валидатора невозможно восстановить, если вы потеряете доступ к нему и не сохраните эту информацию. Если это произойдет, вы ПОТЕРЯЕТЕ ТАКЖЕ ДОСТУП К СВОИМ СРЕДСТВАМ.

Для резервного копирования идентификационного ключа **сделайте резервную копию файла "validator-keypair.json" или seed фразы в безопасное место.**

## Дополнительные настройки Solana CLI

Теперь, когда у вас есть ключ, задайте конфигурацию solana для использования этого ключа следующими командами:

```bash
solana config set --keypair ~/validator-keypair.json
```

Вы должны увидеть следующий вывод:

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://devnet.solana.com
* keypair: /home/solana/validator-keypair.json
```

## Airdrop и проверка баланса

Возьмите для начала немного SOL:

```bash
solana airdrop 10
```

Обратите внимание, что аэродромы доступны только на Devnet и Testnet. Оба ограничены до 10 SOL за запрос.

Для просмотра текущего баланса:

```text
solana balance
```

Или чтобы увидеть более точные сведения:

```text
solana balance --lamports
```

Подробнее о [разнице SOL и lamports здесь](../introduction.md#what-are-sols).

## Создание аккаунта для голосования

Если вы еще не сделали этого, создайте vote аккаунт и зарегистрируйте его в сети. Если вы завершили этот шаг, вы должны увидеть «vote-account-keypair.json» в рабочей папке Solana:

```bash
solana-keygen new -o ~/vote-account-keypair.json
```

Следующая команда может быть использована для создания вашей учетной записи голосования в блокчейне со всеми опциями по умолчанию:

```bash
solana create-vote-account ~/vote-account-keypair.json ~/validator-keypair.json
```

Подробнее о [создании и управлении vote аккаунтом](vote-accounts.md).

## Доверенные валидаторы

Если вы знаете и доверяете другим узлам валидатора, вы можете указать это в командной строке с аргументом `--trusted-validator <PUBKEY>` для `solana-validator`. Вы можете указать несколько из них, повторив аргумент `--trusted-validator <PUBKEY1> --trusted-validator <PUBKEY2>`. Тут присутствуют два эффекта, один из которых состоит в том, что валидатор загружается с `--no-untrusted-rpc`, запрашивает только набор доверенных узлов для загрузки данных генеза и снэпшота. Другой вариант — в сочетании с опцией `--halt-on-trusted-validator-hash-mismatch` будет отслеживаться корневой хэш слияния всех аккаунтов других доверенных узлов на gossip и если хэши выдают какое-либо несоответствие, валидатор остановит узел для предотвращения голосования или обработки потенциально неправильных значений. В данный момент слот, на которую размещается валидатор, привязана к интервалу снимка. Для того чтобы эта функция была эффективной, все валидаторы в доверенном наборе должны быть установлены на один и тот же снэпшот интервала или кратные одному и тому же.

Настоятельно рекомендуется использовать эти параметры для предотвращения вредоносной загрузки состояния моментального снимка или расхождения состояний учетной записи.

## Подключение вашего валидатора

Подключиться к кластеру можно, запустив:

```bash
solana-validator \
  --identity ~/validator-keypair.json \
  --vote-account ~/vote-account-keypair.json \
  --ledger ~/validator-ledger \
  --rpc-port 8899 \
  --entrypoint devnet.solana.com:8001 \
  --limit-ledger-size \
  --log ~/solana-validator.log
```

Чтобы принудительно вести логирование в консоль, добавьте аргумент `--log -`, иначе валидатор будет автоматически записывать логи в файл.

> Примечание: Вы можете использовать seed фразу [бумажного кошелька](../wallet-guide/paper-wallet.md) для вашего `--identity` и/или `--authorized-voter` ключей. Для использования этих аргументов, введите соответствующий аргумент как `solana-validator --identity ASK ... --authorized-voter ASK...` и вам будет предложено ввести seed фразу и, опционально, пароль.

Проверьте подключение вашего валидатора к сети, открыв новый терминал и запустив:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

Если ваш валидатор подключен, в списке появится его публичный ключ и IP-адрес.

### Управление распределением портов локальной сети

По умолчанию валидатор будет динамически выбирать доступные сетевые порты в диапазоне 8000-10000. Это может быть изменено с помощью `--dynamic-port-range`. Например `solana-validator --dynamic-port-range 11000-110...` ограничит использование портов на 11000-11010.

### Ограничение размера ledger для сохранения дискового пространства

Параметр `--limit-ledger-size` позволяет указать, сколько будет занимать [shreds](../terminology.md#shred) на диске. Если вы не включаете этот параметр, то валидатор будет держать весь ledger до тех пор, пока не закончится место на диске.

Значение по умолчанию ограничивает размер ledger до 500ГБ. Большее или меньшее использование диска может быть установлено добавлением аргумента `--limit-ledger-size`. Посмотрите `solana-validator --help` для изменения значения по умолчанию, используемого `--limit-ledger-size`. Дополнительная информация о выборе предельного значения [доступна здесь](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

### Systemd Unit

Запуск валидатора в качестве systemd unit - это один из простых способов управления работой в фоновом режиме.

Если у вас есть пользователь с названием `sol` на вашей машине, создайте файл `/etc/systemd/system/sol.service` с следующими параметрами:

```
[Unit]
Description=Solana Validator
After=network.target
Wants=solana-sys-tuner.service
StartLimitIntervalSec=0

[Service]
Type=simple
Restart=always
RestartSec=1
User=sol
LimitNOFILE=500000
LogRateLimitIntervalSec=0
Environment="PATH=/bin:/usr/bin:/home/sol/.local/share/solana/install/active_release/bin"
ExecStart=/home/sol/bin/validator.sh

[Install]
WantedBy=multi-user.target
```

Теперь создайте `/home/sol/bin/validator.sh` для включения желаемых параметров `solana-validator` командной строки. Убедитесь, что запуск `/home/sol/bin/validator.sh` вручную запускает валидатор. Не забудьте включить права на исполнение `chmod +x /home/sol/bin/validator.sh`

Запуск в качестве службы:

```bash
$ sudo systemctl enable --now sol
```

### Логирование

#### Настройка вывода журнала

Сообщения журнала валидатора могут контролироваться переменной окружения `RUST_LOG`. Детали имеются в [документации](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) для `env_logger`.

Обратите внимание, что если вывод журнала будет уменьшен, это может осложнить отладку с проблемами позже. Если команда обратится за поддержкой, любые изменения должны быть отменены и проблема может повториться до того, как будет оказана помощь.

#### Ротация журнала

Лог работы валидатора, если указан `--log ~/solana-validator.log`, может сильно увеличиться с течением времени, поэтому рекомендуется настраивать ротацию журнала.

Валидатор снова откроет его, когда он получит сигнал `USR1`, он является базовым примитивом, который позволяет ротировать журнал.

#### Использование logrotate

Пример установки `logrotate`, который предполагает, что валидатор работает как системная служба, называемая `sol.service` и записывает лог файл в /home/sol/solana-validator.log:

```bash
# Setup log rotation

cat > logrotate.sol <<EOF
/home/sol/solana-validator.log {
  rotate 7
  daily
  missingok
  postrotate
    systemctl kill -s USR1 sol.service
  endscript
}
EOF
sudo cp logrotate.sol /etc/logrotate.d/sol
systemctl restart logrotate.service
```

### Отключение проверки портов для ускорения перезапуска

Как только ваш валидатор заработает нормально, вы можете сократить время, необходимое для перезапуска валидатора, добавив флаг `--no-port-check` в командную строку `solana-validator`.

### Отключение сжатия снэпшота для уменьшения использования ЦП

Если вы не предоставляете снэпшоты другим валидаторам, сжатие снимков можно отключить, чтобы уменьшить нагрузку ЦП за счет чуть большего использования диска для локального снэпшота.

Добавьте параметр `--snapshot-compression none` к `solana-validator` и перезапустите валидатора.

### Использование ramdisk с расширением в файл подкачки касательно папки accounts для уменьшения износа SSD

Если на вашей машине много оперативной памяти, ramdisk tmpfs ([tmpfs](https://man7.org/linux/man-pages/man5/tmpfs.5.html)) может использоваться для хранения папки accounts

При использовании tmpfs важно также настроить файл подкачки, во избежание возможного переполнения tmpfs.

Рекомендуется использовать раздел 300GB tmpfs, вместе с файлом подкачки в 250GB.

Пример конфигурации:

1. `sudo mkdir /mnt/solana-accounts`
2. Добавьте 300ГБ tmpfs путем добавления новой строки, содержащей `tmpfs /mnt/solana-accounts tmpfs rw,size=300G, ser=sol 0 0` в `/etc/fstab` (при условии, что ваш валидатор запущен под пользователем "sol"). **ВНИМАНИЕ: Если вы неправильно отредактируете /etc/fstab ваша машина может перестать загружаться**
3. Создайте по крайней мере 250ГБ файл подкачки

- Выберите устройство, используемое вместо `SWAPDEV` для остальной части этих инструкций. В идеале выберите свободный раздел 250GB или больше на быстром диске. Если такой файл подкачки отсутствует, создайте его `sudo dd if=/dev/zero of=/swapfile bs=1MiB count=250KiB`, установите права доступа `sudo chmod 0600 /swapfile` и используйте `/swapfile` как `SWAPDEV` для остальных этих инструкций
- Пометьте этот файл для использования в качестве файла подкачки с помощью SWAPDEV `sudo mkswap`

4. Добавить файл подкачки в `/etc/fstab` новой строкой, содержащей `SWAPDEV swap swap defaults 0 0`
5. Включите файл подкачки с помощью `sudo swapon -a` и смонтируйте tmpfs `sudo mount /mnt/solana-accounts/`
6. Убедитесь, что он активен с помощью `free -g`, а с помощью `mount`, что tmpfs смонтирован

Добавьте параметр `--snapshot-compression none` к `solana-validator` и перезапустите валидатора.

### Индексирование аккаунтов

По мере увеличения количества аккаунтов в кластере, RPC запросы, сканирующие весь набор аккаунтов, такие как [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) и [PL-token-specific requests](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate) -- могут работать не корректно. Чтобы ваш валидатор смог поддержать любой из этих запросов, вы можете использовать параметр `--account-index` для активации одного или нескольких индексов в памяти, что значительно улучшает производительность RPC путем индексирования аккаунтов по ключевому полю. В настоящее время поддерживает следующие значения параметров:

- `program-id`: каждый аккаунт индексируется своей собственной программой; используется как [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts)
- `spl-token-mint`: каждый аккаунт токена SPL индексируется своим токеном Mint; используется как [getTokenAccountsByDelegate](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate)и [getTokenLargestAccounts](developing/clients/jsonrpc-api.md#gettokenlargestaccounts)
- `spl-token-owner`: каждый аккаунт токена SPL индексируется адресом владельца токена; используется как [getTokenAccountsByOwner](developing/clients/jsonrpc-api.md#gettokenaccountsbyowner) и [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) запросы, которые включают фильтр spl-token-owner.
