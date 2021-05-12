---
title: Установка и обновления программного обеспечения кластера
---

В настоящее время пользователи должны сами собрать программное обеспечение кластера Solana из git репозитория и вручную обновить его, что чревато ошибками и неудобно.

В этом документе предлагается простое в использовании средство установки и обновления программного обеспечения, которое можно использовать для развертывания предварительно созданных бинарных файлов для поддерживаемых платформ. Пользователи могут использовать исполняемые файлы, предоставленные Solana или любой другой стороной, которой они доверяют. Управление установкой обновлений осуществляется с помощью on-chain программы манифеста обновления.

## Мотивирующие примеры

### Загрузите и запустите заранее созданную программу установки, используя curl/shell скрипт начальной загрузки

Самый простой метод установки для поддерживаемых платформ:

```bash
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh
```

Этот скрипт проверит github на наличие последней отмеченной версии, а также загрузит из него и запустит исполняемый файл `solana-install-init`.

Если необходимо указать дополнительные аргументы во время установки, то используется следующий синтаксис:

```bash
$ init_args=.... # аргументы для `solana-install-init ...`
$ curl -sSf https://raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh -s - ${init_args}
```

### Получите и запустите заранее созданный инсталлятор из релиза на Github

С хорошо известным URL-адресом релизов можно получить готовый бинарный файл для поддерживаемых платформ:

```bash
$ curl -o solana-install-init https://github.com/solana-labs/solana/releases/download/v1.0.0/solana-install-init-x86_64-apple-darwin
$ chmod +x ./solana-install-init
$ ./solana-install-init --help
```

### Создайте и запустите программу установки из исходного кода

Если предварительно созданный бинарный файл не доступен для данной платформы, всегда можно создать установщик из исходного кода:

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana/install
$ cargo run -- --help
```

### Развернуть новое обновление для кластера

Для установки обновления, используя тарбол релиза Solana \(созданный командой `ci/publish-tarball.sh`\), который уже был загружен на общедоступный URL-адрес, используйте следующие команды:

```bash
$ solana-keygen new -o update-manifest.json  # <-- генерируется только один раз, открытый ключ передается пользователям
$ solana-install deploy http://example.com/path/to/solana-release.tar.bz2 update-manifest.json
```

### Запуск узла валидатора, который сам автоматически обновляется

```bash
$ solana-install init --pubkey 92DMonmBYXwEMHJ99c9ceRSpAmk9v6i3RdvDdXaVcrfj  # <-- pubkey получен от того, кто развертывает обновления
$ export PATH=~/.local/share/solana-install/bin:$PATH
$ solana-keygen ...  # <-- запускает последнюю версию solana-keygen
$ solana-install run solana-validator ...  # <-- запускает валидатор, перезагружая его по мере необходимости, когда применяется обновление
```

## On-chain манифест обновления

Манифест обновления используется для объявления о развертывании тарболов с новыми релизами в кластере Solana. Манифест обновления хранится, используя программу `config`, и каждый аккаунт манифеста обновления описывает логический канал обновления для заданной целевой тройки \(например: `x86_64-apple-darwin`\). Публичный ключ аккаунта хорошо известен между объектом, развертывающим новые обновления, и пользователями, использующими эти обновления.

Сам архив обновления размещен в другом месте вне сети и может быть загружен из указанного `download_url`.

```text
use solana_sdk::signature::Signature;

/// Информация, необходимая для загрузки и применения данного обновления
pub struct UpdateManifest {
    pub timestamp_secs: u64, // Когда релиз был развернут в секундах после UNIX EPOCH
    pub download_url: String, // URL для скачивания релиза tar.bz2
    pub download_sha256: String, // SHA256 дайджест файла релиза tar.bz2
}

/// Данные аккаунта программы манифеста обновлений.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct SignedUpdateManifest {
    pub manifest: UpdateManifest,
    pub manifest_signature: Signature,
}
```

Обратите внимание, что само поле `manifest` содержит соответствующую подпись \(`manifest_signature`\) для защиты от атаки man-in-middle между инструментом `solana-install` и RPC API кластера Solana.

Чтобы защититься от атак отката, `solana-install` откажется устанавливать обновление с более старым значением `timestamp_secs`, чем установлено в настоящее время.

## Содержание архива релиза

Архив релиза должен быть tar-файлом, сжатым с помощью bzip2 со следующей внутренней структурой:

- `/version.yml` - простой YAML файл, содержащий поле `"target"` -

  целевой кортеж. Любые дополнительные поля игнорируются.

- `/bin/` -- каталог, содержащий доступные программы в релизе.

  `solana-install` создаст символическую ссылку этого каталога на

  `~/.local/share/solana-install/bin` для использования `PATH` переменной

  окружения.

- `...` -- любые дополнительные файлы и каталоги разрешены

## Инструмент solana-install

Инструмент `solana-install` используется пользователем для установки и обновления программного обеспечения кластера.

Он управляет следующими файлами и каталогами в домашнем каталоге пользователя:

- `~/.config/solana/install/config.yml` - конфигурация пользователя и информация о текущей установленной версии программного обеспечения
- `~/.local/share/solana/install/bin` - символическая ссылка на текущий релиз. Например, `~/.local/share/solana-update/<update-pubkey>-<manifest_signature>/bin`
- `~/.local/share/solana/install/releases/<download_sha256>/` - содержимое релиза

### Интерфейс командной строки

```text
solana-install 0.16.0
The solana cluster software installer

USAGE:
    solana-install [OPTIONS] <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c, --config <PATH>    Configuration file to use [default: .../Library/Preferences/solana/install.yml]

SUBCOMMANDS:
    deploy    deploys a new update
    help      Prints this message or the help of the given subcommand(s)
    info      displays information about the current installation
    init      initializes a new installation
    run       Runs a program while periodically checking and applying software updates
    update    checks for an update, and if available downloads and applies it
```

```text
solana-install-init
initializes a new installation

USAGE:
    solana-install init [OPTIONS]

FLAGS:
    -h, --help    Prints help information

OPTIONS:
    -d, --data_dir <PATH>    Directory to store install data [default: .../Library/Application Support/solana]
    -u, --url <URL>          JSON RPC URL for the solana cluster [default: http://devnet.solana.com]
    -p, --pubkey <PUBKEY>    Public key of the update manifest [default: 9XX329sPuskWhH4DQh6k16c87dHKhXLBZTL3Gxmve8Gp]
```

```text
solana-install info
displays information about the current installation

USAGE:
    solana-install info [FLAGS]

FLAGS:
    -h, --help     Prints help information
    -l, --local    only display local information, don't check the cluster for new updates
```

```text
solana-install deploy
deploys a new update

USAGE:
    solana-install deploy <download_url> <update_manifest_keypair>

FLAGS:
    -h, --help    Prints help information

ARGS:
    <download_url>               URL to the solana release archive
    <update_manifest_keypair>    Keypair file for the update manifest (/path/to/keypair.json)
```

```text
solana-install update
checks for an update, and if available downloads and applies it

USAGE:
    solana-install update

FLAGS:
    -h, --help    Prints help information
```

```text
solana-install run
Runs a program while periodically checking and applying software updates

USAGE:
    solana-install run <program_name> [program_arguments]...

FLAGS:
    -h, --help    Prints help information

ARGS:
    <program_name>            program to run
    <program_arguments>...    arguments to supply to the program

The program will be restarted upon a successful software update
```
