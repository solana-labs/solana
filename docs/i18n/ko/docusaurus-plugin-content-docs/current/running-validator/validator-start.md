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
solana-gossip spy --entrypoint entrypoint.devnet.solana.com:8001
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
vm.max_map_count = 700000
EOF"
```

```bash
sudo sysctl -p /etc/sysctl.d/20-solana-mmaps.conf
```

LimitNOFILE

```
LimitNOFILE=700000
```

vm.max_map_count = 500000 EOF를

```
DefaultLimitNOFILE=700000
```

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
* - nofile 700000
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

You may request that the generated vanity keypair be expressed as a seed phrase which allows recovery of the keypair from the seed phrase and an optionally supplied passphrase (note that this is significantly slower than grinding without a mnemonic):

```bash
solana-keygen grind --use-mnemonic --starts-with e1v1s:1
```

Depending on the string requested, it may take days to find a match...

---

Your validator identity keypair uniquely identifies your validator within the network. **It is crucial to back-up this information.**

If you don’t back up this information, you WILL NOT BE ABLE TO RECOVER YOUR VALIDATOR if you lose access to it. If this happens, YOU WILL LOSE YOUR ALLOCATION OF SOL TOO.

To back-up your validator identify keypair, **back-up your "validator-keypair.json” file or your seed phrase to a secure location.**

## 신뢰할 수있는 유효성 검사기

Now that you have a keypair, set the solana configuration to use your validator keypair for all following commands:

```bash
solana config set --keypair ~/validator-keypair.json
```

You should see the following output:

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://devnet.solana.com
* keypair: /home/solana/validator-keypair.json
```

## Connect Your Validator 다음

Airdrop yourself some SOL to get started:

```bash
solana airdrop 1
```

Note that airdrops are only available on Devnet and Testnet. Both are limited to 1 SOL per request.

To view your current balance:

```text
solana balance
```

Or to see in finer detail:

```text
solana balance --lamports
```

Read more about the [difference between SOL and lamports here](../introduction.md#what-are-sols).

## ## 로그 출력 조정

If you haven’t already done so, create a vote-account keypair and create the vote account on the network. If you have completed this step, you should see the “vote-account-keypair.json” in your Solana runtime directory:

```bash
solana-keygen new -o ~/vote-account-keypair.json
```

The following command can be used to create your vote account on the blockchain with all the default options:

```bash
solana create-vote-account ~/vote-account-keypair.json ~/validator-keypair.json
```

Read more about [creating and managing a vote account](vote-accounts.md).

## Trusted validators

If you know and trust other validator nodes, you can specify this on the command line with the `--trusted-validator <PUBKEY>` argument to `solana-validator`. You can specify multiple ones by repeating the argument `--trusted-validator <PUBKEY1> --trusted-validator <PUBKEY2>`. This has two effects, one is when the validator is booting with `--no-untrusted-rpc`, it will only ask that set of trusted nodes for downloading genesis and snapshot data. Another is that in combination with the `--halt-on-trusted-validator-hash-mismatch` option, it will monitor the merkle root hash of the entire accounts state of other trusted nodes on gossip and if the hashes produce any mismatch, the validator will halt the node to prevent the validator from voting or processing potentially incorrect state values. At the moment, the slot that the validator publishes the hash on is tied to the snapshot interval. For the feature to be effective, all validators in the trusted set should be set to the same snapshot interval value or multiples of the same.

It is highly recommended you use these options to prevent malicious snapshot state download or account state divergence.

## Connect Your Validator

Connect to the cluster by running:

```bash
solana-validator \
  --identity ~/validator-keypair.json \
  --vote-account ~/vote-account-keypair.json \
  --rpc-port 8899 \
  --entrypoint entrypoint.devnet.solana.com:8001 \
  --limit-ledger-size \
  --log ~/solana-validator.log
```

To force validator logging to the console add a `--log -` argument, otherwise the validator will automatically log to a file.

The ledger will be placed in the `ledger/` directory by default, use the `--ledger` argument to specify a different location.

> 참고 :`--identity` 및 / 또는`--authorized-voter` 키 쌍에 \[종이 지갑 시드 문구\] (../ wallet-guide / paper-wallet.md)를 사용할 수 있습니다. To use these, pass the respective argument as `solana-validator --identity ASK ... --authorized-voter ASK ...` and you will be prompted to enter your seed phrases and optional passphrase.

Confirm your validator connected to the network by opening a new terminal and running:

```bash
solana-gossip spy --entrypoint entrypoint.devnet.solana.com:8001
```

If your validator is connected, its public key and IP address will appear in the list.

### 로컬 네트워크 포트 할당 제어

By default the validator will dynamically select available network ports in the 8000-10000 range, and may be overridden with `--dynamic-port-range`. For example, `solana-validator --dynamic-port-range 11000-11010 ...` will restrict the validator to ports 11000-11010.

### 디스크 공간을 절약하기 위해 원장 크기 제한

The `--limit-ledger-size` parameter allows you to specify how many ledger [shreds](../terminology.md#shred) your node retains on disk. If you do not include this parameter, the validator will keep the entire ledger until it runs out of disk space.

The default value attempts to keep the ledger disk usage under 500GB. More or less disk usage may be requested by adding an argument to `--limit-ledger-size` if desired. Check `solana-validator --help` for the default limit value used by `--limit-ledger-size`. More information about selecting a custom limit value is [available here](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

### Systemd Unit

Running the validator as a systemd unit is one easy way to manage running in the background.

Assuming you have a user called `sol` on your machine, create the file `/etc/systemd/system/sol.service` with the following:

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
LimitNOFILE=700000
LogRateLimitIntervalSec=0
Environment="PATH=/bin:/usr/bin:/home/sol/.local/share/solana/install/active_release/bin"
ExecStart=/home/sol/bin/validator.sh

[Install]
WantedBy=multi-user.target
```

Now create `/home/sol/bin/validator.sh` to include the desired `solana-validator` command-line. Ensure that running `/home/sol/bin/validator.sh` manually starts the validator as expected. Don't forget to mark it executable with `chmod +x /home/sol/bin/validator.sh`

Start the service with:

```bash
$ sudo systemctl enable --now sol
```

### 스냅 샷 압축을 비활성화하여 CPU 사용량 줄이기

#### 사용 logrotate

The messages that a validator emits to the log can be controlled by the `RUST_LOG` environment variable. Details can by found in the [documentation](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) for the `env_logger` Rust crate.

Note that if logging output is reduced, this may make it difficult to debug issues encountered later. Should support be sought from the team, any changes will need to be reverted and the issue reproduced before help can be provided.

#### 로그 회전

The validator log file, as specified by `--log ~/solana-validator.log`, can get very large over time and it's recommended that log rotation be configured.

The validator will re-open its when it receives the `USR1` signal, which is the basic primitive that enables log rotation.

If the validator is being started by a wrapper shell script, it is important to launch the process with `exec` (`exec solana-validator ...`) when using logrotate. This will prevent the `USR1` signal from being sent to the script's process instead of the validator's, which will kill them both.

#### Using logrotate

An example setup for the `logrotate`, which assumes that the validator is running as a systemd service called `sol.service` and writes a log file at /home/sol/solana-validator.log:

```bash
# Setup log rotation

cat > logrotate.sol <<EOF
/home/sol/solana-validator.log {
  rotate 7
  daily
  missingok
  postrotate
    systemctl kill -s USR1 sol.service
  endscript
}
EOF
sudo cp logrotate.sol /etc/logrotate.d/sol
systemctl restart logrotate.service
```

### SSD 마모를 줄이기 위해 계정 데이터베이스를 스왑으로 스왑으로 램 디스크를 사용

Once your validator is operating normally, you can reduce the time it takes to restart your validator by adding the `--no-port-check` flag to your `solana-validator` command-line.

### 계정 인덱싱

If you are not serving snapshots to other validators, snapshot compression can be disabled to reduce CPU load at the expense of slightly more disk usage for local snapshot storage.

Add the `--snapshot-compression none` argument to your `solana-validator` command-line arguments and restart the validator.

### Using a ramdisk with spill-over into swap for the accounts database to reduce SSD wear

If your machine has plenty of RAM, a tmpfs ramdisk ([tmpfs](https://man7.org/linux/man-pages/man5/tmpfs.5.html)) may be used to hold the accounts database

When using tmpfs it's essential to also configure swap on your machine as well to avoid running out of tmpfs space periodically.

A 300GB tmpfs partition is recommended, with an accompanying 250GB swap partition.

Example configuration:

1. `sudo mkdir /mnt/solana-accounts`
2. Add a 300GB tmpfs parition by adding a new line containing `tmpfs /mnt/solana-accounts tmpfs rw,size=300G,user=sol 0 0` to `/etc/fstab` (assuming your validator is running under the user "sol"). **CAREFUL: If you incorrectly edit /etc/fstab your machine may no longer boot**
3. Create at least 250GB of swap space

- Choose a device to use in place of `SWAPDEV` for the remainder of these instructions. Ideally select a free disk partition of 250GB or greater on a fast disk. If one is not available, create a swap file with `sudo dd if=/dev/zero of=/swapfile bs=1MiB count=250KiB`, set its permissions with `sudo chmod 0600 /swapfile` and use `/swapfile` as `SWAPDEV` for the remainder of these instructions
- Format the device for usage as swap with `sudo mkswap SWAPDEV`

4. Add the swap file to `/etc/fstab` with a new line containing `SWAPDEV swap swap defaults 0 0`
5. Enable swap with `sudo swapon -a` and mount the tmpfs with `sudo mount /mnt/solana-accounts/`
6. Confirm swap is active with `free -g` and the tmpfs is mounted with `mount`

Now add the `--accounts /mnt/solana-accounts` argument to your `solana-validator` command-line arguments and restart the validator.

### Account indexing

As the number of populated accounts on the cluster grows, account-data RPC requests that scan the entire account set -- like [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) and [SPL-token-specific requests](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate) -- may perform poorly. If your validator needs to support any of these requests, you can use the `--account-index` parameter to activate one or more in-memory account indexes that significantly improve RPC performance by indexing accounts by the key field. Currently supports the following parameter values:

- `program-id`: each account indexed by its owning program; used by [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts)
- `spl-token-mint`: each SPL token account indexed by its token Mint; used by [getTokenAccountsByDelegate](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate), and [getTokenLargestAccounts](developing/clients/jsonrpc-api.md#gettokenlargestaccounts)
- `spl-token-owner`: each SPL token account indexed by the token-owner address; used by [getTokenAccountsByOwner](developing/clients/jsonrpc-api.md#gettokenaccountsbyowner), and [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) requests that include an spl-token-owner filter.
