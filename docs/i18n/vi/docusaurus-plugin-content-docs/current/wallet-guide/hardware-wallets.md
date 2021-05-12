---
title: Sử dụng Ví phần cứng trên Solana CLI
---

Việc ký một giao dịch yêu cầu private key, nhưng lưu trữ private key trên máy tính cá nhân hoặc điện thoại của bạn khiến chúng có thể bị đánh cắp. Thêm mật khẩu vào khóa của bạn sẽ tăng thêm tính bảo mật, nhưng nhiều người thích để tiến thêm một bước nữa và chuyển các private key của họ sang một thiết bị vật lý riêng biệt được gọi là _ví phần cứng_. Ví phần cứng là một thiết bị cầm tay nhỏ lưu trữ private key và cung cấp một số giao diện để ký kết các giao dịch.

Solana CLI hỗ trợ hàng đầu cho ví phần cứng. Bất kì nơi nào bạn sử dụng đường dẫn tệp keypair (được ký hiệu là `<KEYPAIR>` trong tài liệu sử dụng),bạn có thể chuyển một _URL keypair_ xác định duy nhất một keypair trong ví phần cứng.

## Ví phần cứng được hỗ trợ

Solana CLI hỗ trợ các ví phần cứng sau:

- [Ledger Nano S và Ledger Nano X](hardware-wallets/ledger.md)

## Chỉ định URL Keypair

Solana xác định định dạng URL keypair để định vị duy nhất bất kỳ keypair Solana nào trên ví phần cứng được kết nối với máy tính của bạn.

URL keypair có dạng sau, trong đó dấu ngoặc vuông hiển thị các trường tùy chọn:

```text
usb://<MANUFACTURER>[/<WALLET_ID>][?key=<DERIVATION_PATH>]
```

`WALLET_ID` là khóa duy nhất trên toàn cầu được sử dụng để phân định nhiều thiết bị.

`DERVIATION_PATH` được sử dụng để điều hướng đến các khóa Solana trong ví phần cứng của bạn. Đường dẫn có dạng `<ACCOUNT>[/<CHANGE>]`, trong đó mỗi `ACCOUNT` và `CHANGE` là các số nguyên dương.

Ví dụ: URL đủ điều kiện cho thiết bị Ledger có thể là:

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

Tất cả các đường dẫn xuất đều bao gồm tiền tố `44 '/ 501'`, cho biết đường dẫn tuân theo [thông số kỹ thuật BIP44](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki) và bất kỳ khóa dẫn xuất nào đều là khóa Solana (Coin type 501). Dấu ngoặc kép chỉ ra một nguồn gốc "hardened". Bởi vì Solana sử dụng cặp khóa Ed25519 tất cả các dẫn xuất đều được làm cứng và do đó việc thêm trích dẫn là tùy chọn và không cần thiết.
