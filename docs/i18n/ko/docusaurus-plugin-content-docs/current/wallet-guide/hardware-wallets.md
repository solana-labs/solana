---
title: Solana CLI에서 하드웨어 지갑 사용
---

거래에 서명하려면 개인 키가 필요하지만 개인용 컴퓨터 나 전화에 개인 키를 저장하면 도난 당할 수 있습니다. 키에 암호를 추가하면 보안이 강화되지만 많은 사람들이 한 단계 더 나아가 개인 키를 *hardware wallet*이라는 별도의 물리적 장치로 옮기는 것을 선호합니다. 하드웨어 지갑은 개인 키를 저장하고 트랜잭션 서명을위한 인터페이스를 제공하는 소형 휴대용 장치입니다.

Solana CLI는 하드웨어 지갑에 대한 최고 수준의 지원을 제공합니다. 키 쌍 파일 경로 (사용 문서에서`<KEYPAIR>`로 표시됨)를 사용하는 모든 곳에서 하드웨어 지갑에서 키 쌍을 고유하게 식별하는 *keypair URL*을 전달할 수 있습니다.

## 지원되는 하드웨어 지갑

Solana CLI는 다음 하드웨어 지갑을 지원합니다.

- [Ledger Nano S and Ledger Nano X](hardware-wallets/ledger.md)

## 키 페어 URL 지정

-\[Ledger Nano S 및 Ledger Nano X\] (hardware-wallets / ledger.md)

Solana는 컴퓨터에 연결된 하드웨어 지갑에서 Solana 키 쌍을 고유하게 찾기 위해 키 쌍 URL 형식을 정의합니다.

```text
usb : // <MANUFACTURER> [/ <WALLET_ID>] [?
```

`WALLET_ID`는 여러 장치를 명확하게하는 데 사용되는 전역 적으로 고유 한 키입니다.

`DERVIATION_PATH`는 하드웨어 지갑 내에서 Solana 키로 이동하는 데 사용됩니다. 경로 형식은`<ACCOUNT> [/ <CHANGE>]`이며, 여기서 각`ACCOUNT` 및`CHANGE`는 양의 정수입니다.

예를 들어 Ledger 장치의 정규화 된 URL은 다음과 같습니다.

```text
usb : // ledger / BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?
```

모든 파생 경로에는 경로가 \[BIP44 사양\] (https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki)을 따르고 있음을 나타내는 접두사 '44'/ 501 ''이 암시 적으로 포함됩니다. 파생 된 키는 모두 Solana 키입니다 (Coin 유형 501). The single quote indicates a "hardened" derivation. Solana는 Ed25519 키 쌍을 사용하기 때문에 모든 파생이 강화되므로 따옴표 추가는 선택 사항이며 불필요합니다.
