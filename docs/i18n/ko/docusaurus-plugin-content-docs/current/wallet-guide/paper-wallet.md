---
title: Paper Wallet
---

이 문서는 Solana CLI 도구로 종이 지갑을 만들고 사용하는 방법을 설명합니다.

> 우리는 종이 지갑을 _ 안전하게 _ 생성 또는 관리하는 방법에 대해 조언하지 않습니다. 보안 문제를주의 깊게 조사하십시오.

## 개요

Solana는 BIP39 호환 시드 문구에서 키를 추출하는 키 생성 도구를 제공합니다. 밸리데이터를 실행하고 토큰을 스테이킹하기위한 Solana CLI 명령은 모두 시드 문구를 통한 키 쌍 입력을 지원합니다.

BIP39 표준에 대해 자세히 알아 보려면 Bitcoin BIPs Github 저장소 \[여기\] (https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki)를 방문하세요.

## 종이 지갑 사용

Solana 명령은 키 쌍을 컴퓨터의 디스크에 저장하지 않고도 실행할 수 있습니다. 개인 키를 디스크에 쓰는 것을 피하는 것이 보안 문제라면 올바른 위치에 온 것입니다.

> 이 보안 입력 방법을 사용하더라도 암호화되지 않은 메모리 스왑에 의해 개인 키가 디스크에 기록 될 수 있습니다. 이 시나리오로부터 보호하는 것은 사용자의 책임입니다.

## 시작하기 전에

- [Install the Solana command-line tools](../cli/install-solana-cli-tools.md)

### 설치

-\[Solana 명령 줄 도구 설치\] (../ cli / install-solana-cli-tools.md)

````bash
```bash는
솔라 - Keygen은
````

## 지원

keygen` 도구를 만들기,그것은뿐만 아니라 기존의 종자 구문과에서 키 쌍을 도출으로 새로운 종자 문구를 생성 할 수 있습니다 (선택 ) 암호. 시드 문구와 패스 프레이즈는 종이 지갑으로 함께 사용할 수 있습니다. 시드 문구와 암호를 안전하게 보관하는 한이를 사용하여 계정에 액세스 할 수 있습니다.

> 시드 문구 작동 방식에 대한 자세한 내용은이 \[Bitcoin Wiki 페이지\] (https://en.bitcoin.it/wiki/Seed_phrase)를 참조하세요.

### 시드 구문 생성

`solana-keygen new` 명령을 사용하여 새 키 쌍을 생성 할 수 있습니다. 이 명령은 임의의 시드 문구를 생성하고 선택적 암호를 입력하도록 요청한 다음 파생 된 공개 키와 종이 지갑에 대해 생성 된 시드 문구를 표시합니다.

--version```##은`솔라사용하여종이 지갑

````bash
:```bash는
솔라나 - Keygen은
````

> 보안을 강화하기 위해내용은`--word-count`인수를사용하여 종자 구문 단어 수를 증가

(file-system-wallet.md)이명령의 출력이 같은 선을 표시한다

```bash
:<code>배시
pubkey :
9ZNTfG4NyQgxy2SWjSiQoUyBPEvXT2xo7fKc5hPYYJ7b</code>후
```

후

시드 구문을 복사 한 후 \[공개 키 파생\] (# public-key-derivation) 지침을 사용하여 오류가 없는지 확인할 수 있습니다.

> 네트워크로 연결된 컴퓨터에서 쉽게 사용할 수 있도록 파생 된 주소를 USB 스틱에 복사합니다.

생략하면`bash는 솔라-keygen은 새로운 --no-OUTFILE은`>,

```bash
:<code>bash는
솔라-keygen은 새로운
--help</code>
```

### 공개 키 유도

공개 키를 수 사용하기로 선택한 경우 시드 문구와 암호에서 파생됩니다. This is useful for using an offline-generated seed phrase to derive a valid public key. The `solana-keygen pubkey` command will walk you through how to use your seed phrase (and a passphrase if you chose to use one) as a signer with the solana command-line tools using the `ask` uri scheme.

```bash
solana-keygen pubkey prompt://
```

> 잠재적으로 같은 종자 문구에 대해 서로 다른 암호 문구를 사용할 수 있습니다. 각각의 고유 한 암호는 다른 키 쌍을 생성합니다.

`solana-keygen` 도구는 시드 구문을 생성하는 것과 동일한 BIP39 표준 영어 단어 목록을 사용합니다. 시드 문구가 다른 단어 목록을 사용하는 다른 도구로 생성 된 경우 여전히`solana-keygen`을 사용할 수 있지만`--skip-seed-phrase-validation` 인수를 전달하고이 유효성 검증을 생략해야합니다.

```bash
solana-keygen pubkey prompt:// --skip-seed-phrase-validation
```

After entering your seed phrase with `solana-keygen pubkey prompt://` the console will display a string of base-58 character. This is the base _wallet address_ associated with your seed phrase.

> Copy the derived address to a USB stick for easy usage on networked computers

> 일반적인 다음 단계는 공개 키와 연결된 계정의 \[잔액 확인\] (# checking-account-balance)입니다.

** 참고 : ** 종이 지갑 및 파일 시스템 지갑으로 작업 할 때 "pubkey"와 "wallet address"라는 용어는 때때로 같은 의미로 사용됩니다.

```bash
solana-keygen pubkey --help
```

### Hierarchical Derivation

The solana-cli supports [BIP32](https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki) and [BIP44](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki) hierarchical derivation of private keys from your seed phrase and passphrase by adding either the `?key=` query string or the `?full-path=` query string.

By default, `prompt:` will derive solana's base derivation path `m/44'/501'`. To derive a child key, supply the `?key=<ACCOUNT>/<CHANGE>` query string.

```bash
solana-keygen pubkey prompt://?key=0/1
```

To use a derivation path other than solana's standard BIP44, you can supply `?full-path=m/<PURPOSE>/<COIN_TYPE>/<ACCOUNT>/<CHANGE>`.

```bash
solana-keygen pubkey prompt://?full-path=m/44/2017/0/1
```

Because Solana uses Ed25519 keypairs, as per [SLIP-0010](https://github.com/satoshilabs/slips/blob/master/slip-0010.md) all derivation-path indexes will be promoted to hardened indexes -- eg. `?key=0'/0'`, `?full-path=m/44'/2017'/0'/1'` -- regardless of whether ticks are included in the query-string input.

## 키 쌍을 확인하는 것은

솔라

```bash
solana-keygen verify <PUBKEY> prompt://
```

where `<PUBKEY>` is replaced with the wallet address and the keyword `prompt://` tells the command to prompt you for the keypair's seed phrase; `key` and `full-path` query-strings accepted. Note that for security reasons, your seed phrase will not be displayed as you type. After entering your seed phrase, the command will output "Success" if the given public key matches the keypair generated from your seed phrase, and "Failed" otherwise.

## 계정 잔액 확인 계정 잔액

을 확인하는 데 필요한 모든 것은 계정의 공개 키입니다. 종이 지갑에서 공개 키를 안전하게 검색하려면 \[에어 갭 컴퓨터\] (https://en.wikipedia.org/wiki/Air_gap_ (networking))에서 \[공개 키 파생\] (# public-key-derivation) 지침을 따르세요. 그런 다음 공개 키를 수동으로 입력하거나 USB 스틱을 통해 네트워크로 연결된 컴퓨터로 전송할 수 있습니다.

전체 사용 자세한 실행할

```bash
<code>bash
solana config set --url <CLUSTER URL> # (ie https :
//api.mainnet-beta.solana.com)</code>마지막으로,
```

`bash는 솔라 - Keygen은 pubkey ASK`>은

```bash
solana balance <PUBKEY>
```

## 여러 종이 지갑 주소를 생성

만들 수 있습니다 원하는만큼의 지갑 주소. \[Seed Phrase Generation\] (# seed-phrase-generation) 또는 \[Public Key Derivation\] (# public-key-derivation)의 단계를 다시 실행하여 새 주소를 만듭니다. 다른 목적으로 자신의 계정간에 토큰을 전송하려는 경우 여러 지갑 주소가 유용 할 수 있습니다.

## Support

다음으로`solana` CLI 도구를 구성하여 \[특정 클러스터에 연결\] (../ cli / choose-a-cluster.md) :
