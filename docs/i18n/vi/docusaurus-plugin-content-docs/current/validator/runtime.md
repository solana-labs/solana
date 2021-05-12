---
title: Thời gian chạy
---

## Thời gian chạy

Thời gian chạy là một bộ xử lý giao dịch đồng thời. Các giao dịch chỉ định trước các phụ thuộc dữ liệu của chúng và phân bổ bộ nhớ động là rõ ràng. Bằng cách tách mã chương trình khỏi trạng thái mà nó hoạt động, thời gian chạy có thể dàn dựng truy cập đồng thời. Các giao dịch truy cập tài khoản chỉ đọc được thực hiện song song trong khi các giao dịch truy cập tài khoản có thể ghi được tuần tự. Thời gian chạy tương tác với chương trình thông qua một điểm nhập có giao diện được xác định rõ ràng. Dữ liệu được lưu trữ trong tài khoản là một loại không rõ ràng, một mảng các byte. Chương trình có toàn quyền kiểm soát nội dung của nó.

Cấu trúc giao dịch chỉ định danh sách các khóa công khai và chữ ký cho các khóa đó và danh sách tuần tự các hướng dẫn sẽ hoạt động trên các trạng thái được liên kết với khóa tài khoản. Để giao dịch được cam kết, tất cả các hướng dẫn phải được thực hiện thành công; nếu có bất kỳ hủy bỏ, toàn bộ giao dịch không được cam kết.

#### Cấu trúc Tài khoản

Các tài khoản duy trì số dư lamport và bộ nhớ theo chương trình cụ thể.

## Công cụ giao dịch

Công cụ ánh xạ các public key đến các tài khoản và định tuyến chúng đến điểm nhập của chương trình.

### Thực thi

Các giao dịch được thực hiện theo từng đợt và được xử lý theo một quy trình. TPU và TVU đi theo một con đường hơi khác. Thời gian chạy TPU đảm bảo rằng bản ghi PoH xảy ra trước khi bộ nhớ được cam kết.

Thời gian chạy TVU đảm bảo rằng xác minh PoH xảy ra trước khi thời gian chạy xử lý bất kỳ giao dịch nào.

![Đường ống thời gian chạy](/img/runtime.svg)

Ở giai đoạn _thực thi_, các tài khoản được tải không có phụ thuộc dữ liệu, vì vậy tất cả các chương trình có thể được thực thi song song.

Thời gian chạy thi hành các quy tắc sau:

1. Chỉ chương trình _chủ sở hữu_ mới có thể sửa đổi nội dung của tài khoản. Điều này có nghĩa là vectơ dữ liệu khi gán được đảm bảo bằng 0.
2. Tổng số dư trên tất cả các tài khoản bằng nhau trước và sau khi thực hiện giao dịch.
3. Sau khi giao dịch được thực hiện, số dư của tài khoản chỉ đọc phải bằng số dư trước khi giao dịch.
4. Tất cả các hướng dẫn trong giao dịch được thực hiện nguyên tử. Nếu không thành công, tất cả các sửa đổi tài khoản sẽ bị hủy.

Việc thực thi chương trình liên quan đến việc ánh xạ public key của chương trình tới một điểm nhập có một con trỏ đến giao dịch và một loạt các tài khoản được tải.

### Giao diện chương trình hệ thống

Giao diện được mô tả tốt nhất bằng `Instruction::data` mà người dùng mã hóa.

- `CreateAccount` - Điều này cho phép người dùng tạo tài khoản với một mảng dữ liệu được cấp phát và gán nó cho một Chương trình.
- `CreateAccountWithSeed` - Giống như `CreateAccount`, nhưng địa chỉ của tài khoản mới được lấy từ
  - pubkey của tài khoản tài trợ,
  - một chuỗi ghi nhớ (hạt giống), và
  - pubkey của Chương trình
- `Assign` - Cho phép người dùng gán tài khoản hiện có cho chương trình.
- `Transfer` - Chuyển các lamport giữa các tài khoản.

### Chương trình An ninh

Để blockchain hoạt động chính xác, mã chương trình phải có khả năng phục hồi đối với đầu vào của người dùng. Đó là lý do tại sao trong thiết kế này mã cụ thể chương trình là mã duy nhất có thể thay đổi trạng thái của mảng byte dữ liệu trong các tài khoản được gán cho nó. Đó cũng là lý do tại sao `Assign` hoặc `CreateAccount` phải loại bỏ dữ liệu. Nếu không, chương trình sẽ không có cách nào để phân biệt dữ liệu tài khoản được chỉ định gần đây với chuyển đổi trạng thái được tạo nguyên bản mà không có một số siêu dữ liệu bổ sung từ thời gian chạy để chỉ ra rằng bộ nhớ này được gán thay vì được tạo tự nhiên.

Để chuyển thông báo giữa các chương trình, chương trình nhận phải chấp nhận thông báo và sao chép trạng thái. Nhưng trong thực tế, một bản sao là không cần thiết và là điều không mong muốn. Chương trình tiếp nhận có thể đọc trạng thái thuộc về các tài khoản khác mà không sao chép nó, và trong khi đọc nó có một sự bảo đảm về trạng thái của chương trình người gửi.

### Ghi chú

- Không có phân bổ bộ nhớ động. Khách hàng cần sử dụng các hướng dẫn `CreateAccount` để tạo bộ nhớ trước khi chuyển nó sang chương trình khác. Hướng dẫn này có thể được làm thành một giao dịch duy nhất với lệnh gọi đến chính chương trình.
- `CreateAccount` và `Assign`đảm bảo rằng khi tài khoản được chỉ định cho chương trình, dữ liệu của Tài khoản sẽ không được khởi tạo.
- Các giao dịch chỉ định tài khoản cho một chương trình hoặc phân bổ không gian phải được ký bằng khóa cá nhân của địa chỉ tài khoản trừ khi Tài khoản được tạo bởi `CreateAccountWithSeed`, trong trường hợp đó không có khóa riêng tương ứng cho địa chỉ/pubkey của tài khoản.
- Sau khi được chỉ định để lập trình, một Tài khoản không thể được chỉ định lại.
- Thời gian chạy đảm bảo rằng mã của chương trình là mã duy nhất có thể sửa đổi dữ liệu Tài khoản mà Tài khoản được chỉ định.
- Thời gian chạy đảm bảo rằng chương trình chỉ có thể sử dụng các lamport có trong tài khoản được chỉ định cho nó.
- Thời gian chạy đảm bảo số dư của các tài khoản được cân bằng trước và sau khi giao dịch.
- Thời gian chạy đảm bảo rằng tất cả các lệnh được thực hiện thành công khi một giao dịch được cam kết.

## Công việc tương lai

- [Sự liên tục và tín hiệu cho các Giao dịch dài hạn](https://github.com/solana-labs/solana/issues/1485)
