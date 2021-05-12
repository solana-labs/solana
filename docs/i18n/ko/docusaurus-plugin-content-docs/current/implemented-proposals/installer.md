---
title: 클러스터 소프트웨어 설치 및 업데이트
---

현재 사용자는 git 저장소에서 솔라나 클러스터 소프트웨어를 직접 빌드하고 수동으로 업데이트해야하므로 오류가 발생하기 쉽고 불편합니다.

이 문서는 지원되는 플랫폼 용으로 미리 빌드 된 바이너리를 배포하는 데 사용할 수있는 사용하기 쉬운 소프트웨어 설치 및 업데이트 프로그램을 제안합니다. 사용자는 Solana 또는 자신이 신뢰하는 다른 당사자가 제공하는 바이너리를 사용하도록 선택할 수 있습니다. ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ ㅇㅇㅇ 업데이트 배포는 온 체인 업데이트 매니페스트 프로그램을 사용하여 관리됩니다.

## 동기 부여 예제

### 부트 스트랩 curl / shell 스크립트를 사용하여 미리 빌드 된 설치 프로그램 가져 오기 및 실행

지원되는 플랫폼을위한 가장 쉬운 설치 방법 :

```bash
$ curl -sSf https://raw.githubusercontent.com/solana- labs / solana / v1.0.0 / install / solana-install-init.sh |
쉬```이

스크립트는 최신 태그 버전 및 다운로드 github에를 확인하고 거기에서`솔라 설치-init` 바이너리를 실행합니다.
```

잘 알려진 릴리스 URL을 사용하면 지원되는 플랫폼에 대해 사전 빌드 된 바이너리를 얻을 수 있습니다

.`bash
$ curl -o solana-install-init https://github.com/solana-labs/solana/releases/download/v1.0.0/solana-install-init-x86_64-apple-darwin
$ chmod + x ./solana-- 초기화를
$ ./solana-install-init
--help`

```bash
#`솔라 설치 - 초기화 HTTPS의인수
...`$컬 -sSf : // raw.githubusercontent.com/solana-labs/solana/v1.0.0/install/solana-install-init.sh | sh -s-$ {init_args}
```

### Github 릴리스에서 사전 빌드 된 설치 프로그램 가져 오기 및 실행

빌드설치하고소스에서 설치 프로그램을 실행하면

```bash
$ curl -o solana-install-init https://github.com/solana-labs/solana/releases/download/v1.0.0/solana-install-init-x86_64-apple-darwin
$ chmod +x ./solana-install-init
$ ./solana-install-init --help
```

### 명령 줄 인터페이스

소스에서 설치 프로그램을 구축,사전 구축 된 바이너리가 특정 플랫폼에 사용할 수없는 경우항상 옵션입니다

```bash
$ git clone https://github.com/solana-labs/solana.git
$ cd solana/install
$ cargo run -- --help
```

### 배포 클러스터에 새 업데이트를

:`bash는
$ 자식 클론 https://github.com/solana-labs/solana.git
$ CD를 솔라는 /설치
$화물 실행-
--help`

```bash
$ solana-keygen new -o update-manifest.json  # <-- only generated once, the public key is shared with users
$ solana-install deploy http://example.com/path/to/solana-release.tar.bz2 update-manifest.json
```

### Run a validator node that auto updates itself

```bash
$ export PATH = ~ / .local / share / solana-install / bin : $ PATH
$ solana-keygen ... # &#060;-최신 solana를 실행합니다. -keygen
$ 솔라 설치 실행 솔라나 - 검증 ... # &#060;- 때 모든 necesary으로그것을 다시 시작, 발리 실행을
```

## ```적용될

매니페스트업데이트 매니페스트의 배포를 광고하는 데 사용됩니다솔라나 클러스터의 새 릴리스 타르볼. 업데이트 매니페스트는`config` 프로그램을 사용하여 저장되며 각 업데이트 매니페스트 계정은 주어진 타겟 트리플 \ (예 :`x86_64-apple-darwin` \)에 대한 논리적 업데이트 채널을 설명합니다. 계정 공개 키는 새 업데이트를 배포하는 엔터티와 해당 업데이트를 사용하는 사용자간에 잘 알려져 있습니다.

감안할 때 이미 공개적으로 액세스 할 수있는 URL에 업로드 한 다음 명령을 배포 할 업데이트 (CI / \-tarball.sh`을 게시`에 의해 만들어진 같은) 솔라 릴리스 타르볼은

```text
use solana_sdk :: signature :: Signature;

/// 주어진 업데이트를 다운로드하고 적용하는 데 필요한 정보
pub struct UpdateManifest {
    pub timestamp_secs : u64, // UNIX EPOCH 이후 몇 초 만에 릴리스가 배포 된 경우
    pub download_url : String, // 릴리스에 대한 URLtar.bz2
    다운로드pub download_sha256 : String, // 릴리스 tar.bz2 파일의 SHA256 다이제스트
}

/// 업데이트 매니페스트 프로그램 계정의 데이터입니다.
# [derive (Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct SignedUpdateManifest {
    pub manifest : UpdateManifest,
    pub manifest_signature : Signature,
}
```

\```bash는 $의 새로운 -o를 솔라-keygen은 update-manifest.json # <-한 번만 생성되며 공개 키는 사용자와 공유됩니다 $ solana-in 스톨 배포 http://example.com/path/to/solana-release.tar.bz2 갱신의 manifest.json```자동

업데이트 설치하는검증 노드를 실행 ###

## 릴리스 아카이브 내용

릴리스 아카이브는 다음 내부 구조를 가진 bzip2로 압축 된 tar 파일로 예상됩니다.

- `/ version.yml`-필드`"target"`을 포함하는 간단한 YAML 파일-

  대상 튜플. 추가 필드는 무시됩니다.

- `/ bin /`-릴리스에서 사용 가능한 프로그램이 포함 된 디렉토리입니다.

  `solana-install`은하기 위해이 디렉토리를심볼릭 링크

  `PATH` 환경에서 사용`~ / .local / share / solana-install / bin`에

  변수합니다.

- ...`-모든 추가 파일 및 디렉토리가 허용됩니다.

## -`## solana-install 도구

업데이트가에체인 업데이트

사용자의 홈 디렉토리에서 다음 파일 및 디렉토리를 관리합니다.

- `solana-install` 도구는 사용자가 클러스터 소프트웨어를 설치하고 업데이트하는 데 사용됩니다.
- `~ / .config / solana / install / config.yml`-현재 설치된 소프트웨어 버전에 대한 사용자 구성 및 정보-`~ / .local / share / solana / install / bin`-현재 릴리스에 대한 심볼릭 링크입니다. 예 :`~ / .local / share / solana-update / <update-pubkey>-<manifest_signature> / bin`- `~ / .local / share / solana / install / releases / <download_sha256> /`- release
- `~/.local/share/solana/install/releases/<download_sha256>/` - contents of a release

### Command-line Interface

```text
solana-install 0.16.0
solana 클러스터 소프트웨어 설치 프로그램

사용법 :
    solana-install [옵션] <SUBCOMMAND>

플래그 :
    -h, --help 도움말 정보 인쇄
    -V,- version 버전 정보를 인쇄합니다. 옵션 :
    -c, --config <경로> 사용할 구성 파일 [기본값 : ... / Library / Preferences / solana / install.yml]

하위 명령 :배포이
    새 업데이트배포
    도움말메시지 또는 도움말을 인쇄합니다.주어진 하위 명령의 (들)
    현재 설치  대한정보를 표시 정보를
    초기화에새로 설치
    실행가능한주기적으로 확인하고소프트웨어 업데이트를적용하는 동안 프로그램을
    다운로드를업데이트를업데이트 확인을실행하고있는경우  적용


초기화과``````텍스트를
solana-install-init
는 새 설치를 초기화합니다.
```

```text
사용법 :
    solana-install init [옵션]

플래그 :
    -h, --help 도움말 정보를 인쇄합니다. 옵션 :
    -d, --dat a_dir <경로> 설치 데이터를 저장할 디렉토리 [기본값 : ... / Library / Application Support / solana]
    -u, --url <URL> solana 클러스터의 JSON RPC URL [기본값 : http : //devnet.solana. com]
    -p, --pubkey <PUBKEY> 업데이트 매니페스트의 공개 키 [기본값 : 9XX329sPuskWhH4DQh6k16c87dHKhXLBZTL3Gxmve8Gp]
```

```text
solana-install info정보를
는 현재 설치에 대한표시합니다. USAGE :
    solana-install info [FLAGS]

FLAGS :
    -h, --help 도움말 정보 인쇄
    -l, --local 로컬 정보 만 표시, 클러스터에서 새 업데이트 확인 안 함
```

```text
solana-install deploy deploys
a new update

USAGE :
    solana-install deploy <download_url> <update_manifest_keypair>

FLAGS :
    -h, --help 도움말 정보를 인쇄합니다. ARGS :
    <download_url> solana 릴리스 아카이브의 URL
    <update_manifest_keypair> 업데이트 매니페스트 용 키쌍 파일 (/path/to/keypair.json)
```

```text
solana-install update업데이트를
는확인하고 사용 가능한 경우 다운로드하여 적용합니다. USAGE :
    solana-install update

FLAGS :
    -h, --help Prints도움말


정보는``````실행텍스트는
솔라 설치
주기적으로 확인하고 소프트웨어 업데이트를적용하는 동안 프로그램을 실행합니다

사용을:
    솔라가 설치 <program_name은> [program_arguments] ...실행

:FLAGS
    -h, --help 도움말을 인쇄 정보

ARGS:
    <program_name> 실행할 프로그램
    <program_arguments> ...
```

```text
solana-install update업데이트를
는확인하고 사용 가능한 경우 다운로드하여 적용합니다.

USAGE :
    solana-install update

FLAGS :
    -h, --help Prints도움말


정보는``````실행텍스트는
솔라 설치
주기적으로 확인하고 소프트웨어 업데이트를적용하는 동안 프로그램을 실행합니다

사용을:
    솔라가 설치 <program_name은> [program_arguments] ...실행

:FLAGS
    -h, --help 도움말을 인쇄 정보

ARGS:
    <program_name> 실행할 프로그램
    <program_arguments> ...
```
