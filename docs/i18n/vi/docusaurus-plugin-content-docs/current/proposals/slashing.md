---
title: Quy tắc Slashing
---

Không giống như Proof of Work \(PoW\), nơi đã có chi phí vốn ngoài chuỗi được triển khai tại thời điểm xây dựng khối/bỏ phiếu, các hệ thống PoS yêu cầu vốn có rủi ro để ngăn chặn chiến lược hợp lý/tối ưu của nhiều chuỗi bỏ phiếu. Chúng tôi dự định thực hiện các quy tắc slashing, nếu bị phá vỡ, một số tiền stake của validator vi phạm sẽ bị xóa khỏi lưu thông. Với các thuộc tính sắp xếp của cấu trúc dữ liệu PoH, chúng tôi tin rằng chúng tôi có thể đơn giản hóa các quy tắc slashing của mình đến mức thời gian lockout biểu quyết được chỉ định cho mỗi phiếu bầu.

Tức là. Mỗi phiếu bầu có thời gian lockout liên quan \(thời gian PoH\) thể hiện thời hạn bởi bất kỳ phiếu bầu bổ sung nào từ validator đó phải nằm trong PoH có chứa phiếu bầu ban đầu hoặc một phần stake của validator đó có thể xử lý được. Khoảng thời gian này là một hàm của số PoH phiếu bầu ban đầu và tất cả số lượng PoH phiếu bầu bổ sung. Nó có thể sẽ có dạng:

```text
Lockouti\(PoHi, PoHj\) = PoHj + K \* exp\(\(PoHj - PoHi\) / K\)
```

Trong đó PoHi là chiều cao của phiếu bầu mà lockout sẽ được áp dụng và PoHj là chiều cao của phiếu bầu hiện tại trên cùng một fork. Nếu validator gửi phiếu bầu về một fork PoH khác trên bất kỳ PoHk nào mà k &gt; j &gt; i và PoHk &lt; Lockout\(PoHi, PoHj\), thì một phần stake của validator đó có nguy cơ bị cắt.

Ngoài việc lockout biểu mẫu chức năng được mô tả ở trên, việc triển khai sớm có thể là một phép gần đúng số dựa trên cấu trúc dữ liệu Đầu vào, Đầu ra trước \(FIFO\) và logic sau:

- Hàng đợi FIFO giữ 32 phiếu bầu cho mỗi validator đang hoạt động
- phiếu bầu mới được đẩy lên đầu hàng đợi \(`push_front`\)
- phiếu bầu đã hết hạn được bật lên trên cùng \(`pop_front`\)
- khi phiếu bầu được đẩy vào hàng đợi, việc lockout mỗi phiếu bầu đã xếp hàng sẽ tăng gấp đôi
- phiếu bầu bị di chuyển khỏi hàng đợi nếu `queue.len() > 32`
- chiều cao sớm nhất và mới nhất đã được di chuyển khỏi mặt sau của hàng đợi sẽ được lưu trữ

Có khả năng phần thưởng sẽ được cung cấp dưới dạng % của số tiền bị cắt cho bất kỳ node nào gửi bằng chứng về việc điều kiện slashing này bị vi phạm đến PoH.

### Slashing một phần

Trong lược đồ được mô tả cho đến nay, khi một validator bỏ phiếu trên một luồng PoH nhất định, họ đang cam kết với fork đó trong một thời gian được xác định bởi lockout bỏ phiếu. Một câu hỏi mở là liệu những validator có do dự khi bắt đầu bỏ phiếu cho một đợt fork có sẵn hay không nếu các hình phạt được cho là quá khắc nghiệt đối với một sai lầm trung thực hoặc một chút sai sót.

Một cách để giải quyết mối quan tâm này sẽ là một thiết kế slashing một phần dẫn đến một số tiền có thể cắt giảm như một chức năng của một trong hai:

1. phần của validator, trong tổng số validator, cũng bị cắt trong cùng khoảng thời gian \(ala Casper\)
2. khoảng thời gian kể từ khi bỏ phiếu \(ví dụ: % tăng tuyến tính trong tổng số tiền gửi dưới dạng số tiền có thể cắt giảm theo thời gian\),, hoặc cả hai.

Đây là một khu vực hiện đang được thăm dò.
