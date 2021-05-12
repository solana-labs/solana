---
title: 유효성 검사기 정보 게시
---

다른 사용자가 공개적으로 볼 수 있도록 유효성 검사기 정보를 체인에 게시 할 수 있습니다.

## solana validator-info 실행

solana CLI를 실행하여 유효성 검사기 정보 계정을 채 웁니다.

```bash
solana validator-info publish --keypair ~ / validator-keypair.json <VALIDATOR_INFO_ARGS> <VALIDATOR_NAME>
```

VALIDATOR_INFO_ARGS의 선택적 필드에 대한 자세한 내용은 다음을 참조하세요.

```bash
솔라나 밸리데이터 정보 게시-도움말
```

## 예제 명령

게시 명령의 예 :

```bash
solana validator-info 게시 "Elvis Validator"-n elvis -w "https://elvis-validates.com"
```

쿼리 명령 예 :

```bash
솔라나 밸리데이터 정보 얻기
```

어느 출력

```text
8WdJvDz6obhADdxpGCiJKZsDYwTLNEDFizayqziDc9ah의 유효성 검사기 정보
  유효성 검사기 pubkey : 6dMH3u76qZ7XG4bVboVRnBHR2FfrxEqTTTyj4xmyDMWo
  정보 : { "keybaseUsername": "elvis", "name": "Elvis Validator", "website": "https://elvis-validates.com"}
```

## 키베이스

Keybase 사용자 이름을 포함하면 클라이언트 애플리케이션 \ (예 : Solana Network Explorer \)이 암호화 증명, 브랜드 아이덴티티 등을 포함한 유효성 검사기 공개 프로필을 자동으로 가져올 수 있습니다. 유효성 검사기 pubkey를 Keybase와 연결하려면 :

1. [https://keybase.io/](https://keybase.io/)에 가입하고 유효성 검사기 프로필 작성
2. 유효성 검사기 ** identity pubkey **를 Keybase에 추가합니다.

   - CLI는`validator- <PUBKEY>`파일을 확인합니다.
   - -Keybase에서 파일 섹션으로 이동하여 pubkey 파일을

     공용 폴더의`solana` 하위 디렉토리 :`/ keybase / public / <KEYBASE_USERNAME> / solana`

   - -pubkey를 확인하려면 다음을 성공적으로 탐색 할 수 있는지 확인하십시오.

     `https://keybase.pub/ <KEYBASE_USERNAME> / solana / validator- <PUBKEY>`

3. Keybase 사용자 이름으로`solana validator-info`를 추가하거나 업데이트합니다. 그만큼

   -로컬 컴퓨터에`validator- <PUBKEY>`라는 빈 파일을 만듭니다.
