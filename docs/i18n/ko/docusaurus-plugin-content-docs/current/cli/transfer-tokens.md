---
title: 토큰 주고 받기
---

이 페이지에서는 커맨드라인 지갑인 [ paper wallet](../wallet-guide/paper-wallet.md), [file system wallet](../wallet-guide/file-system-wallet.md) 또는 [hardware wallet](../wallet-guide/hardware-wallets.md)을 통해 SOL 토큰을 주고 받는 방법을 설명합니다. 시작하기 전에 지갑을 생성했고 해당 주소 (pubkey) 및 서명 키 쌍에 액세스 할 수 있는지 확인하세요. [ 다양한 지갑 유형에 대한 키 쌍을 입력하기위한 규칙 ](../cli/conventions.md#keypair-conventions)을 확인하세요.

## 지갑 테스팅

공개키를 다른 사람과 공유하기 전에 먼저 키가 유효하고 실제로 해당 개인 키를 보유하고 있는지 확인하는 것이 좋습니다.

이 예시에서는 첫 번째 지갑에 추가하여 두 번째 지갑을 생성한 다음 일부 토큰이 첫번째 지갑으로 전송됩니다. 이후 선택한 지갑 유형에서 토큰을 보내고받을 수 있음을 확인할 수 있습니다.

이 테스트 예제는 devnet이라는 개발자 테스트 넷을 사용합니다. devnet에서 발행 된 토큰은 가치가 **no** 임으로 분실에 걱정할 필요가 없습니다.

#### 시작하려면 일부 토큰을 에어드랍하세요.

먼저 devnet에서 테스트 토큰을 직접 *에어드랍*합니다.

```bash
solana airdrop 10 <RECIPIENT_ACCOUNT_ADDRESS> --url https://devnet.solana.com
```

여기서 `<RECIPIENT_ACCOUNT_ADDRESS>` 텍스트를 base58를 base58로 인코딩 된 공개키/지갑 주소로 바꿉니다.

#### 잔액 확인

계정 잔액을 확인하여 에어드랍이 성공했는지 확인하십시오. `10 SOL`이 출력되어야 합니다.

```bash
solana balance <ACCOUNT_ADDRESS> --url https://devnet.solana.com

```

#### 두번째 지갑 주소 생성

토큰을 받으려면 새 주소가 필요합니다. 두 번째 키 쌍을 만들고 해당 pubkey를 기록합니다.

```bash
solana-keygen new --no-passphrase --no-outfile
```

`pubkey :` 텍스트 뒤에 주소가 출력됩니다. 주소를 복사하고 안전하게 보관해두세요. 다음 단계에서 새로운 주소를 활용합니다.

```text
pubkey: GKvqsuNcnwWqPzzuhLmGi4rzzh55FhJtGizkhHaEJqiV

```

또한 두번째 지갑은 [ 종이 ](../wallet-guide/paper-wallet#creating-multiple-paper-wallet-addresses), [ 파일 시스템 ](../wallet-guide/file-system-wallet.md#creating-multiple-file-system-wallet-addresses), 또는 [ 하드웨어 ](../wallet-guide/hardware-wallets.md#multiple-addresses-on-a-single-hardware-wallet) 타입으로 만들 수 있습니다.

#### 첫번째 지갑에서 두번째 주소로 토큰 전송

다음으로, 지급 받은 토큰을 전송하여 토큰 소유 여부를 확실히 알아보겠습니다. 솔라나 클러스터는 트랜잭션에서 보낸 사람의 공개키에 해당하는 개인키 쌍으로 트랜잭션에 서명하는 경우에만 전송을 수락합니다.

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> 5 --url https://devnet.solana.com --fee-payer <KEYPAIR>
```

여기서 `<KEYPAIR>`를 첫 번째 지갑의 키 쌍 경로로 바꿉니다. 그리고 `<RECIPIENT_ACCOUNT_ADDRESS>`를 두 번째 주소로 바꿉니다.

`solana balance`로 업데이트 된 잔액을 확인합니다.

```bash
solana balance <ACCOUNT_ADDRESS> --url http://devnet.solana.com
```

여기서 `<ACCOUNT_ADDRESS>`는 키 쌍의 공개키 또는 수신자의 공개키입니다.

#### 테스트 전송의 전체 예시

```bash
$ solana-keygen new --outfile my_solana_wallet.json   # Creating my first wallet, a file system wallet
Generating a new keypair
For added security, enter a passphrase (empty for no passphrase):
Wrote new keypair to my_solana_wallet.json
==========================================================================
pubkey: DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK                          # Here is the address of the first wallet
==========================================================================
Save this seed phrase to recover your new keypair:
width enhance concert vacant ketchup eternal spy craft spy guard tag punch    # If this was a real wallet, never share these words on the internet like this!
==========================================================================

$ solana airdrop 10 DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com  # Airdropping 10 SOL to my wallet's address/pubkey
Requesting airdrop of 10 SOL from 35.233.193.70:9900
10 SOL

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com # Check the address's balance
10 SOL

$ solana-keygen new --no-outfile  # Creating a second wallet, a paper wallet
Generating a new keypair
====================================================================
Save this seed phrase to recover your new keypair:
clump panic cousin hurt coast charge engage fall eager urge win love   # If this was a real wallet, never share these words on the internet like this!
====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 5 --url https://devnet.solana.com --fee-payer my_solana_wallet.json  # Transferring tokens to the public address of the paper wallet
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # This is the transaction signature

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com
4.999995 SOL  # The sending account has slightly less than 5 SOL remaining due to the 0.000005 SOL transaction fee payment

$ solana balance ====================================================================

$ solana transfer --from my_solana_wallet.json 7S3P4HxJpyyigGzodYwHtCxZyUQe9JiBMHyRWXArAaKv 5 --url https://devnet.solana.com --fee-payer my_solana_wallet.json  # Transferring tokens to the public address of the paper wallet
3gmXvykAd1nCQQ7MjosaHLf69Xyaqyq1qw2eu1mgPyYXd5G4v1rihhg1CiRw35b9fHzcftGKKEu4mbUeXY2pEX2z  # This is the transaction signature

$ solana balance DYw8jCTfwHNRJhhmFcbXvVDTqWMEVFBX6ZKUmG5CNSKK --url https://devnet.solana.com
4.999995 SOL  # The sending account has slightly less than 5 SOL remaining due to the 0.000005 SOL transaction fee payment

$ solana balance

```

## 토큰 받기

토큰을 받으려면 다른 사람이 토큰을 보낼 주소가 필요합니다. 솔라나에서 지갑 주소는 키 쌍의 공개키입니다. 키 쌍을 생성하는 다양한 방법이 있습니다. 선택하는 방법은 키 쌍 저장 방법에 따라 다릅니다. 키 쌍은 지갑에 저장됩니다. 받기 전에 토큰의 경우 [지갑을 생성](../wallet-guide/cli.md)해야합니다. 완료되면 생성 한 각 키 쌍에 대한 공개키가 있어야합니다. 공개키는 base58 문자의 긴 문자열입니다. 길이는 32자에서 44자까지 다양합니다.

## 토큰 전송

이미 SOL을 보유하고 있고 다른 사람에게 토큰을 보내려면 키 쌍에 대한 경로, base58로 인코딩 된 공개 키 및 전송할 토큰이 필요합니다. 이후에 `solana transfer` 명령을 사용해 토큰을 전송할 수 있습니다:

```bash
solana transfer --from <KEYPAIR> <RECIPIENT_ACCOUNT_ADDRESS> <AMOUNT> --fee-payer <KEYPAIR>
```

` solana balance`: 업데이트 된 잔액을 확인합니다.

```bash
solana balance <ACCOUNT_ADDRESS>
```
