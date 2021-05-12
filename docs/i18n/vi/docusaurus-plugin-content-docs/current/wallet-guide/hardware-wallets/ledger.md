---
title: Ledger Nano
---

Trang này mô tả cách sử dụng Ledger Nano S hoặc Nano X để tương tác với Solana bằng cách sử dụng các công cụ dòng lệnh.  Để xem các giải pháp khác để tương tác Solana với Nano của bạn, [hãy nhấn vào đây](../ledger-live.md#interact-with-the-solana-network).

## Trước khi bắt đầu

- [Thiết lập Nano với Ứng dụng Solana](../ledger-live.md)
- [Cài đặt các công cụ dòng lệnh Solana](../../cli/install-solana-cli-tools.md)

## Sử dụng Ledger Nano với Solana CLI

1. Đảm bảo ứng dụng Ledger Live đã đóng
2. Cắm Nano của bạn vào cổng USB của máy tính
3. Nhập mã pin của bạn và khởi động ứng dụng Solana trên Nano
4. Đảm bảo màn hình ghi "Application is ready"

### Xem ID Wallet của bạn

Trên máy tính của bạn, hãy chạy:

```bash
solana-keygen pubkey usb://ledger
```

Điều này xác nhận thiết bị Ledger của bạn được kết nối đúng cách và ở trạng thái sẵn sàng để tương tác với Solana CLI. Lệnh trả về _ID ví_ duy nhất của Ledger của bạn. Khi bạn có nhiều thiết bị Nano được kết nối với cùng một máy tính, bạn có thể sử dụng ID ví của mình để chỉ định ví phần cứng Ledger nào bạn muốn sử dụng. Nếu bạn chỉ sử dụng một Nano duy nhất trên máy tính của mình tại một thời điểm, bạn không cần ID ví. Để biết thêm thông tin về sử dụng ID ví để sử dụng sổ cái cụ thể, hãy đọc [Quản Lý Nhiều Ví Phần Cứng](#manage-multiple-hardware-wallets).

### Xem địa chỉ Ví của bạn

Nano của bạn hỗ trợ số lượng địa chỉ ví và người ký hợp lệ tùy ý. Để xem một địa chỉ bất kì, hãy sử dụng lệnh ` solana-keygen pubkey `, như được hiển thị bên dưới, theo sau là [URL keypair](../hardware-wallets.md#specify-a-keypair-url) hợp lệ.

Nhiều địa chỉ ví có thể hữu ích nếu bạn muốn chuyển các mã thông báo giữa tài khoản của riêng bạn cho các mục đích khác nhau hoặc sử dụng các keypair khác nhau trên thiết bị làm cơ quan ký cho tài khoản stake chẳng hạn.

Tất cả các lệnh sau sẽ hiển thị các địa chỉ khác nhau, được liên kết với đường dẫn keypair đã cho. Thử đi!

```bash
solana-keygen pubkey usb://ledger
solana-keygen pubkey usb://ledger?key=0
solana-keygen pubkey usb://ledger?key=1
solana-keygen pubkey usb://ledger?key=2
```

* LƯU Ý: thông số url keypair bị bỏ qua trong **zsh** &nbsp;[hãy xem cách khắc phục sự cố để biết thêm thông tin](#troubleshooting)

Bạn cũng có thể sử dụng các giá trị khác cho số sau `key=`. Bất kỳ địa chỉ nào được hiển thị bởi các lệnh này đều là địa chỉ ví Solana hợp lệ. Phần riêng tư được liên kết với mỗi địa chỉ được lưu trữ an toàn trên Nano và được sử dụng để ký các giao dịch từ địa chỉ này. Chỉ cần ghi chú URL keypair nào bạn đã sử dụng để lấy bất kỳ địa chỉ nào bạn sử dụng để nhận mã thông báo.

Nếu bạn chỉ định sử dụng một địa chỉ/keypair duy nhất trên thiết bị của mình, một đường dẫn tốt, dễ nhớ có thể sử dụng địa chỉ tại `key=0`. Xem địa chỉ này với:

```bash
solana-keygen pubkey usb://ledger?key=0
```

Bây giờ bạn có một địa chỉ ví (hoặc nhiều địa chỉ), bạn có thể chia sẻ bất kỳ những địa chỉ này công khai như một địa chỉ nhận và bạn có thể sử dụng URL keypair được liên kết làm người ký cho các giao dịch từ địa chỉ đó.

### Xem số dư của bạn

Để xem số dư của bất kỳ tài khoản nào, bất kể tài khoản đó sử dụng ví nào, hãy sử dụng lệnh `solana balance`:

```bash
solana balance SOME_WALLET_ADDRESS
```

Ví dụ: nếu địa chỉ của bạn là `7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`, hãy nhập lệnh sau để xem số dư:

```bash
solana balance 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri
```

Bạn cũng có thể xem số dư của bất kỳ địa chỉ tài khoản nào trên tab Tài khoản trong [Trình khám phá](https://explorer.solana.com/accounts) và dán địa chỉ vào ô để xem số dư trong trình duyệt web của bạn.

Lưu ý: Bất kỳ địa chỉ nào có số dư bằng 0 SOL, chẳng hạn như địa chỉ mới được tạo trên Ledger, sẽ hiển thị là "Not Found" trong trình khám phá. Tài khoản trống và tài khoản không tồn tại được xử lý như nhau trong Solana. Điều này sẽ thay đổi khi địa chỉ tài khoản của bạn có một số SOL trong đó.

### Gửi SOL từ Nano

Để gửi một số mã thông báo từ một địa chỉ từ Nano của bạn, bạn sẽ cần sử dụng thiết bị để ký một giao dịch, sử dụng cùng một URL keypair mà bạn được sử dụng để lấy địa chỉ. Để làm điều này, hãy đảm bảo rằng Nano của bạn đã được cắm, được mở khóa bằng mã PIN, Ledger Live không chạy và Ứng dụng Solana đang mở trên thiết bị, hiển thị "Application is Ready".

Lệnh `solana transfer` được sử dụng để chỉ định địa chỉ nào sẽ gửi mã thông báo, bao nhiêu mã thông báo được gửi và sử dụng `--keypair` để xác định keypair gửi các mã thông báo, sẽ tiến hành ký giao dịch và số dư từ địa chỉ liên kết sẽ giảm.

```bash
solana transfer RECIPIENT_ADDRESS AMOUNT --keypair KEYPAIR_URL_OF_SENDER
```

Dưới đây là một ví dụ đầy đủ. Đầu tiên, một địa chỉ được xem tại URL keypair. Thứ hai, số dư của địa chỉ tht được kiểm tra. Cuối cùng, một giao dịch chuyển khoản được nhập để gửi `1` SOL đến địa chỉ người nhận `7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri`. Khi bạn nhấn Enter cho một lệnh chuyển, bạn sẽ được nhắc phê duyệt giao dịch trên thiết bị Ledger của mình. Trên thiết bị, sử dụng nút phải và trái để xem lại chi tiết giao dịch. Nếu chúng chính xác, hãy nhấp vào cả hai nút trên màn hình "Approve", nếu không hãy nhấp cả hai nút trên màn hình "Reject".

```bash
~$ solana-keygen pubkey usb://ledger?key=42
CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV

~$ solana balance CjeqzArkZt6xwdnZ9NZSf8D1CNJN1rjeFiyd8q7iLWAV
1.000005 SOL

~$ solana transfer 7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri 1 --keypair usb://ledger?key=42
Waiting for your approval on Ledger hardware wallet usb://ledger/2JT2Xvy6T8hSmT8g6WdeDbHUgoeGdj6bE2VueCZUJmyN
✅ Approved

Signature: kemu9jDEuPirKNRKiHan7ycybYsZp7pFefAdvWZRq5VRHCLgXTXaFVw3pfh87MQcWX4kQY4TjSBmESrwMApom1V
```

Sau khi phê duyệt giao dịch trên thiết bị của bạn, chương trình sẽ hiển thị signature giao dịch và đợi số lượng xác nhận tối đa là (32) trước khi trở về. Qúa trình này chỉ mất vài giây và sau đó giao dịch hoàn thành trên mạng Solana. Bạn có thể xem chi tiết của giao dịch này hoặc bất kỳ giao dịch nào khác bằng cách chuyển đến tab Giao dịch trong [Trình khám phá](https://explorer.solana.com/transactions) và dán signature giao dịch vào.

## Hoạt động nâng cao

### Quản Lý Nhiều Ví Phần Cứng

Đôi khi sẽ hữu ích khi ký một giao dịch bằng các với các khóa từ nhiều ví phần cứng. Việc ký bằng nhiều ví yêu cầu phải có _URL keypair đủ điều kiện_. Khi URL không đủ điều kiện, Solana CLI sẽ nhắc bạn với các URL đủ điều kiện của tất cả các ví phần cứng được kết nối và yêu cầu bạn chọn ví nào sẽ sử dụng cho mỗi chữ ký.

Thay vì sử dụng các lời nhắc tương tác, bạn có thể tạo các URL đủ điều kiện bằng cách sử dụng lệnh Solana CLI `resolve-signer`. Ví dụ, hãy thử kết nối Nano với USB, mở khóa bằng mã pin của bạn và chạy lệnh sau:

```text
solana resolve-signer usb://ledger?key=0/0
```

Bạn sẽ thấy đầu ra tương tự như sau:

```text
usb://ledger/BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK?key=0/0
```

trong đó `BsNsvfXqQTtJnagwFWdBS7FBXgnsK8VZ5CmuznN85swK` là `WALLET_ID` của bạn.

Với URL đủ điều kiện của mình, bạn có thể kết nối nhiều ví phần cứng với cùng một máy tính và xác định duy nhất một keypair từ bất kỳ ví nào trong số đó. Sử dụng đầu ra từ lệnh `resolve-signer` ở bất kỳ nơi nào mà lệnh `solana` yêu cầu `<KEYPAIR>` mục nhập để sử dụng đường dẫn đã phân giải đó làm người ký tên cho một phần của giao dịch nhất định.

## Khắc phục sự cố

### Các thông số URL của Keypair bị bỏ qua trong zsh

Ký tự dấu chấm hỏi là một ký tự đặc biệt trong zsh. Nếu đó không phải là một tính năng bạn sử dụng, hãy thêm dòng sau vào `~/.zshrc` của bạn để coi nó như một ký tự bình thường:

```bash
unsetopt nomatch
```

Sau đó, khởi động lại cửa sổ shell của bạn hoặc chạy `~/.zshrc`:

```bash
source ~/.zshrc
```

Nếu bạn không muốn tắt tính năng xử lý đặc biệt của zsh đối với ký tự dấu chấm hỏi, bạn có thể tắt tính năng này bằng dấu gạch chéo ngược trong các URL keypair của mình. Ví dụ:

```bash
solana-keygen pubkey usb://ledger\?key=0
```

## Hỗ trợ

Hãy xem [Trang hỗ trợ Ví](../support.md) để biết cách nhận trợ giúp.

Đọc thêm về [gửi và nhận mã thông báo](../../cli/transfer-tokens.md) và [ủy quyền stake](../../cli/delegate-stake.md). Bạn có thể sử dụng URL keypair Ledger của mình ở bất kỳ nơi nào bạn thấy tùy chọn hoặc chấp nhận `<KEYPAIR>`.
