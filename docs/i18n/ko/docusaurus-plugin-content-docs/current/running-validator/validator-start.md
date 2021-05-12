---
title: 밸리데이터 시작하기
---

## Solana CLI 구성

solana cli에는 cli 명령에 대한`--url` 인수를 자동으로 설정하는`get` 및`set` 구성 명령이 포함되어 있습니다. 예를

```bash
solana config set --url http://devnet.solana.com
```

들어`bash는 솔라 설정 세트 --url http://devnet.solana.com`이

## 설치 확인

Before attaching a validator node, sanity check that the cluster is accessible to your machine by fetching the transaction count:

```bash
sudo bash -c "cat> /etc/sysctl.d/20-solana-udp-buffers.conf << EOF
# UDP 버퍼 크기 증가
net.core .rmem_default = 134217728
net.core.rmem_max = 134217728
net.core.wmem_default = 134217728
net.core.wmem_max = 134217728
EOF "
```

클러스터입니다 연결 가능 확인 ##

## ```

발리 노드, 클러스터가 트랜잭션의 수를 가져 오는하여 시스템에 액세스 할 수 있는지 전성 검사를 장착하기 전에

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
# Press ^C to exit
```

## 추가 Solana CLI 구성

:`bash는 솔라 거래 카운트`에서

[통계 대시 보드]보기 (HTTPS : /을 클러스터 활동에 대한 자세한 내용은 /metrics.solana.com:3000/d/monitor/cluster-telemetry)를 참조하십시오.

## 공중 투하 및 확인 검사기 밸런스

### 열려있는 모든 세션은(다시에, 다음 로그 아웃)

#### Automatic

다음 명령을 실행하여 가십 네트워크에 가입하고 클러스터의 다른 모든 노드를보십시오 :

The daemon (`solana-sys-tuner`) is included in the solana binary release. Restart it, _before_ restarting your validator, after each software upgrade to ensure that the latest recommended settings are applied.

`bash sudo sysctl -p /etc/sysctl.d/20-solana-udp -buffers.conf`

````bash
실행하려면 :

```bash
sudo solana-sys-tuner --user $ (whoami)> sys-tuner.log 2> & 1 &
````

#### Manual

** 증가 된 메모리 ** 파일이 제한 매핑```bash는 sudo는 bash는 -c "고양이> /etc/sysctl.d/20-solana-mmaps.conf << EOF

##### **\*\*** UDP 버퍼 증가 **\*\***

```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-udp-buffers.conf <<EOF
# Increase UDP buffer size
net.core.rmem_default = 134217728
net.core.rmem_max = 134217728
net.core.wmem_default = 134217728
net.core.wmem_max = 134217728
EOF"
```

```bash
/etc/sysctl.d/20-solana-mmaps.conf<code>추가</code>LimitNOFILE
```

##### **증가 메모리 파일제한 매핑**

```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-mmaps.conf <<EOF
# Increase memory mapped files limit
vm.max_map_count = 500000
EOF"
```

```bash
sudo sysctl -p /etc/sysctl.d/20-solana-mmaps.conf
```

LimitNOFILE

```
500000NOFILE
EOF
```

vm.max_map_count = 500000 EOF를

````
추가```DefaultLimitNOFILE
````

"``````bash는 sudo는 sysctl을 -p

```bash
와시작 서비스
:<code>bash는
$ sudo를 systemctl --now 사용하도록 설정
졸</code>
```

```bash
sudo bash -c "cat >/etc/security/limits.d/90-solana-nofiles.conf <<EOF
# Increase process file descriptor count limit
* - nofile 500000
EOF"
```

```bash
### Close all open sessions (log out then, in again) ###
```

## 투표 계정

Create an identity keypair for your validator by running:

```bash
참고 : "validator-keypair.json"파일은 \ (ed25519 \) 개인 키이기도합니다.
```

리로드``````bash는 sudo는 bash는 -c "고양이> /etc/security/limits.d/90-solana-nofiles.conf << EOF

```bash
solana-keygen pubkey ~/validator-keypair.json
```

> Note: The "validator-keypair.json” file is also your \(ed25519\) private key.

### 종이 지갑 ID

"``````bash는

```bash
solana-keygen new --no-outfile
```

리로드``````bash는 sudo는 bash는 -c "고양이> /etc/security/limits.d/90-solana-nofiles.conf << EOF

```bash
solana-keygen pubkey ASK
```

"``````bash는

정체성생성

---

### 베니 티 키 페어

solana-keygen을 사용하여 사용자 지정 베니 티 키 쌍을 생성 할 수 있습니다. 예를

```bash
solana-keygen grind --starts-with e1v1s:1
```

bash는 솔라-keygen은 새로운 -o ~ / 검증 - keypair.json```신원

---

당신의 검증 정체성 키 쌍은 고유 검사기를 식별 네트워크 내에서. **It is crucial to back-up this information.**

하지 않으면 액세스 권한을 잃어 버리면 유효성 검사기를 복구 할 수 없습니다. 이런 일이 발생하면 SOL 할당도 잃게됩니다.

`bash solana새로운 --no-OUTFILE을 -keygen`대응의

## 신뢰할 수있는 유효성 검사기

신원 공개 키는 현재 실행하여 볼 수

```bash
solana config set --keypair ~/validator-keypair.json
```

`bash solana새로운 --no-OUTFILE을 -keygen`대응의

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://devnet.solana.com
* keypair: /home/solana/validator-keypair.json
```

## Connect Your Validator 다음

신원 공개 키는 현재 실행하여 볼 수

```bash
solana airdrop 10
```

Note that airdrops are only available on Devnet and Testnet. Both are limited to 10 SOL per request.

To view your current balance:

```text
solana balance
```

미세한 자세히 볼 수 있습니다

```text
solana balance --lamports
```

들어`배쉬 솔라 나 - Keygen은이 갈기 --starts-와 e1v1s : 1`문자열이

## ## 로그 출력 조정

만약 아직 수행하지 않은 경우 투표 계정 키 쌍을 만들고 네트워크에서 투표 계정을 만듭니다. 이 단계를 완료 한 경우에, 당신은 당신의 솔라 런타임 디렉토리에 "투표-계정 keypair.json"을 참조한다

```bash
solana-keygen new -o ~/vote-account-keypair.json
```

들어`배쉬 솔라나 - Keygen은이 갈기 --starts-와 e1v1s : 1`문자열이

```bash
solana create-vote-account ~/vote-account-keypair.json ~/validator-keypair.json
```

Read more about [creating and managing a vote account](vote-accounts.md).

## Trusted validators

다른 유효성 검사기 노드를 알고 신뢰하는 경우 명령 줄에서`--trusted-validator <PUBKEY>`인수를 사용하여`solana-validator`에 지정할 수 있습니다. `--trusted-validator <PUBKEY1> --trusted-validator <PUBKEY2>`인수를 반복하여 여러 항목을 지정할 수 있습니다. 하나는 유효성 검사기가`--no-untrusted-rpc`로 부팅 할 때 생성 및 스냅 샷 데이터를 다운로드하기 위해 신뢰할 수있는 노드 집합 만 요청하는 것입니다. 또 다른 하나는`--halt-on-trusted-validator-hash-mismatch` 옵션과 함께 가십에있는 다른 신뢰할 수있는 노드의 전체 계정 상태에 대한 머클 루트 해시를 모니터링하고 해시가 불일치를 생성하는 경우, 유효성 검사기는 유효성 검사기가 투표하거나 잠재적으로 잘못된 상태 값을 처리하는 것을 방지하기 위해 노드를 중지합니다. 현재 유효성 검사기가 해시를 게시하는 슬롯은 스냅 샷 간격과 연결되어 있습니다. 기능이 유효하려면 신뢰할 수있는 집합의 모든 유효성 검사기가 동일한 스냅 샷 간격 값 또는 동일한 값의 배수로 설정되어야합니다.

악의적인 스냅샷 상태 다운로드 및 계정 상태 분산 예방을 위해 해당 옵션들을 활용하길 매우 권장합니다.

## Connect Your Validator

Connect to the cluster by running:

```bash
solana-validator \
  --identity ~/validator-keypair.json \
  --vote-account ~/vote-account-keypair.json \
  --ledger ~/validator-ledger \
  --rpc-port 8899 \
  --entrypoint devnet.solana.com:8001 \
  --limit-ledger-size \
  --log ~/solana-validator.log
```

It is highly recommended you use these options to prevent malicious snapshot state download or account state divergence.

> 참고 :`--identity` 및 / 또는`--authorized-voter` 키 쌍에 \[종이 지갑 시드 문구\] (../ wallet-guide / paper-wallet.md)를 사용할 수 있습니다. 이를 사용하려면 해당 인수를`solana-validator --identity ASK ... --authorized-voter ASK ...`로 전달하면 시드 구문과 선택적 암호를 입력하라는 메시지가 표시됩니다.

새 터미널을 열고 실행하여 네트워크에 연결하여 유효성을 확인 :

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

:`bash는 솔라 설정 세트 --keypair ~ / 검증 - keypair.json`당신은

### 로컬 네트워크 포트 할당 제어

기본적으로 유효성 검사기는 8000-10000 범위에서 사용 가능한 네트워크 포트를 동적으로 선택하며`--dynamic-port-range`로 재정의 할 수 있습니다. 예를 들어`solana-validator --dynamic-port-range 11000-11010 ...`은 유효성 검사기를 포트 11000-11010으로 제한합니다.

### 디스크 공간을 절약하기 위해 원장 크기 제한

`--limit-ledger-size` 매개 변수를 사용하면 노드가 디스크에 보유하는 원장 \[shreds\] (../ terminology.md # shred) 수를 지정할 수 있습니다. 이 매개 변수를 포함하지 않으면 유효성 검사기는 디스크 공간이 부족해질 때까지 전체 원장을 유지합니다.

기본값은 원장 디스크 사용량을 500GB 미만으로 유지하려고합니다. 원하는 경우`--limit-ledger-size`에 인수를 추가하여 디스크 사용량을 더 많이 또는 더 적게 요청할 수 있습니다. `--limit-ledger-size`에서 사용하는 기본 제한 값은`solana-validator --help`를 확인하세요. 맞춤 제한 값 선택에 대한 자세한 내용은 \[여기\] (https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26)를 참조하세요.

### Systemd Unit

현재 잔액을 보려면

:..`텍스트 솔라 균형을`또는

```
[Unit]
Description=Solana Validator
After=network.target
Wants=solana-sys-tuner.service
StartLimitIntervalSec=0

[Service]
Type=simple
Restart=always
RestartSec=1
User=sol
LimitNOFILE=500000
LogRateLimitIntervalSec=0
Environment="PATH=/bin:/usr/bin:/home/sol/.local/share/solana/install/active_release/bin"
ExecStart=/home/sol/bin/validator.sh

[Install]
WantedBy=multi-user.target
```

Now create `/home/sol/bin/validator.sh` to include the desired `solana-validator` command-line. / 홈 / 솔 / 빈 / validator.sh`원하는`solana-validator`명령 줄을 포함합니다.`/ home / sol / bin / validator.sh`를 실행하여 예상대로 유효성 검사기를 수동으로 시작하는지 확인합니다.chmod를 + X / 홈 / 솔 / 빈 / validator.sh`'와 그것을 실행 표시하는 것을 잊지 마세요 / 홈 / 솔 / 빈 / validator.sh`원하는`solana-validator`명령 줄을 포함합니다.`/ home / sol / bin / validator.sh`를 실행하여 예상대로 밸리데이터를 수동으로 시작하는지 확인합니다.chmod를 + X / 홈 / 솔 / 빈 / validator.sh`'와 그것을 실행 표시하는 것을 잊지 마세요 Don't forget to mark it executable with `chmod +x /home/sol/bin/validator.sh`

:..`텍스트 솔라 균형을`또는

````bash
```bash는
sudo는 systemctl 데몬
````

### 스냅 샷 압축을 비활성화하여 CPU 사용량 줄이기

#### 사용 logrotate

유효성 검사기가 로그에 내보내는 메시지는`RUST_LOG` 환경 변수로 제어 할 수 있습니다. 자세한 내용은`env_logger` Rust 상자에 대한 \[문서\] (https://docs.rs/env_logger/latest/env_logger/#enabling-logging)에서 찾을 수 있습니다.

로깅 출력이 감소하면 나중에 발생하는 문제를 디버깅하기 어려울 수 있습니다. 팀에서 지원을 받으려면 모든 변경 사항을 되돌리고 문제를 재현해야 도움을받을 수 있습니다.

#### 로그 회전

:`bash는 솔라-keygen은 새로운 -o ~ / 투표-계정 keypair.json`다음을

명령은 모든 기본 옵션으로 blockchain에 투표 계정을 생성 할 수 있습니다

#### Using logrotate

:`bash는 솔라-keygen은 새로운 -o ~ / 투표-계정 keypair.json`다음을

```bash
cat&#062; logrotate.sol &#060;&#060; EOF
/home/sol/solana-validator.log {
  rotate 7
  daily
  missingok
  postrotate
    systemctl kill -s USR1 sol.service
  endscript
}
EOF
sudo cp logrotate.sol / etc / logrotate.d / 졸
systemctl를 다시 시작
```

### SSD 마모를 줄이기 위해 계정 데이터베이스를 스왑으로 스왑으로 램 디스크를 사용

명령은 모든 기본 옵션으로 blockchain에 투표 계정을 생성 할 수 있습니다

### 계정 인덱싱

:`bash는생성-투표-계정 솔라 ~ / 투표-계정 keypair.json ~ / 검증 - keypair.json`읽기보다

약 \[투표 계정 생성 및 관리\] (vote-accounts.md).

### Using a ramdisk with spill-over into swap for the accounts database to reduce SSD wear

다른 밸리데이터에 스냅 샷을 제공하지 않는 경우 스냅 샷 압축을 비활성화하여 로컬 스냅 샷 저장소에 대한 디스크 사용량이 약간 더 늘어나는 대신 CPU로드를 줄일 수 있습니다.

악의적 인 스냅 샷 상태 다운로드 또는 계정 상태 차이를 방지하려면 이러한 옵션을 사용하는 것이 좋습니다.

을 실행하여 클러스터에 연결하십시오 :

`bash solana-validator \ --identity ~ / validator-keypair.json \ --vote-account ~ / vote-account-keypair.json \ --ledger ~ / 검증 원장 \ --rpc 포트 8899 \ --entrypoint devnet.solana.com:8001 \ --limit 원장 크기 \ --log ~ / 솔라나 - validator.log`받는

1. `sudo mkdir /mnt/solana-accounts`
2. 구성 예 : 1.`sudo mkdir / mnt / solana-accounts` 2.`tmpfs 포함하는 새 줄을 추가하여 300GB tmpfs 파티션을 추가 / mnt / solana-accounts tmpfs rw, size = 300G, user = sol 0 0`을합니다. `/ etc / fstab` (밸리데이터가 "sol"사용자로 실행되고 있다고 가정). **CAREFUL: If you incorrectly edit /etc/fstab your machine may no longer boot**
3. Create at least 250GB of swap space

- 최소 250GB의 스왑 공간을 만듭니다 .-이 지침의 나머지 부분에서`SWAPDEV` 대신 사용할 장치를 선택합니다. 빠른 디스크에서 250GB 이상의 여유 디스크 파티션을 선택하는 것이 가장 좋습니다. 사용할 수없는 경우`sudo dd if = / dev / zero of = / swapfile bs = 1MiB count = 250KiB`로 스왑 파일을 만들고`sudo chmod 0600 / swapfile`로 권한을 설정하고`/ swapfile`을 다음과 같이 사용합니다. 이 지침의 나머지 부분에 대해`SWAPDEV` -`sudo mkswap SWAPDEV`를 사용하여 스왑으로 사용할 장치를 포맷합니다.fstab`4.`SWAPDEV 스왑 스왑 기본값 0 0`을 포함하는 새 줄을 사용하여`/ etc /에 스왑 파일을 추가합니다.
- Format the device for usage as swap with `sudo mkswap SWAPDEV`

4. Add the swap file to `/etc/fstab` with a new line containing `SWAPDEV swap swap defaults 0 0`
5. 5 .`sudo swapon -a`로 스왑을 활성화하고`sudo mount / mnt / solana-accounts /`로 tmpfs를 마운트합니다.
6. 6.`free -g`로 스왑이 활성화되고`mount`로 tmpfs가 마운트되었는지 확인

250GB 스왑 파티션과 함께 300GB tmpfs 파티션을 권장합니다.

### Account indexing

클러스터에 채워진 계정 수가 증가함에 따라 [`getProgramAccounts`] (developing / clients / jsonrpc-api.md # getprogramaccounts) 및 \[ SPL-token-specific requests\] (developing / clients / jsonrpc-api.md # gettokenaccountsbydelegate)-성능이 좋지 않을 수 있습니다. 유효성 검사기가 이러한 요청을 지원해야하는 경우`--account-index` 매개 변수를 사용하여 키 필드로 계정을 색인화하여 RPC 성능을 크게 향상시키는 하나 이상의 메모리 내 계정 색인을 활성화 할 수 있습니다. 현재 다음 매개 변수 값을 지원합니다.

- URL : http://devnet.solana.com \*이키 쌍 :`/home/solana/validator-keypair.json
- `spl-token-mint`: each SPL token account indexed by its token Mint; used by [getTokenAccountsByDelegate](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate), and [getTokenLargestAccounts](developing/clients/jsonrpc-api.md#gettokenlargestaccounts)
- \[getTokenAccountsByDelegate\] (developing / clients / jsonrpc-api.md # gettokenaccountsbydelegate) 및 \[getTokenLargestAccounts\] (developing / clients / jsonrpc-api.md # gettokenlargestaccounts) 에서 사용-`spl-token-owner` : 색인이 생성 된 각 SPL 토큰 계정 토큰 소유자 주소로; spl-token-owner 필터를 포함하는 \[getTokenAccountsByOwner\] (developing / clients / jsonrpc-api.md # gettokenaccountsbyowner) 및 [`getProgramAccounts`] (developing / clients / jsonrpc-api.md # getprogramaccounts) 요청에서 사용됩니다.
