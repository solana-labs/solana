---
title: Установите набор инструментов Solana
---

Есть несколько способов установки инструментов Solana на вашем компьютере в зависимости от предпочитаемого вами рабочего процесса:

- [Использование инструмента установки Solana (самый простой способ)](#use-solanas-install-tool)
- [Скачать готовые бинарные файлы](#download-prebuilt-binaries)
- [Собрать из исходного кода](#build-from-source)

## Использование инструмента установки Solana

### MacOS & Linux

- Откройте любимое приложение Terminal

- Установите релиз Solana [LATEST_SOLANA_RELEASE_VERSION](https://github.com/solana-labs/solana/releases/tag/LATEST_SOLANA_RELEASE_VERSION) на вашей машине:

```bash
sh -c "$(curl -sSfL https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/install)"
```

- Вы можете заменить `LATEST_SOLANA_RELEASE_VERSION` на тег выпуска, соответствующий программной версии желаемого релиза, или используйте одно из трех символьных названий каналов: `стабильная`, `бета`, или `край`.

- Следующий вывод указывает на успешное обновление:

```text
загрузка установки LATEST_SOLANA_RELEASE_VERSION
Конфигурация: /home/solana/.config/solana/install/config.yml
Каталог активных релизов: /home/solana/.local/share/solana/install/active_release
* Версия релиз: LATEST_SOLANA_RELEASE_VERSION
* URL-адрес релиза: https://github.com/solana-labs/solana/releases/download/LATEST_SOLANA_RELEASE_VERSION/solana-release-x86_64-unknown-linux-gnu.tar.bz2
Успешное обновление обновлений
```

- В зависимости от вашей системы, конец сообщений программы установки может попросить вас

```bash
Пожалуйста, обновите переменную окружения PATH для включения программ Solana:
```

- Если вы получите вышеуказанное сообщение, скопируйте и вставьте рекомендованную команду ниже для обновления `PATH`
- Подтвердите, что нужная версия `solana` установлена, запущена:

```bash
solana --version
```

- После успешной установки, `solana-install update` может быть использован для легкого обновления программного обеспечения Solana до новой версии в любое время.

---

### Windows

- Открыть командную строку (`cmd.exe`) в качестве администратора

  - Поиск команд в строке поиска Windows. Когда появляется приложение команды Подсказки, щелкните правой кнопкой мыши и выберите «Открыть как администратор». Если у вас появилось всплывающее окно с просьбой «Разрешите этому приложению вносить изменения на вашем устройстве?», нажмите Да.

- Скопируйте и вставьте следующую команду, затем нажмите Enter для загрузки программы установки Solana во временный каталог:

```bash
curl https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/solana-install-init-x86_64-pc-windows-msvc.exe --output C:\solana-install-tmp\solana-install-init.exe --create-dirs
```

- Скопируйте и вставьте следующую команду, затем нажмите Enter для установки последней версии Solana. Если вы видите всплывающее окно системы безопасности, выберите, чтобы разрешить запуск программы.

```bash
C:\solana-install-tmp\solana-install-init.exe LATEST_SOLANA_RELEASE_VERSION
```

- После завершения установки нажмите Enter.

- Закройте окно командной строки и заново откройте новое окно командной строки как обычный пользователь
  - Поиск "Командная строка" в строке поиска, а затем щелкните левой кнопкой мыши на значке приложения "Командная строка", не нужно запускать как администратор)
- Подтвердите, что нужная версия `solana` установлена, запущена:

```bash
solana --version
```

- После успешной установки, `solana-install update` может быть использован для легкого обновления программного обеспечения Solana до новой версии в любое время.

## Скачать готовые бинарные файлы

Если вы не хотите использовать `solana-install` для управления установкой, вы можете загрузить и установить исполняемые файлы.

### Linux

Загрузите бинарные файлы, перейдя по ссылке [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), скачайте **solana-release-x86_64-unknown-linux-msvc.tar.bz2**, затем распакуйте архив:

```bash
tar jxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### MacOS

Загрузите бинарные файлы, перейдя по ссылке [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), скачайте **solana-release-x86_64-apple-darwin.tar.bz2**, затем распакуйте архив:

```bash
tar jxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### Windows

- Загрузите бинарные файлы, перейдя по ссылке [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), скачайте **solana-release-x86_64-pc-windows-msvc.tar.bz2**, затем распакуйте архив.

- Откройте командную строку и перейдите в каталог, в который вы распаковали бинарные файлы и выполните:

```bash
cd solana-release/
установить PATH=%cd%/bin;%PATH%
```

## Собрать из исходного кода

Загрузите бинарные файлы, перейдя по ссылке [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), скачайте **solana-release-x86_64**, затем распакуйте архив. Извлечь код и собрать файлы с:

```bash
./scripts/cargo-install-all.sh .
экспорт PATH=$PWD/bin:$PATH
```

Затем вы можете выполнить следующую команду, чтобы получить тот же результат, что и с предварительно собранными бинарными файлами:

```bash
init solana-install
```
