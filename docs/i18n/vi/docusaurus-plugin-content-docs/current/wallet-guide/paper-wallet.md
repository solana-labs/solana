---
title: Ví Giấy
---

Tài liệu này mô tả cách tạo và sử dụng ví giấy với công cụ Solana CLI.

> Chúng tôi không có ý định tư vấn về cách tạo hoặc quản lý ví giấy _một cách an toàn_. Vui lòng nghiên cứu kỹ các mối quan tâm về bảo mật.

## Tổng quát

Solana cung cấp một công cụ tạo khóa để lấy khóa từ các cụm từ hạt giống tuân thủ BIP39. Các lệnh Solana CLI để chạy validator và staking mã thông báo đều hỗ trợ nhập keypair thông qua các cụm từ hạt giống.

Để tìm hiểu thêm về tiêu chuẩn BIP39, hãy truy cập kho lưu trữ Bitcoin BIPs Github [tại đây](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki).

## Sử dụng Ví giấy

Các lệnh Solana có thể được chạy mà không cần lưu keypair vào ổ đĩa cứng trên máy. Nếu việc tránh ghi private key vào ổ đĩa cứng là mối quan tâm bảo mật của bạn, thì bạn đã đến đúng nơi.

> Ngay cả khi sử dụng phương thức nhập an toàn này, vẫn có khả năng private key được ghi vào ổ đĩa cứng bằng các hoán đổi bộ nhớ không được mã hóa. Người dùng có trách nhiệm tự bảo vệ mình khỏi trường hợp này.

## Trước khi bắt đầu

- [Cài đặt các công cụ dòng lệnh Solana](../cli/install-solana-cli-tools.md)

### Kiểm tra cài đặt của bạn

Kiểm tra xem `solana-keygen` đã được cài đặt đúng chưa bằng cách chạy:

```bash
solana-keygen --version
```

## Tạo Ví giấy

Sử dụng công cụ `solana-keygen`, có thể tạo các cụm từ hạt giống mới cũng như lấy một keypair từ một cụm từ hạt giống hiện có và cụm mật khẩu (tùy chọn). Cụm từ hạt giống và cụm mật khẩu có thể được sử dụng cùng nhau như một ví giấy. Miễn là bạn lưu giữ cụm từ gốc và cụm mật khẩu của mình một cách an toàn, bạn có thể sử dụng chúng để truy cập tài khoản của mình.

> Để biết thêm thông tin về cách hoạt động của các cụm từ hạt giống, hãy xem lại [trang Bitcoin Wiki](https://en.bitcoin.it/wiki/Seed_phrase).

### Tạo Cụm từ hạt giống

Việc tạo keypair mới có thể được thực hiện bằng lệnh `solana-keygen new`. Lệnh sẽ tạo một cụm từ hạt giống ngẫu nhiên, yêu cầu bạn nhập một cụm mật khẩu tùy chọn, sau đó sẽ hiển thị public key dẫn xuất và cụm từ hạt giống đã tạo cho ví giấy của bạn.

Sau khi sao chép cụm từ hạt giống của mình, bạn có thể sử dụng hướng dẫn [dẫn xuất public key](#public-key-derivation) để xác minh rằng bạn không mắc phải bất kỳ lỗi nào.

```bash
solana-keygen new --no-outfile
```

> Nếu `--no-outfile` bị bỏ qua, hành vi mặc định là ghi keypair vào `~/.config/solana/id.json`, dẫn đến [ví hệ thống tệp](file-system-wallet.md)

Đầu ra của lệnh này sẽ hiển thị một dòng như sau:

```bash
pubkey: 9ZNTfG4NyQgxy2SWjSiQoUyBPEvXT2xo7fKc5hPYYJ7b
```

Giá trị được hiển thị sau văn bản `pubkey:` là _địa chỉ ví_ của bạn.

**Lưu Ý:** Khi làm việc với ví giấy và ví hệ thống tệp, các thuật ngữ "pubkey" và "địa chỉ ví" đôi khi được sử dụng để thay thế cho nhau.

> Để tăng cường bảo mật, hãy tăng số lượng từ của cụm từ gốc bằng cách sử dụng `--word-count`

Để biết chi tiết sử dụng đầy đủ, hãy chạy:

```bash
solana-keygen new --help
```

### Khởi tạo Public Key

Public key có thể bắt nguồn từ một cụm từ gốc và một cụm mật khẩu nếu bạn chọn sử dụng. This is useful for using an offline-generated seed phrase to derive a valid public key. The `solana-keygen pubkey` command will walk you through how to use your seed phrase (and a passphrase if you chose to use one) as a signer with the solana command-line tools using the `ask` uri scheme.

```bash
solana-keygen pubkey prompt://
```

> Lưu ý rằng bạn có thể sử dụng các cụm mật khẩu khác nhau cho cùng một cụm từ hạt giống. Mỗi cụm mật khẩu sẽ mang lại một keypair khác nhau.

Công cụ `solana-keygen` sử dụng cùng một danh sách từ tiếng Anh chuẩn BIP39 để tạo ra các cụm từ hạt giống. Nếu cụm từ hạt giống của bạn được tạo bằng công cụ sử dụng từ danh sách từ khác, bạn vẫn có thể sử dụng `solana-keygen`, nhưng sẽ cần chuyển đối số `--skip-seed-phrase-validation` và bỏ qua xác thực này.

```bash
solana-keygen pubkey prompt:// --skip-seed-phrase-validation
```

After entering your seed phrase with `solana-keygen pubkey prompt://` the console will display a string of base-58 character. This is the base _wallet address_ associated with your seed phrase.

> Sao chép địa chỉ khởi tạo vào USB để sử dụng dễ dàng trên các máy tính nối mạng

> Bước tiếp theo là [kiểm tra số dư](#checking-account-balance) của tài khoản được liên kết với public key

Để biết thêm chi tiết sử dụng đầy đủ, hãy chạy:

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

## Xác minh Keypair

Để chứng minh rằng bạn kiểm soát private key của địa chỉ ví giấy, hãy sử dụng `solana-keygen verify`:

```bash
solana-keygen verify <PUBKEY> prompt://
```

where `<PUBKEY>` is replaced with the wallet address and the keyword `prompt://` tells the command to prompt you for the keypair's seed phrase; `key` and `full-path` query-strings accepted. Note that for security reasons, your seed phrase will not be displayed as you type. After entering your seed phrase, the command will output "Success" if the given public key matches the keypair generated from your seed phrase, and "Failed" otherwise.

## Kiểm tra số dư Tài khoản

Tất cả những gì cần thiết để kiểm tra số dư tài khoản là public key của tài khoản. Để lấy public key một cách an toàn từ ví giấy, hãy làm theo [Khởi tạo Public Key](#public-key-derivation) và hướng dẫn [air gapped computer](<https://en.wikipedia.org/wiki/Air_gap_(networking)>). Sau đó, public key có thể được nhập bằng tay hoặc kết nối USB vào máy được nối mạng.

Tiếp theo, Chạy công cụ `solana` CLI để [kết nối với một cụm cụ thể ](../cli/choose-a-cluster.md):

```bash
solana config set --url <CLUSTER URL> # (i.e. https://api.mainnet-beta.solana.com)
```

Cuối cùng, để kiểm tra số dư, hãy chạy lệnh sau:

```bash
solana balance <PUBKEY>
```

## Tạo nhiều Địa chỉ Ví giấy

Bạn có thể tạo bao nhiêu địa chỉ ví nếu thích. Chỉ cần chạy lại các bước trong [Tạo cụm từ hạt giống](#seed-phrase-generation) hoặc [Tạo Public Key](#public-key-derivation) để tạo địa chỉ mới. Nhiều địa chỉ ví khác nhau có thể hữu ích nếu như bạn muốn chuyển mã thông báo giữa các tài khoản của riêng bạn cho các mục đích khác nhau.

## Hỗ trợ

Hãy xem [Trang Hỗ Trợ Ví](support.md) của chúng tôi để biết các cách nhận trợ giúp.
