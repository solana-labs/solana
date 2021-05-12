---
title: Xác minh Ảnh chụp nhanh
---

## Sự cố

Khi một validator khởi động từ ảnh chụp nhanh, nó cần có cách để xác minh bộ tài khoản khớp với những gì phần còn lại của mạng được nhìn thấy một cách nhanh chóng. Kẻ tấn công tiềm năng có thể cung cấp cho validator một trạng thái không chính xác và sau đó cố gắng thuyết phục nó chấp nhận một giao dịch mà nếu không sẽ bị từ chối.

## Giải pháp

Hiện tại, hàm băm ngân hàng có nguồn gốc từ việc băm trạng thái đồng bằng của các tài khoản trong một slot, sau đó được kết hợp với giá trị hàm băm của ngân hàng trước đó. Vấn đề với điều này là danh sách các hàm băm sẽ phát triển theo thứ tự của số lượng slot được xử lý bởi chuỗi và trở thành gánh nặng cho cả việc truyền và xác minh thành công.

Một phương pháp ngây thơ khác có thể là tạo một cây merkle của trạng thái tài khoản. Điều này có nhược điểm là với mỗi lần cập nhật tài khoản, cây merkle sẽ phải được tính toán lại từ toàn bộ trạng thái tài khoản của tất cả các tài khoản đang hoạt động trong hệ thống.

Để xác minh ảnh chụp nhanh, chúng tôi thực hiện như sau:

Trên kho lưu trữ tài khoản của các tài khoản lamport khác không, chúng tôi băm dữ liệu sau:

- Chủ tài khoản
- Dữ liệu tài khoản
- Tài khoản pubkey
- Số dư tài khoản lamport
- Fork tài khoản được lưu trữ trên

Sử dụng giá trị kết quả băm này làm đầu vào cho một hàm mở rộng sẽ mở rộng giá trị băm thành giá trị hình ảnh. Hàm sẽ tạo một khối dữ liệu 440 byte trong đó 32 byte đầu tiên là giá trị băm và 440 - 32 byte tiếp theo được tạo từ Chacha RNG với hàm băm là hạt giống.

Các hình ảnh tài khoản sau đó được kết hợp với xor. Giá trị tài khoản trước đó sẽ được tính vào trạng thái và giá trị tài khoản mới cũng được tính vào trạng thái.

Bỏ phiếu và giá trị hàm băm hệ thống xảy ra với hàm băm của giá trị hình ảnh đầy đủ thu được.

Trên validator boot, khi tải từ một ảnh chụp nhanh, nó sẽ xác minh giá trị băm với các tài khoản đã đặt. Sau đó, nó sẽ sử dụng SPV để hiển thị phần trăm mạng đã bỏ phiếu cho giá trị băm đã cho.

Giá trị kết quả có thể được xác nhận bởi một validator là kết quả của việc gộp tất cả các trạng thái tài khoản hiện tại lại với nhau.

Một ảnh chụp nhanh phải được xóa tài khoản zero lamport trước khi tạo và trong quá trình xác minh vì tài khoản zero lamport không ảnh hưởng đến giá trị hàm băm nhưng có thể gây ra một ngân hàng xác thực để đọc một tài khoản không xuất hiện khi nó thực sự nên như vậy.

Một cuộc tấn công vào trạng thái xor có thể được thực hiện để ảnh hưởng đến giá trị của nó:

Do đó, kích thước hình ảnh 440 byte lấy từ giấy này, tránh va chạm xor với 0 ( hoặc bất kỳ mẫu bit nào khác): \[[https://link.springer.com/content/pdf/10.1007%2F3-540-45708-9_19.pdf](https://link.springer.com/content/pdf/10.1007%2F3-540-45708-9_19.pdf)\]

Phép toán cung cấp bảo mật 128 bit trong trường hợp này:

```text
O(k * 2^(n/(1+lg(k)))
k=2^40 accounts
n=440
2^(40) * 2^(448 * 8 / 41) ~= O(2^(128))
```
