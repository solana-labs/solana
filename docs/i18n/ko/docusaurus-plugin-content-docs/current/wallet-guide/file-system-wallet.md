---
title: 파일 시스템 지갑
---

이 문서는 Solana CLI 도구로 파일 시스템 지갑을 만들고 사용하는 방법을 설명합니다. 파일 시스템 지갑은 컴퓨터 시스템의 파일 시스템에 암호화되지 않은 키 쌍 파일로 존재합니다.

> 파일 시스템 지갑은 SOL 토큰을 저장하는 ** 가장 안전한 ** 방법입니다. 파일 시스템 지갑에 많은 양의 토큰을 저장하는 것은 ** 권장하지 않습니다 **.

## 시작하기 전에

\[Solana 명령 줄 도구를 설치\] (../ cli / install-solana-cli-tools.md)

## 파일 시스템 지갑 키 쌍 생성

Solana의 명령 줄 도구 'solana-keygen'을 사용하여 키 쌍 파일을 생성합니다. 예를 들어 명령 줄 셸에서 다음을 실행합니다.

```bash
mkdir ~ / my-solana-wallet
solana-keygen 새로운 --outfile ~ / my-solana-wallet / my-keypair.json
```

이 파일에는 ** 암호화되지 않은 ** 키 쌍이 포함되어 있습니다. 실제로 암호를 지정하더라도 해당 암호는 파일이 아닌 복구 시드 문구에 적용됩니다. 이 파일을 다른 사람과 공유하지 마십시오. 이 파일에 대한 액세스 권한이있는 모든 사용자는 공개 키로 전송 된 모든 토큰에 액세스 할 수 있습니다. 대신 공개 키만 공유해야합니다. 공개 키를 표시하려면 다음을 실행하십시오.

```bash
solana-keygen pubkey ~ / my-solana-wallet / my-keypair.json
```

다음과 같은 문자열을 출력합니다.

```text
ErRr1caKzK8L8nn4xmEWtimYRiTCAZXjBtVphuZ5vMKy
```

`~ / my-solana-wallet / my-keypair.json`의 키 쌍에 해당하는 공개 키입니다. 키 쌍 파일의 공개 키는 _wallet address_입니다.

## 키 페어 파일에 대해 주소 확인

주어진 주소에 대한 개인 키를 보유하고 있는지 확인하려면`solana-keygen verify`를 사용하세요.

```bash
solana-keygen 확인 <PUBKEY> ~ / my-solana-wallet / my-keypair.json
```

여기서`<PUBKEY>`는 지갑 주소로 대체됩니다. 이 명령은 주어진 주소가 키쌍 파일의 주소와 일치하면 "Success"를 출력하고 그렇지 않으면 "Failed"를 출력합니다.

## 여러 파일 시스템 지갑 주소 생성

원하는만큼 지갑 주소를 만들 수 있습니다. \[파일 시스템 지갑 생성\] (# generate-a-file-system-wallet-keypair)의 단계를 다시 실행하고`--outfile` 인수와 함께 새 파일 이름 또는 경로를 사용해야합니다. 다른 목적으로 자신의 계정간에 토큰을 전송하려는 경우 여러 지갑 주소가 유용 할 수 있습니다.
