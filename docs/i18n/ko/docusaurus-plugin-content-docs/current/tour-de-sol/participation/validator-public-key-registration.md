---
title: 유효성 검사기 공개 키 만들기
---

참여하려면 먼저 등록해야합니다. \[등록 정보\] (../ registration / how-to-register.md)를 참조하세요.

SOL 할당량을 얻으려면 keybase.io 계정에 유효성 검사기의 신원 공개 키를 게시해야합니다.

## \***\* 키쌍 생성 \*\***

1. 아직 생성하지 않은 경우 다음을 실행하여 유효성 검사기의 ID 키 쌍을 생성합니다.

   ```bash
     solana-keygen 새로운 -o ~ / validator-keypair.json
   ```

2. 이제 다음을 실행하여 ID 공개 키를 볼 수 있습니다.

   ```bash
     solana-keygen pubkey ~ / validator-keypair.json
   ```

> 참고 : "validator-keypair.json"파일은 \ (ed25519 \) 개인 키이기도합니다.

유효성 검사기 ID 키 쌍은 네트워크 내에서 유효성 검사기를 고유하게 식별합니다. **It is crucial to back-up this information.**

이 정보를 백업하지 않으면 해당 정보에 대한 액세스 권한을 잃어 버리면 유효성 검사기를 복구 할 수 없습니다. 이런 일이 발생하면 SOL 할당도 잃게됩니다.

유효성 검사기 식별 키 쌍을 백업하려면 \*\* "validator-keypair.json"파일을 안전한 위치에 백업하십시오.

## Solana Pubkey를 Keybase 계정에 연결

Solana pubkey를 Keybase.io 계정에 연결해야합니다. 다음 지시 사항은 서버에 Keybase를 설치하여이를 수행하는 방법을 설명합니다.

1. 컴퓨터에 \[Keybase\] (https://keybase.io/download)를 설치합니다.
2. 서버에서 Keybase 계정에 로그인하십시오. 아직 Keybase 계정이없는 경우 먼저 Keybase 계정을 만듭니다. 다음은 \[기본 Keybase CLI 명령어 목록\] (https://keybase.io/docs/command_line/basics)입니다.
3. 공용 파일 폴더에 Solana 디렉토리를 만듭니다.`mkdir / keybase / public / <KEYBASE_USERNAME> / solana`
4. Keybase 공용 파일 폴더에`/ keybase / public / <KEYBASE_USERNAME> / solana / validator- <BASE58_PUBKEY>`형식으로 빈 파일을 만들어 유효성 검사기의 ID 공개 키를 게시합니다. 예를 들면 :

   ```bash
     / keybase / public / <KEYBASE_USERNAME> / solana / validator- <BASE58_PUBKEY>를 터치합니다.
   ```

5. 공개 키가 게시되었는지 확인하려면`https://keybase.pub/ <KEYBASE_USERNAME> / solana / validator- <BASE58_PUBKEY>`로 이동할 수 있는지 확인합니다.
