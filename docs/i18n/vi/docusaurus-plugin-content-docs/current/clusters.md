---
title: Cụm Solana
---

Solana duy trì một số cụm khác nhau với các mục đích khác nhau.

Trước khi bắt đầu, hãy đảm bảo rằng bạn [đã cài đặt các công cụ dòng lệnh Solana](cli/install-solana-cli-tools.md)

Explorers:

- [http://explorer.solana.com/](https://explorer.solana.com/).
- [http://solanabeach.io/](http://solanabeach.io/).

## Devnet

- Devnet phục vụ như một sân chơi cho bất kỳ ai muốn dùng Solana để thử nghiệm, với tư cách là người dùng, chủ sở hữu mã thông báo, nhà phát triển ứng dụng hoặc validator.
- Các nhà phát triển ứng dụng nên nhắm mục tiêu Devnet.
- Các validator tiềm năng nên nhắm mục tiêu Devnet đầu tiên.
- Sự khác biệt chính giữa Devnet và Mainnet Beta:
  - Mã thông báo Devnet **không phải thật**
  - Devnet bao gồm một vòi mã thông báo cho airdrop để thử nghiệm ứng dụng
  - Devnet có thể phải đặt lại sổ cái
  - Devnet thường sẽ chạy một phiên bản phần mềm mới hơn Mainnet Beta
- Điểm nhập gossip cho Devnet: `entrypoint.devnet.solana.com:8001`
- Các số liệu biến đổi môi trường cho Devnet:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=devnet,u=scratch_writer,p=topsecret"
```

- URL RPC cho Devnet: `https://devnet.solana.com`

##### Ví dụ về cấu hình dòng lệnh `solana`

```bash
solana config set --url https://devnet.solana.com
```

##### Ví dụ dòng lệnh `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator dv1LfzJvDF7S1fBKpFgKoKXK5yoSosmkAdfbxBo1GqJ \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.devnet.solana.com:8001 \
    --expected-genesis-hash EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

`--trusted-validator` được vận hành bởi Solana

## Testnet

- Testnet là nơi chúng tôi nhấn mạnh kiểm tra các tính năng phát hành gần đây trên một cụm trực tiếp, đặc biệt tập trung vào hiệu suất mạng, độ ổn định và hành vi của validator.
- Sáng kiến [Tour de SOL](tour-de-sol.md) chạy trên Testnet, nơi chúng tôi khuyến khích các cuộc tấn công trên mạng và các hành vi độc hại để giúp chúng tôi tìm và loại bỏ các lỗi hoặc lỗ hổng mạng.
- Mã thông báo Testnet **không phải thật**
- Testnet có thể phải đặt lại sổ cái.
- Testnet bao gồm một vòi mã thông báo cho airdrop để thử nghiệm ứng dụng
- Testnet thường sẽ chạy một bản phát hành phần mềm mới hơn cả Devnet và Mainnet Beta
- Điểm nhập gossip cho Testnet: `entrypoint.testnet.solana.com:8001`
- Các số liệu biến đổi môi trường cho Testnet:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=tds,u=testnet_write,p=c4fa841aa918bf8274e3e2a44d77568d9861b3ea"
```

- URL RPC cho Testnet: `https://testnet.solana.com`

##### Ví dụ về cấu hình dòng lệnh `solana`

```bash
solana config set --url https://testnet.solana.com
```

##### Ví dụ dòng lệnh `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on \
    --trusted-validator ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T \
    --trusted-validator Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN \
    --trusted-validator 9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.testnet.solana.com:8001 \
    --expected-genesis-hash 4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

Danh tính của các `--trusted-validator` là:

- `5D1fNXzvv5NjV1ysLjirC4WY92RNsVH18vjmcszZd8on` - testnet.solana.com (Solana)
- `ta1Uvfb7W5BRPrdGnhP9RmeCGKzBySGM1hTE4rBRy6T` - Break RPC node (Solana)
- `Ft5fbkqNa76vnsjYNwjDZUXoTWpP7VYm3mtsaQckQADN` - Certus One
- `9QxCLckBiJc783jnMvXZubK4wH86Eqqvashtrwvcsgkv` - Algo|Stake

## Mainnet Beta

Một cụm liên tục, không được phép dành cho chủ sở hữu mã thông báo sớm và các đối tác khởi chạy. Hiện tại, phần thưởng và lạm phát đã bị vô hiệu hóa.

- Các mã thông báo được phát hành trên Mainnet Beta là SOL strong x-id="1">thật</strong>
- Nếu bạn đã trả tiền để mua/được mã các thông báo phát hành, chẳng hạn như thông qua đấu giá CoinList của chúng tôi, các mã thông báo này sẽ được chuyển trên Mainnet Beta.
  - Lưu ý: Nếu bạn đang sử dụng ví không phải dòng lệnh như [Solflare](wallet-guide/solflare.md), ví sẽ luôn kết nối với Mainnet Beta.
- Điểm nhập gossip cho Mainnet Beta: `entrypoint.mainnet-beta.solana.com:8001`
- Các số liệu biến đổi môi trường cho Mainnet Beta:

```bash
export SOLANA_METRICS_CONFIG="host=https://metrics.solana.com:8086,db=mainnet-beta,u=mainnet-beta_write,p=password"
```

- URL RPC cho Mainnet Beta: `https://api.mainnet-beta.solana.com`

##### Ví dụ về cấu hình dòng lệnh `solana`

```bash
solana config set --url https://api.mainnet-beta.solana.com
```

##### Ví dụ dòng lệnh `solana-validator`

```bash
$ solana-validator \
    --identity ~/validator-keypair.json \
    --vote-account ~/vote-account-keypair.json \
    --trusted-validator 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 \
    --trusted-validator GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ \
    --trusted-validator DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ \
    --trusted-validator CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S \
    --no-untrusted-rpc \
    --ledger ~/validator-ledger \
    --rpc-port 8899 \
    --private-rpc \
    --dynamic-port-range 8000-8010 \
    --entrypoint entrypoint.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint2.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint3.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint4.mainnet-beta.solana.com:8001 \
    --entrypoint entrypoint5.mainnet-beta.solana.com:8001 \
    --expected-genesis-hash 5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d \
    --wal-recovery-mode skip_any_corrupted_record \
    --limit-ledger-size
```

Tất cả bốn `--trusted-validator` đều được vận hành bởi Solana
