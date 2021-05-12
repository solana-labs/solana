---
title: Sử dụng Solana CLI
---

Trước khi chạy bất kỳ lệnh Solana CLI nào, hãy xem qua một số quy ước mà bạn sẽ thấy trên tất cả các lệnh. Đầu tiên, Solana CLI thực sự là một tập hợp các lệnh khác nhau cho mỗi hành động bạn muốn thực hiện. Bạn có thể xem danh sách tất cả các lệnh có thể bằng cách chạy:

```bash
solana --help
```

Để phóng to cách sử dụng một lệnh cụ thể, hãy chạy:

```bash
solana <COMMAND> --help
```

nơi bạn thay thế `<COMMAND>` bằng tên của lệnh bạn muốn tìm hiểu thêm.

Thông báo sử dụng của lệnh thường sẽ bao gồm các từ như `<AMOUNT>`, `<ACCOUNT_ADDRESS>` hoặc `<KEYPAIR>`. Mỗi từ là một trình giữ chỗ cho _type_ văn bản mà bạn có thể thực hiện lệnh. Ví dụ: bạn có thể thay thế `<AMOUNT>` bằng một số như `42` or `100.42`. Bạn có thể thay thế `<ACCOUNT_ADDRESS>` bằng mã hóa base58 của public key, chẳng hạn như `9grmKMwTiZwUHSExjtbFzHLPTdWoXgcg1bZkhvwTrTww`.

## Quy ước về Keypair

Nhiều lệnh sử dụng công cụ CLI yêu cầu giá trị cho `<KEYPAIR>`. Giá trị bạn nên sử dụng cho keypair phụ thuộc vào loại [ví dòng lệnh bạn đã tạo](../wallet-guide/cli.md).

Ví dụ: cách hiển thị địa chỉ của ví bất kỳ (còn được gọi là pubkey của keypair), tài liệu trợ giúp CLI cho thấy:

```bash
solana-keygen pubkey <KEYPAIR>
```

Dưới đây, chúng tôi chỉ ra cách giải quyết những gì bạn nên đưa vào `<KEYPAIR>` tùy thuộc vào loại ví của bạn.

#### Ví giấy

Trong ví giấy, keypair có nguồn gốc an toàn từ words và cụm mật khẩu tùy chọn mà bạn đã nhập khi tạo ví. Để sử dụng keypair ví giấy ở bất kỳ đâu mà `<KEYPAIR>` được hiển thị trong các ví dụ hoặc các tài liệu trợ giúp, hãy nhập `ASK` và chương trình sẽ nhắc bạn nhập words khi bạn chạy lệnh.

Để hiển thị địa chỉ ví của Ví giấy:

```bash
solana-keygen pubkey ASK
```

#### Ví Hệ thống tệp

Với ví hệ thống tệp, keypair được lưu trữ trong một file trên máy tính của bạn. Thay thế `<KEYPAIR>` bằng đường dẫn đến file keypair.

Ví dụ: nếu vị trí tệp keypair hệ thống tệp là `/home/solana/my_wallet.json`, để hiển thị địa chỉ, hãy thực hiện:

```bash
solana-keygen pubkey /home/solana/my_wallet.json
```

#### Ví phần cứng

Nếu bạn chọn một ví phần cứng, hãy sử dụng [URL keypair](../wallet-guide/hardware-wallets.md#specify-a-hardware-wallet-key), chẳng hạn như `usb://ledger?key=0`.

```bash
solana-keygen pubkey usb://ledger?key=0
```
