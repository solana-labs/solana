---
title: "Thời gian chạy"
---

## Khả năng của các chương trình

Thời gian chạy chỉ cho phép chương trình chủ sở hữu ghi nợ tài khoản hoặc sửa đổi dữ liệu của nó. Sau đó, chương trình xác định các quy tắc bổ sung về việc liệu khách hàng có thể sửa đổi các tài khoản mà nó sở hữu hay không. Trong trường hợp của chương trình Hệ thống, nó cho phép người dùng chuyển các lamport bằng cách nhận dạng chữ ký giao dịch. Nếu thấy khách hàng đã ký giao dịch bằng _private key_ của keypair, nó sẽ biết khách hàng đã ủy quyền chuyển mã thông báo.

Nói cách khác, toàn bộ tập hợp tài khoản thuộc sở hữu của một chương trình nhất định có thể được coi là kho lưu trữ khóa-giá trị trong đó khóa là địa chỉ tài khoản và giá trị là dữ liệu nhị phân tùy ý của chương trình cụ thể. Tác giả chương trình có thể quyết định cách quản lý toàn bộ trạng thái của chương trình có thể là nhiều tài khoản.

Sau khi thời gian chạy thực hiện từng hướng dẫn của giao dịch, nó sử dụng siêu dữ liệu tài khoản để xác minh rằng chính sách truy cập không bị vi phạm. Nếu một chương trình vi phạm chính sách, thời gian chạy sẽ loại bỏ tất cả các thay đổi tài khoản được thực hiện bởi tất cả các hướng dẫn trong giao dịch và đánh dấu giao dịch là không thành công.

### Chính sách

Sau khi một chương trình đã xử lý một lệnh, thời gian chạy xác minh rằng chương trình chỉ thực hiện các hoạt động mà nó được phép và kết quả tuân thủ chính sách thời gian chạy.

Chính sách như sau:
- Chỉ chủ sở hữu của tài khoản mới có thể thay đổi chủ sở hữu.
  - Và chỉ khi tài khoản có thể ghi được.
  - Và chỉ khi tài khoản không thực thi được
  - Và chỉ khi dữ liệu không được khởi tạo hoặc trống.
- Một tài khoản không được chỉ định cho chương trình không bị giảm số dư.
- Số dư của tài khoản chỉ đọc và tài khoản thực thi có thể không thay đổi.
- Chỉ chương trình hệ thống mới có thể thay đổi kích thước của dữ liệu và chỉ khi chương trình hệ thống sở hữu tài khoản.
- Chỉ chủ sở hữu mới có thể thay đổi dữ liệu tài khoản.
  - Và nếu tài khoản có thể ghi được.
  - Và nếu tài khoản không thực thi được.
- Khả năng thực thi là một chiều (false->true) và chỉ chủ sở hữu tài khoản mới có thể thiết lập.
- Không có bất kỳ sửa đổi nào đối với Rent_epoch được liên kết với tài khoản này.

## Tính toán ngân sách

Để ngăn chương trình lạm dụng tài nguyên tính toán, mỗi lệnh trong giao dịch được cấp một ngân sách tính toán.  Các ngân sách bao gồm các đơn vị tính toán được sử dụng khi chương trình thực hiện các hoạt động khác nhau và giới hạn mà chương trình không được vượt quá.  Khi chương trình sử dụng toàn bộ ngân sách hoặc vượt quá giới hạn thì thời gian chạy sẽ tạm dừng chương trình và trả về lỗi.

Các hoạt động sau đây phát sinh chi phí tính toán:
- Thực thi hướng dẫn BPF
- Gọi cuộc gọi hệ thống
  - khai thác gỗ
  - tạo địa chỉ chương trình
  - lời gọi chương trình chéo
  - ...

Đối với các lời gọi chương trình chéo, các chương trình được gọi kế thừa ngân sách của chương trình cha mẹ của chúng.  Nếu một chương trình được gọi tiêu tốn ngân sách hoặc vượt quá giới hạn toàn bộ chuỗi lệnh gọi và chương trình cha mẹ sẽ bị tạm dừng.

Bạn có thể tìm thấy [tính toán ngân sách](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/src/process_instruction.rs#L65) hiện tại trong Solana SDK.

Ví dụ: nếu ngân sách hiện tại là:

```rust
max_units: 200,000,
log_units: 100,
log_u64_units: 100,
create_program address units: 1500,
invoke_units: 1000,
max_invoke_depth: 4,
max_call_depth: 64,
stack_frame_size: 4096,
log_pubkey_units: 100,
```

Sau đó, chương trình
- Có thể thực hiện 200,000 lệnh BPF nếu nó không làm gì khác
- Có thể ghi lại 2,000 tin nhắn nhật ký
- Không được vượt quá 4k mức sử dụng ngăn xếp
- Không được vượt quá độ sâu cuộc gọi BPF là 64
- Không được vượt quá 4 cấp độ của lời gọi chương trình chéo.

Vì ngân sách tính toán được tiêu thụ tăng dần khi chương trình thực hiện nên tổng mức tiêu thụ ngân sách sẽ là sự kết hợp của các chi phí khác nhau của các hoạt động mà chương trình thực hiện.

Trong thời gian chạy, một chương trình có thể ghi lại số lượng ngân sách tính toán còn lại. Xem gỡ lỗi để biết thêm thông tin.  Xem [gỡ lỗi](developing/deployed-programs/debugging.md#monitoring-compute-budget-consumption) để biết thêm thông tin.

Giá trị ngân sách có điều kiện đối với việc kích hoạt tính năng, hãy xem chức năng [ mới của ngân sách tính toán ](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/src/process_instruction.rs#L97) để tìm hiểu cách ngân sách được xây dựng.  Cần có hiểu biết về cách hoạt động của các [tính năng](runtime.md#features) và những tính năng nào được bật trên cụm đang được sử dụng để xác định giá trị của ngân sách hiện tại.

## Các tính năng mới

Khi Solana phát triển, các tính năng hoặc bản vá mới có thể được giới thiệu để thay đổi hành vi của cụm và cách chương trình chạy.  Các thay đổi trong hành vi phải được phối hợp giữa các node khác nhau của cụm, nếu các node không phối hợp thì những thay đổi này có thể dẫn đến sự đồng thuận bị phá vỡ.  Solana hỗ trợ một cơ chế được gọi là các tính năng thời gian chạy để tạo điều kiện thuận lợi cho việc áp dụng các thay đổi một cách suôn sẻ.

Các tính năng thời gian chạy là các sự kiện được phối hợp theo thời gian trong đó một hoặc nhiều thay đổi hành vi đối với cụm sẽ xảy ra.  Những thay đổi mới đối với Solana sẽ thay đổi hành vi được bao bọc bằng cổng tính năng và bị vô hiệu hóa theo mặc định.  Các công cụ Solana sau đó được sử dụng để kích hoạt một tính năng, đánh dấu nó đang chờ xử lý, một khi được đánh dấu là đang chờ xử lý, tính năng đó sẽ được kích hoạt vào kỷ nguyên tiếp theo.

Để xác định tính năng nào được kích hoạt, hãy sử dụng [Các công cụ dòng lệnh Solana](cli/install-solana-cli-tools.md):

```bash
solana feature status
```

Nếu bạn gặp sự cố, trước tiên hãy đảm bảo rằng phiên bản công cụ Solana bạn đang sử dụng khớp với phiên bản được trả về `solana cluster-version`.  Nếu chúng không khớp, hãy [cài đặt đúng bộ công cụ](cli/install-solana-cli-tools.md).
