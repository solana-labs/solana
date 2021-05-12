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

Public key có thể bắt nguồn từ một cụm từ gốc và một cụm mật khẩu nếu bạn chọn sử dụng. Điều này hữu ích khi sử dụng cụm từ hạt giống được tạo ngoại tuyến để lấy public key hợp lệ. Lệnh `solana-keygen pubkey`sẽ hướng dẫn bạn thông qua việc nhập cụm từ hạt giống và cụm mật khẩu nếu bạn chọn sử dụng nó.

```bash
solana-keygen pubkey ASK
```

> Lưu ý rằng bạn có thể sử dụng các cụm mật khẩu khác nhau cho cùng một cụm từ hạt giống. Mỗi cụm mật khẩu sẽ mang lại một keypair khác nhau.

Công cụ `solana-keygen` sử dụng cùng một danh sách từ tiếng Anh chuẩn BIP39 để tạo ra các cụm từ hạt giống. Nếu cụm từ hạt giống của bạn được tạo bằng công cụ sử dụng từ danh sách từ khác, bạn vẫn có thể sử dụng `solana-keygen`, nhưng sẽ cần chuyển đối số `--skip-seed-phrase-validation` và bỏ qua xác thực này.

```bash
solana-keygen pubkey ASK --skip-seed-phrase-validation
```

Sau khi nhập cụm từ hạt giống của bạn bằng `solana-keygen pubkey ASK` bảng điều khiển sẽ hiển thị một chuỗi ký tự base-58. Đây là _địa chỉ ví_ được liên kết với cụm từ hạt giống của bạn.

> Sao chép địa chỉ khởi tạo vào USB để sử dụng dễ dàng trên các máy tính nối mạng

> Bước tiếp theo là [kiểm tra số dư](#checking-account-balance) của tài khoản được liên kết với public key

Để biết thêm chi tiết sử dụng đầy đủ, hãy chạy:

```bash
solana-keygen pubkey --help
```

## Xác minh Keypair

Để chứng minh rằng bạn kiểm soát private key của địa chỉ ví giấy, hãy sử dụng `solana-keygen verify`:

```bash
solana-keygen verify <PUBKEY> ASK
```

trong đó `<PUBKEY>` được thay thế bằng địa chỉ ví và từ khóa `ASK` cho lệnh nhắc bạn về cụm từ hạt giống của keypair. Lưu ý rằng vì lý do bảo mật, cụm từ hạt giống của bạn sẽ không được hiển thị khi bạn nhập. Sau khi nhập cụm từ hạt giống của bạn, lệnh sẽ xuất ra "Success" nếu public key đã cho khớp với keypair được tạo từ cụm từ hạt giống của bạn và "Failed" khi nó không khớp.

## Kiểm tra số dư Tài khoản

Tất cả những gì cần thiết để kiểm tra số dư tài khoản là public key của tài khoản. Để lấy public key một cách an toàn từ ví giấy, hãy làm theo [Khởi tạo Public Key](#public-key-derivation) và hướng dẫn [air gapped computer](https://en.wikipedia.org/wiki/Air_gap_(networking)). Sau đó, public key có thể được nhập bằng tay hoặc kết nối USB vào máy được nối mạng.

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
