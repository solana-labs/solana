---
title: Chuyển tiếp leader đến leader
---

Thiết kế này mô tả cách các leader chuyển đổi quá trình sản xuất sổ cái PoH giữa nhau khi mỗi leader tạo ra slot riêng của mình.

## Thử thách

Leader hiện tại và leader tiếp theo đều đang chạy đua để tạo ra đánh dấu cuối cùng cho slot hiện tại. Leader tiếp theo có thể đến slot đó trong khi vẫn xử lý các mục nhập của leader hiện tại.

Kịch bản lý tưởng sẽ là leader tiếp theo tạo ra slot riêng của mình ngay sau khi họ có thể bỏ phiếu cho leader hiện tại. Rất có thể leader tiếp theo sẽ đến độ cao slot PoH của họ trước khi leader hiện tại kết thúc việc phát toàn bộ khối.

Leader tiếp theo phải đưa ra quyết định gắn khối riêng của mình vào khối đã hoàn thành cuối cùng hoặc đợi để hoàn thiện khối đang chờ xử lý. Có thể leader tiếp theo sẽ tạo ra một khối đề xuất rằng leader hiện tại đã thất bại, mặc dù phần còn lại của mạng quan sát thấy khối đó thành công.

Leader hiện tại có các động lực để bắt đầu slot của mình càng sớm càng tốt để thu được phần thưởng kinh tế. Những ưu đãi đó cần được cân bằng bởi nhu cầu của leader gắn khối của nó vào một khối có nhiều cam kết nhất từ ​​phần còn lại của mạng.

## Thời gian chờ của Leader

Trong khi leader đang tích cực nhận các mục nhập cho slot trước đó, leader có thể trì hoãn việc phát sóng bắt đầu khối của nó trong thời gian thực. Độ trễ có thể được cấu hình cục bộ bởi mỗi leader và có thể tự động dựa trên hành vi của leader trước đó. Nếu khối của leader trước đó được TVU của leader đó xác nhận trước thời gian chờ, PoH được đặt lại về đầu slot và leader này tạo ra khối của nó ngay lập tức.

Nhược điểm:

- Leader trì hoãn slot của mình, có khả năng cho phép leader tiếp theo có thêm thời gian để

  bắt kịp.

Những ưu điểm so với guards:

- Tất cả không gian trong một khối được sử dụng cho các mục nhập.
- Thời gian chờ không cố định.
- Thời gian chờ là cục bộ đối với leader, và do đó có thể thông minh. Kinh nghiệm của leader có thể tính đến hiệu suất turbine.
- Thiết kế này không yêu cầu hard fork sổ cái để cập nhật.
- Leader trước đó có thể truyền thừa mục nhập cuối cùng trong khối cho leader tiếp theo và leader tiếp theo có thể suy đoán tin cậy để tạo khối mà không cần xác minh khối trước đó.
- Leader có thể suy đoán tạo ra đánh dấu cuối cùng từ mục nhập nhận được cuối cùng.
- Leader có thể suy đoán xử lý các giao dịch và đoán những giao dịch nào sẽ không được leader trước đó mã hóa. Đây cũng là một vector tấn công kiểm duyệt. Leader hiện tại có thể giữ lại các giao dịch mà nó nhận được từ khách hàng để nó có thể mã hóa chúng vào slot riêng của mình. Sau khi được xử lý, các mục nhập có thể được phát lại vào PoH một cách nhanh chóng.

## Tùy chọn thiết kế thay thế

### Đánh dấu bảo vệ ở cuối slot

Một leader không tạo các mục nhập trong khối của mình sau đánh dấu áp chót, là lần đánh dấu cuối cùng trước đánh dấu đầu tiên của slot tiếp theo. Mạng bỏ phiếu vào lần _đánh dấucuối cùng_, do đó, chênh lệch thời gian giữa lần _tick áp chót_ và lần _đánh dấu cuối cùng_ là độ trễ bắt buộc đối với toàn bộ mạng, cũng như leader tiếp theo trước khi có thể tạo slot mới. Mạng có thể tạo ra _đánh dấu cuối cùng_ từ _đánh dấu áp chót_.

Nếu leader tiếp theo nhận được _đánh dấu áp chót_ trước khi nó tạo ra _đánh dấu đầu tiên_ của riêng mình, nó sẽ đặt lại PoH của mình và tạo ra _đánh dấu đầu tiên_ từ _đánh dấu áp chót_ của leader trước đó. Phần còn lại của mạng cũng sẽ đặt lại PoH của nó để tạo ra _đánh dấu cuối cùng_ làm id để bỏ phiếu.

Nhược điểm:

- Mọi phiếu bầu, và do xác nhận, đều bị trì hoãn bởi thời gian chờ cố định. 1 đánh dấu, hoặc khoảng 100ms.
- Thời gian xác nhận trung bình cho một giao dịch sẽ thấp hơn ít nhất 50 mili giây.
- Nó là một phần của định nghĩa sổ cái, vì vậy để thay đổi hành vi này sẽ cần một đợt hard fork.
- Không phải tất cả không gian có sẵn đều được sử dụng cho các mục nhập.

Ưu điểm so với thời gian chờ của leader:

- Leader tiếp theo đã nhận được tất cả các mục nhập trước đó, vì vậy nó có thể bắt đầu xử lý các giao dịch mà không cần ghi chúng vào PoH.
- Leader trước đó có thể truyền dư thừa mục nhập cuối cùng có _đánh dấu áp chót_ cho leader tiếp theo. Leader tiếp theo có thể suy đoán tạo ra _đánh dấu cuối cùng_</em> ngay khi nhận được _đánh dấu áp chót_, thậm chí trước khi xác minh nó.
