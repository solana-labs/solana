---
title: Solana Tool Suite 설치하기
---

여러분이 선호하는 워크플로우에 따라서 다양한 방식으로 솔라나 툴을 설치할 수 있습니다:

- [솔라나 설치 툴 사용하기 (가장 쉬운 옵션)](#use-solanas-install-tool)
- [프리빌트 바이너리 다운로드하기](#download-prebuilt-binaries)
- [소스로부터 개발하기](#build-from-source)

## 솔라나 설치 툴 사용하기

### MacOS & 리눅스

- 가장 선호하는 터미널을 실행합니다.

- 다음 명령어로 기기에 솔라나를 설치합니다. [LATEST_SOLANA_RELEASE_VERSION](https://github.com/solana-labs/solana/releases/tag/LATEST_SOLANA_RELEASE_VERSION)

```bash
sh -c "$(curl -sSfL https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/install)"
```

- 여러분이 원하는 소프트웨어 버전에 맞춰 `LATEST_SOLANA_RELEASE_VERSION` 부분을 치환하거나, 가장 많이 쓰이는 다음 3가지 채널명 중 선택하세요. `stable`, `beta`, 또는 `edge`.

- 다음과 같은 출력이 나온다면 성공적으로 업데이트 된 것입니다.

```text
downloading LATEST_SOLANA_RELEASE_VERSION installer
Configuration: /home/solana/.config/solana/install/config.yml
Active release directory: /home/solana/.local/share/solana/install/active_release
* Release version: LATEST_SOLANA_RELEASE_VERSION
* Release URL: https://github.com/solana-labs/solana/releases/download/LATEST_SOLANA_RELEASE_VERSION/solana-release-x86_64-unknown-linux-gnu.tar.bz2
Update successful
```

- 여러분의 시스템에 따라 설치 마지막 메시징은 다음과 같은 요구가 나올 수 있습니다.

```bash
Please update your PATH environment variable to include the solana programs:
```

- 위와 같은 메시지 발생 시, 아래의 권장 커맨드를 복사하고 붙여넣어 `PATH`를 업데이트 합니다.
- 원하는 `solana` 버전이 설치되었는지 확인하려면 다음 커맨드를 실행합니다:

```bash
solana --version
```

- 성공적인 설치 이후 `solana-install update`를 통해 솔라나 소프트웨어를 언제나 손쉽게 업데이트 할 수 있습니다.

---

### Windows

- 관리자 권한으로 커맨드 프롬프트(`cmd.exe`)를 실행합니다.

  - 윈도우즈 검색창에서 커맨드 프롬프트를 검색합니다. 커맨드 프롬프트 앱이 표시되면 우클릭 한 뒤 "관리자 권한으로 실행"을 클릭합니다. "이 앱이 디바이스를 변경할 수 있도록 허용하시겠어요?" 라는 문구 팝업이 뜰 시 예라고 클릭합니다.

- 다음 커맨드를 복사하여 붙여넣고, 엔터 키를 누르면 솔라나 인스톨러를 임시 디렉토리에 다운로드 합니다:

```bash
curl https://release.solana.com/LATEST_SOLANA_RELEASE_VERSION/solana-install-init-x86_64-pc-windows-msvc.exe --output C:\solana-install-tmp\solana-install-init.exe --create-dirs
```

- 다음 커맨드를 복사하여 붙여넣고, 엔터 키를 누르면 솔라나의 최신 버전을 설치하게 됩니다. 만일 시스템 보안 관련 팝업이 뜨면 허용하여 프로그램이 실행되도록 해줍니다.

```bash
C:\solana-install-tmp\solana-install-init.exe LATEST_SOLANA_RELEASE_VERSION
```

- 설치가 끝나면 엔터 키를 누릅니다.

- 커맨드 프롬프트 창을 닫고 일반 유저로서 새로운 커맨드 프롬프트 창을 엽니다.
  - "커맨드 프롬프트"를 검색창으로 검색하고 아이콘을 좌클릭하여 실행합니다. 관리자 권한으로 실행은 필요 없습니다.
- 여러분이 원하는 버전의 `solana`가 설치되었는지 다음 커맨드를 통해 알아보세요:

```bash
solana --version
```

- 설치 완료 후 `solana-install update`커맨드를 통해 신규 솔라나 소프트웨어로 언제든지 업데이트 할 수 있습니다.

## 프리빌트 바이너리 다운로드

`solana-install`로 설치를 진행하고 싶지 않다면 바이너리를 직접 다운로드하여 설치할 수 있습니다.

### 리눅스

다음 경로에서 바이너리를 내려받습니다 [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), 다음 **solana-release-x86_64-unknown-linux-msvc.tar.bz2**를 다운로드 받은 뒤 아카이브를 추출합니다:

```bash
tar jxf solana-release-x86_64-unknown-linux-gnu.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### MacOS

다음 경로에서 바이너리를 내려받습니다 [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), 다음 **solana-release-x86_64-apple-darwin.tar.bz2**를 다운로드 받은 뒤 아카이브를 추출합니다:

```bash
tar jxf solana-release-x86_64-apple-darwin.tar.bz2
cd solana-release/
export PATH=$PWD/bin:$PATH
```

### Windows

- 다음 경로에서 바이너리를 내려받습니다 [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest), 다음 **solana-release-x86_64-pc-windows-msvc.tar.bz2**를 다운로드 받은 뒤 WinZip과 같은 앱을 사용해 아카이브를 추출합니다.

- 커맨드 프롬프트를 실행하고 바이너리를 추출한 디렉토리로 간 다음 다음을 실행합니다:

```bash
cd solana-release/
set PATH=%cd%/bin;%PATH%
```

## 소스로부터 개발하기

만일 프리빌트 바이너리를 사용할 수 없거나 소스로부터 스스로 개발하고 싶다면 다음 경로로 이동 합니다 [https://github.com/solana-labs/solana/releases/latest](https://github.com/solana-labs/solana/releases/latest). 이후 **소스 코드** 아카이브를 다운로드 합니다. 코드를 추출한 후 다음으로 바이너리를 생성합니다:

```bash
./scripts/cargo-install-all.sh .
export PATH=$PWD/bin:$PATH
```

이후 다음 커멘드를 실행하면 프리빌트 바이너리의 경우와 같은 결과를 얻을 수 있습니다:

```bash
solana-install init
```
