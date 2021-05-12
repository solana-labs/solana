---
title: Bắt đầu Validator
---

## Định cấu hình Solana CLI

Các lệnh cli bao gồm `get` và `set` cấu hình solana để tự động đặt đối số `--url` cho các lệnh cli. Ví dụ:

```bash
solana config set --url http://devnet.solana.com
```

Phần này trình bày cách kết nối với cụm Devnet, các bước thực hiện tương tự đối với [Solana Clusters](../clusters.md) khác.

## Xác nhận Cụm có thể Truy cập

Trước khi đính kèm một node validator, hãy kiểm tra kỹ để đảm bảo rằng cụm này có thể truy cập được vào máy của bạn bằng cách tìm nạp số lượng giao dịch:

```bash
solana transaction-count
```

Xem [bảng điều khiển số liệu](https://metrics.solana.com:3000/d/monitor/cluster-telemetry) để biết thêm chi tiết về hoạt động của cụm.

## Xác nhận cài đặt của bạn

Hãy thử chạy lệnh sau để tham gia mạng gossip và xem tất cả các node khác trong cụm:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
# Press ^C to exit
```

## Bật CUDA

Nếu máy của bạn có GPU được cài đặt CUDA \(hiện chỉ dành cho Linux\), hãy bao gồm đối số `--cuda` thành `solana-validator`.

Khi validator của bạn được khởi động, hãy tìm thông báo nhật ký sau để cho biết rằng CUDA đã được bật: `"[<timestamp> solana::validator] CUDA is enabled"`

## Điều chỉnh hệ thống

### Linux

#### Tự động

Kho lưu trữ solana bao gồm một daemon để điều chỉnh cài đặt hệ thống nhằm tối ưu hóa hiệu suất (cụ thể là bằng cách tăng bộ đệm UDP hệ điều hành và giới hạn ánh xạ tệp).

Daemon (`solana-sys-tuner`) được bao gồm trong bản phát hành nhị phân solana. Khởi động lại nó, _trước khi_ khởi động lại validator của bạn, sau mỗi lần nâng cấp phần mềm để đảm bảo rằng các cài đặt được đề xuất mới nhất được áp dụng.

Để chạy nó:

```bash
sudo solana-sys-tuner --user $(whoami) > sys-tuner.log 2>&1 &
```

#### Thủ công

Nếu bạn muốn tự mình quản lý cài đặt hệ thống, bạn có thể làm như vậy với các lệnh sau.

##### **Tăng bộ đệm UDP**

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
sudo sysctl -p /etc/sysctl.d/20-solana-udp-buffers.conf
```

##### **Tăng giới hạn tệp ánh xạ bộ nhớ**

```bash
sudo bash -c "cat >/etc/sysctl.d/20-solana-mmaps.conf <<EOF
# Increase memory mapped files limit
vm.max_map_count = 500000
EOF"
```

```bash
sudo sysctl -p /etc/sysctl.d/20-solana-mmaps.conf
```

Thêm

```
LimitNOFILE=500000
```

vào phần `[Service]` của tệp dịch vụ systemd của bạn, nếu bạn sử dụng, nếu không, hãy thêm

```
DefaultLimitNOFILE=500000
```

vào phần `[Manager]` của `/etc/systemd/system.conf`.

```bash
sudo systemctl daemon-reload
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

## Tạo danh tính

Tạo keypair nhận dạng cho validator của bạn bằng cách chạy:

```bash
solana-keygen new -o ~/validator-keypair.json
```

Public key danh tính hiện có thể được xem bằng cách chạy:

```bash
solana-keygen pubkey ~/validator-keypair.json
```

> Lưu ý: Tệp "validator-keypair.json" cũng là private key \(ed25519\) của bạn.

### Danh tính Ví giấy

Bạn có thể tạo một ví giấy cho tệp danh tính của mình thay vì ghi tệp keypair vào ổ đĩa cứng bằng:

```bash
solana-keygen new --no-outfile
```

Public key danh tính tương ứng hiện có thể được xem bằng cách chạy:

```bash
solana-keygen pubkey ASK
```

và sau đó nhập cụm từ hạt giống của bạn.

Xem [Cách sử dụng Ví giấy](../wallet-guide/paper-wallet.md) để biết thêm thông tin.

---

### Vanity Keypair

Bạn có thể tạo một keypair tùy chỉnh bằng cách sử dụng solana-keygen. Ví dụ:

```bash
solana-keygen grind --starts-with e1v1s:1
```

Tùy thuộc vào chuỗi được yêu cầu, có thể mất nhiều ngày để tìm thấy một kết quả phù hợp...

---

Keypair nhận dạng validator của bạn xác định duy nhất validator của bạn trong mạng. **Điều quan trọng là phải sao lưu thông tin này.**

Nếu bạn không sao lưu thông tin này, bạn SẼ KHÔNG CÓ KHẢ NĂNG PHỤC HỒI VALIDATOR CỦA BẠN nếu bạn mất quyền truy cập vào nó. Nếu điều này xảy ra, BẠN SẼ MẤT SỰ CẤP QUYỀN CHO SOL.

Để sao lưu validator của bạn xác định keypair, **hãy sao lưu tệp "validator-keypair.json” hoặc cụm từ hạt giống của bạn vào một vị trí an toàn.**

## Thêm cấu hình Solana CLI

Bây giờ bạn đã có một keypair, hãy đặt cấu hình solana để sử dụng keypair validator của bạn cho tất cả các lệnh sau:

```bash
solana config set --keypair ~/validator-keypair.json
```

Bạn sẽ thấy kết quả sau:

```text
Wallet Config Updated: /home/solana/.config/solana/wallet/config.yml
* url: http://devnet.solana.com
* keypair: /home/solana/validator-keypair.json
```

## Airdrop & Kiểm tra số dư của Validator

Airdrop cho mình một số SOL để bắt đầu:

```bash
solana airdrop 10
```

Lưu ý rằng airdrop chỉ khả dụng trên Devnet và Testnet. Cả hai đều được giới hạn ở 10 SOL cho mỗi yêu cầu.

Để xem số dư hiện tại của bạn:

```text
solana balance
```

Hoặc xem chi tiết hơn:

```text
solana balance --lamports
```

Đọc thêm [sự khác biệt giữa SOL và các lamport ở đây.](../introduction.md#what-are-sols).

## Tạo Tài khoản Bỏ phiếu

Nếu bạn chưa làm như vậy, hãy tạo keypair tài khoản bỏ phiếu và tạo tài khoản bỏ phiếu trên mạng. Nếu bạn đã hoàn thành bước này, bạn sẽ thấy “vote-account-keypair.json” trong thư mục thời gian chạy Solana của mình:

```bash
solana-keygen new -o ~/vote-account-keypair.json
```

Lệnh sau có thể được sử dụng để tạo tài khoản bỏ phiếu của bạn trên blockchain với tất cả các tùy chọn mặc định:

```bash
solana create-vote-account ~/vote-account-keypair.json ~/validator-keypair.json
```

Đọc thêm về [cách tạo và quản lý tài khoản bỏ phiếu ](vote-accounts.md).

## Các validator đáng tin cậy

Nếu bạn biết và tin tưởng các node validator khác, bạn có thể chỉ định điều này trên dòng lệnh với code>--trusted-validator &lt;PUBKEY&gt;</code> đối số là `solana-validator`. Bạn có thể chỉ định nhiều cái bằng cách lặp lại đối số `--trusted-validator <PUBKEY1> --trusted-validator <PUBKEY2>`. Điều này có hai tác dụng, một là khi validator khởi động bằng `--no-untrusted-rpc`, nó sẽ chỉ yêu cầu tập hợp các node đáng tin cậy đó để tải xuống dữ liệu nguồn gốc và ảnh chụp nhanh. Khác là kết hợp với tùy chọn `--halt-on-trusted-validator-hash-mismatch`, nó sẽ theo dõi hàm băm gốc merkle của toàn bộ trạng thái tài khoản của các node đáng tin cậy khác trên gossip và nếu các hàm băm tạo ra bất kỳ sự không khớp nào, validator sẽ tạm dừng node để ngăn validator bỏ phiếu hoặc xử lý các giá trị trạng thái có khả năng không chính xác. Hiện tại, slot mà validator xuất bản hàm băm được gắn với khoảng thời gian chụp nhanh. Để tính năng hoạt động hiệu quả, tất cả các validator trong tập hợp tin cậy phải được đặt thành cùng một giá trị khoảng thời gian chụp nhanh hoặc bội số của cùng một giá trị.

Chúng tôi khuyên bạn nên sử dụng các tùy chọn này để ngăn chặn tải xuống trạng thái ảnh chụp nhanh độc hại hoặc phân kỳ trạng thái tài khoản.

## Kết nối validator của bạn

Kết nối với cụm bằng cách chạy:

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

Để buộc validator đăng nhập vào bảng điều khiển, hãy thêm đối số `--log -`, nếu không, validator sẽ tự động đăng nhập vào một tệp.

> Lưu ý: Bạn có thể sử dụng [cụm từ hạt giống ví giấy](../wallet-guide/paper-wallet.md) cho `--identity` và /hoặc keypair `--authorized-voter`. Để sử dụng chúng, hãy chuyển đối số tương ứng dưới dạng `solana-validator --identity ASK ... --authorized-voter ASK ...` và bạn sẽ được nhắc nhập các cụm từ hạt giống và cụm mật khẩu tùy chọn của bạn.

Xác nhận validator của bạn được kết nối với mạng bằng cách mở một thiết bị đầu cuối mới và chạy:

```bash
solana-gossip spy --entrypoint devnet.solana.com:8001
```

Nếu validator của bạn được kết nối, public key và địa chỉ IP của nó sẽ xuất hiện trong danh sách.

### Kiểm soát phân bổ cổng mạng cục bộ

Theo mặc định, validator sẽ tự động chọn các cổng mạng có sẵn trong phạm vi 8000-10000 và có thể bị ghi đè bằng `--dynamic-port-range`. Ví dụ, `solana-validator --dynamic-port-range 11000-11010 ...` sẽ hạn chế validator cho các cổng 11000-11010.

### Giới hạn kích thước sổ cái để tiết kiệm dung lượng ổ đĩa cứng

Tham số `--limit-ledger-size` cho phép bạn chỉ định bao nhiêu sổ cái [shreds](../terminology.md#shred) node của bạn được giữ lại trên ổ đĩa cứng. Nếu bạn không bao gồm tham số này, validator sẽ giữ toàn bộ sổ cái cho đến khi hết dung lượng ổ đĩa cứng.

Giá trị mặc định cố gắng duy trì mức sử dụng ổ đĩa cứng sổ cái dưới 500GB. Có thể yêu cầu sử dụng ổ đĩa cứng nhiều hơn hoặc ít hơn bằng cách thêm đối số vào `--limit-ledger-size` nếu muốn. Kiểm tra `solana-validator --help` giá trị giới hạn mặc định được sử dụng bởi `--limit-ledger-size`. Thông tin thêm về việc chọn giá trị giới hạn tùy chỉnh [có sẵn tại đây](https://github.com/solana-labs/solana/blob/583cec922b6107e0f85c7e14cb5e642bc7dfb340/core/src/ledger_cleanup_service.rs#L15-L26).

### Đơn vị Systemd

Chạy validator dưới dạng đơn vị systemd là một cách dễ dàng để quản lý việc chạy trong nền.

Giả sử bạn có người dùng có tên được gọi bằng `sol` trên máy của mình, hãy tạo tệp `/etc/systemd/system/sol.service` bằng cách sau:

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

Bây giờ hãy tạo `/home/sol/bin/validator.sh` để bao gồm dòng lệnh `solana-validator` mong muốn. Đảm bảo rằng việc chạy `/home/sol/bin/validator.sh` bằng cách thủ công sẽ khởi động validator như mong đợi. Đừng quên đánh dấu nó có thể thực thi bằng `chmod +x /home/sol/bin/validator.sh`

Bắt đầu dịch vụ với:

```bash
$ sudo systemctl enable --now sol
```

### Ghi nhật ký

#### Điều chỉnh đầu ra nhật ký

Các tin nhắn mà validator gửi tới nhật ký có thể được kiểm soát bởi `RUST_LOG` biến môi trường. Thông tin chi tiết có thể tìm thấy trong [tài liệu](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) về thùng Rust `env_logger`.

Lưu ý rằng nếu sản lượng ghi nhật ký bị giảm, điều này có thể gây khó khăn cho việc gỡ lỗi các vấn đề gặp phải sau này. Nếu nhóm tìm kiếm sự hỗ trợ, mọi thay đổi sẽ cần phải được hoàn nguyên và tái tạo sự cố trước khi có thể cung cấp trợ giúp.

#### Xoay vòng nhật ký

Tệp nhật ký validator, như được chỉ định bởi `--log ~/solana-validator.log`, có thể rất lớn theo thời gian và bạn nên định cấu hình xoay vòng nhật ký.

Trình xác thực sẽ mở lại khi nhận được tín hiệu `USR1`, đây là là nguyên thủy cơ bản cho phép xoay vòng nhật ký.

#### Sử dụng logrotate

Một ví dụ thiết lập cho `logrotate`, giả định rằng validator đang chạy dưới dạng dịch vụ systemd được gọi là `sol.service` và ghi tệp nhật ký tại /home/sol/solana-validator.log:

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

### Tắt kiểm tra cổng để tăng tốc độ khởi động lại

Khi validator của bạn đang hoạt động bình thường, bạn có thể giảm thời gian khởi động lại trình xác thực của bạn bằng cách thêm cờ `--no-port-check` vào dòng lệnh `solana-validator` của mình.

### Tắt tính năng nén ảnh chụp nhanh để giảm mức sử dụng CPU

Nếu bạn không cung cấp ảnh chụp nhanh cho các validator khác, thì tính năng nén ảnh chụp nhanh có thể bị vô hiệu hóa để giảm tải CPU với chi phí sử dụng ổ đĩa cứng nhiều hơn một chút cho lưu trữ ảnh chụp nhanh cục bộ.

Thêm đối số `--snapshot-compression none` vào các đối số `solana-validator` dòng lệnh của bạn và khởi động lại validator.

### Sử dụng ramdisk có spill-over trao đổi cho cơ sở dữ liệu tài khoản để giảm hao mòn SSD

Nếu máy của bạn có nhiều RAM, ramdisk tmpfs ([tmpfs](https://man7.org/linux/man-pages/man5/tmpfs.5.html)) có thể được sử dụng để chứa cơ sở dữ liệu tài khoản

Khi sử dụng tmpfs, bạn cũng cần định cấu hình hoán đổi trên máy của mình để tránh hết dung lượng tmpfs theo định kỳ.

Nên sử dụng phân vùng tmpfs 300GB, với phân vùng hoán đổi 250GB đi kèm.

Cấu hình ví dụ:

1. `sudo mkdir /mnt/solana-accounts`
2. Thêm parition 300GB tmpfs bằng cách thêm một dòng mới chứa `tmpfs /mnt/solana-accounts tmpfs rw,size=300G,user=sol 0 0` vào `/etc/fstab` (giả sử validator của bạn đang chạy dưới tư cách người dùng "sol"). **CẨN THẬN: Nếu bạn chỉnh sửa sai /etc/fstab, máy của bạn có thể không khởi động được nữa**
3. Tạo ít nhất 250GB không gian hoán đổi

- Chọn một thiết bị để sử dụng thay cho `SWAPDEV` cho phần còn lại của các hướng dẫn này. Lý tưởng nhất là chọn phân vùng ổ đĩa cứng trống có dung lượng 250GB trở lên trên ổ đĩa nhanh. Nếu không có sẵn, hãy tạo tệp hoán đổi với `sudo dd if=/dev/zero of=/swapfile bs=1MiB count=250KiB`, đặt quyền của nó bằng `sudo chmod 0600 /swapfile` và sử dụng `/swapfile` làm `SWAPDEV` cho phần còn lại của những hướng dẫn này
- Định dạng thiết bị để sử dụng dưới dạng trao đổi với `sudo mkswap SWAPDEV`

4. Thêm tệp hoán đổi vào `/etc/fstab` với một dòng mới chứa `SWAPDEV swap swap defaults 0 0`
5. Bật tính năng hoán đổi với `sudo swapon -a` và gắn kết các tmpfs với `sudo mount /mnt/solana-accounts/`
6. Xác nhận hoán đổi đang hoạt động với `free -g` và tmpfs được gắn với `mount`

Bây giờ, hãy thêm đối số `--accounts /mnt/solana-accounts` vào các đối số dòng lệnh `solana-validator` của bạn và khởi động lại validator.

### Lập chỉ mục tài khoản

Khi số lượng tài khoản phổ biến trên cụm tăng lên, RPC dữ liệu tài khoản yêu cầu quét toàn bộ tập tài khoản -- như [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) và [Các yêu cầu cụ thể về mã thông báo SPL](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate) -- có thể hoạt động kém. Nếu validator của bạn cần hỗ trợ bất kỳ yêu cầu nào trong số này, bạn có thể sử dụng tham số `--account-index` để kích hoạt một hoặc nhiều chỉ mục tài khoản trong bộ nhớ giúp cải thiện đáng kể hiệu suất RPC bằng cách lập chỉ mục tài khoản theo trường khóa. Hiện đang hỗ trợ các giá trị tham số sau:

- `program-id` mỗi tài khoản được lập chỉ mục bởi chương trình riêng của nó; được sử dụng bởi [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts)
- `spl-token-mint`: mỗi tài khoản mã thông báo SPL được lập chỉ mục bởi mã thông báo Mint của nó; được sử dụng bởi [getTokenAccountsByDelegate](developing/clients/jsonrpc-api.md#gettokenaccountsbydelegate), và [getTokenLargestAccounts](developing/clients/jsonrpc-api.md#gettokenlargestaccounts)
- `spl-token-owner`: mỗi tài khoản mã thông báo SPL được lập chỉ mục bởi địa chỉ chủ sở hữu mã thông báo; được sử dụng bởi [getTokenAccountsByOwner](developing/clients/jsonrpc-api.md#gettokenaccountsbyowner), và [`getProgramAccounts`](developing/clients/jsonrpc-api.md#getprogramaccounts) các yêu cầu bao gồm bộ lọc chủ sở hữu mã thông báo spl.
