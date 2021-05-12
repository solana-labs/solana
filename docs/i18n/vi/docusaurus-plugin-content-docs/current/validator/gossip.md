---
title: Dịch vụ Gossip
---

Dịch vụ Gossip hoạt động như một cổng vào các node trong mặt phẳng điều khiển. Các validator sử dụng dịch vụ để đảm bảo thông tin có sẵn cho tất cả các node khác trong một cụm. Dịch vụ truyền phát thông tin bằng giao thức gossip.

## Tổng quan về Gossip

Các node liên tục chia sẻ các đối tượng dữ liệu đã ký với nhau để quản lý một cụm. Ví dụ, họ chia sẻ thông tin liên hệ, chiều cao sổ cái và phiếu bầu.

Mỗi phần mười giây, mỗi node sẽ gửi một tin nhắn "push" và/hoặc một tin nhắn "pull". Tin nhắn push và pull có thể gợi ra phản hồi và tin nhắn push có thể được chuyển tiếp đến những người khác trong cụm.

Gossip chạy trên một cổng UDP/IP nổi tiếng hoặc một cổng trong một phạm vi nổi tiếng. Khi một cụm được khởi động, các node quảng cáo cho nhau nơi tìm điểm cuối gossip của chúng \(một địa chỉ socket\).

## Bản ghi Gossip

Các bản ghi được chia sẻ qua gossip là tùy ý, nhưng được ký và tạo phiên bản \(với dấu thời gian\) khi cần thiết để có ý nghĩa đối với node nhận chúng. Nếu một node nhận được hai bản ghi từ cùng một nguồn, nó sẽ cập nhật bản sao của chính nó với bản ghi có dấu thời gian gần đây nhất.

## Giao diện dịch vụ Gossip

### Tin nhắn Push

Một node sẽ gửi một tin nhắn push để cho cụm biết rằng nó có thông tin để chia sẻ. Các node gửi tin nhắn push đến `PUSH_FANOUT` để đẩy đồng nghiệp.

Khi nhận được tin nhắn push, một node sẽ kiểm tra tin nhắn để tìm:

1. Sao chép: nếu tin nhắn đã được nhìn thấy trước đó, node sẽ bỏ tin nhắn và có thể phản hồi `PushMessagePrune` nếu được chuyển tiếp từ một node có stake thấp
2. Dữ liệu mới: nếu tin nhắn là mới cho node

   - Lưu trữ thông tin mới với phiên bản cập nhật trong thông tin cụm của nó và

     xóa mọi giá trị cũ hơn trước đó

   - Lưu trữ tin nhắn trong `pushed_once` \(được sử dụng để phát hiện các bản sao,

     được thúc giục sau `PUSH_MSG_TIMEOUT * 5` ms\)

   - Truyền lại các tin nhắn đến các đồng nghiệp đẩy của chính nó

3. Hết hạn: các node thả thông báo đẩy cũ hơn `PUSH_MSG_TIMEOUT`

### Push ngang hàng, tin nhắn Prune

Một node chọn ngẫu nhiên các push ngang hàng của nó từ tập hợp các ngang hàng đã biết đang hoạt động. Node giữ lựa chọn này trong một thời gian tương đối dài. Khi nhận được tin nhắn sơ lược, node loại bỏ ứng dụng push đã gửi sơ lược. Prune là một dấu hiệu cho thấy có một đường dẫn khác, có tỉ trọng stake cao hơn đến node đó hơn là push trực tiếp.

Tập hợp các push ngang hàng được giữ mới bằng cách xoay một node mới vào tập hợp mỗi `PUSH_MSG_TIMEOUT/2` mili giây.

### Tin nhắn Pull

Một node sẽ gửi một tin nhắn pull để hỏi cụm xem có bất kỳ thông tin mới nào không. Một tin nhắn pull được gửi ngẫu nhiên đến một người ngang hàng và bao gồm một bộ lọc Bloom đại diện cho những thứ mà nó đã có. Một node nhận được tin nhắn pull sẽ lặp lại các giá trị của nó và xây dựng phản hồi pull của những thứ bộ lọc bỏ sót và sẽ phù hợp với một tin nhắn.

Một node xây dựng bộ lọc Bloom pull bằng cách lặp lại các giá trị hiện tại và các giá trị đã xóa gần đây.

Một node xử lý các mục trong phản hồi pull giống như cách nó xử lý dữ liệu mới trong tin nhắn push.

## Thanh lọc

Các node giữ lại các phiên bản trước của giá trị \(những giá trị được cập nhật bằng cách pull hoặc push\) và các giá trị đã hết hạn \(những thứ cũ hơn `GOSSIP_PULL_CRDS_TIMEOUT_MS`\) trong `purged_values` \(những thứ tôi đã có gần đây\). Xóa các node `purged_values` cũ hơn `5 * GOSSIP_PULL_CRDS_TIMEOUT_MS`.

## Cuộc tấn công Eclipse

Một cuộc tấn công eclipse là một nỗ lực để chiếm lấy tập hợp các kết nối node với các điểm cuối đối nghịch.

Điều này có liên quan đến việc triển khai của chúng tôi theo những cách sau.

- Tin nhắn pull chọn một node ngẫu nhiên từ mạng. Một cuộc tấn công eclipse khi _pull_ sẽ yêu cầu kẻ tấn công tác động đến lựa chọn ngẫu nhiên theo cách mà chỉ các node đối nghịch được chọn để pull.
- Tin nhắn push duy trì một tập hợp các node đang hoạt động và chọn một fanout ngẫu nhiên cho mỗi tin nhắn push. Một cuộc tấn công eclipse khi _push_ sẽ ảnh hưởng đến lựa chọn tập hợp đang hoạt động hoặc lựa chọn fanout ngẫu nhiên.

### Tỉ trọng dựa trên thời gian và stake

Trọng lượng được tính toán dựa trên `time since last picked` và `natural log` của `stake weight`.

Việc tính toán `ln` của tỷ trọng stake cho phép cung cấp cho tất cả các node cơ hội phủ sóng mạng công bằng hơn trong một khoảng thời gian hợp lý. Nó giúp bình thường hóa sự khác biệt lớn về `stake weight` có thể có giữa các node. Bằng cách này, một node có `stake weight` thấp, so với một node có `stake weight` lớn sẽ chỉ phải đợi một vài bội số ln\(`stake`\) vài giây trước khi nó được chọn.

Không có cách nào để đối thủ có thể tác động đến các thông số này.

### Tin nhắn Pull

Một node sẽ gửi một thông điệp pull để hỏi cụm xem có bất kỳ thông tin mới nào không.

### Tin nhắn Push

Một thông điệp sơ lược chỉ có thể loại bỏ kẻ thù khỏi một kết nối tiềm năng.

Cũng giống như _tin nhắn pull_, các node được chọn vào tập hợp hoạt động dựa trên trọng số.

## Sự khác biệt đáng chú ý so với PlumTree

Giao thức push hoạt động được mô tả ở đây dựa trên [Plum Tree](https://haslab.uminho.pt/sites/default/files/jop/files/lpr07a.pdf). Sự khác biệt chính là:

- Tin nhắn push có một wallclock được ký bởi người tạo. Khi wallclock hết hạn, tin nhắn sẽ bị xóa. Giới hạn bước nhảy rất khó thực hiện trong bối cảnh đối đầu.
- Lazy Push không được triển khai vì nó không rõ ràng về cách ngăn kẻ thù giả mạo tin nhắn dấu vân tay. Một cách tiếp cận ngây thơ sẽ cho phép một đối thủ được ưu tiên pull dựa trên đầu vào của họ.
