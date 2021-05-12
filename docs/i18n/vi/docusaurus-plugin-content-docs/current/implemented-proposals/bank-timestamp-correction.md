---
title: Sửa dấu thời gian ngân hàng
---

Mỗi Ngân hàng có một dấu thời gian được lưu trữ trong hệ thống Đồng hồ và được sử dụng để đánh giá việc khóa tài khoản stake dựa trên-thời gian. Tuy nhiên, kể từ khi ra đời, giá trị này dựa trên lý thuyết slot trên giây thay vì thực tế, vì vậy nó khá không chính xác. Điều này đặt ra một vấn đề đối với việc khóa tài khoản, vì các tài khoản sẽ không đăng ký là không bị khóa vào (hoặc bất cứ lúc nào gần) ngày hết hạn khóa.

Thời gian khối đã được ước tính để lưu vào bộ nhớ cache trong Blockstore và lưu trữ dài hạn bằng cách sử dụng [validator timestamp oracle](validator-timestamp-oracle.md); dữ liệu này tạo cơ hội để điều chỉnh dấu thời gian của ngân hàng chặt chẽ hơn với thời gian trong thế giới thực.

Đề cương chung của việc thực hiện được đề xuất như sau:

- Sửa từng dấu thời gian của Ngân hàng bằng cách sử dụng dấu thời gian do validator cung cấp.
- Cập nhật phép tính dấu thời gian do validator cung cấp để sử dụng giá trị trung bình có trọng lượng tiền stake, thay vì giá trị trung bình có trọng lượng tiền stake.
- Ràng buộc hiệu chỉnh dấu thời gian để nó không thể sai lệch quá xa so với ước tính lý thuyết dự kiến

## Sửa dấu thời gian

Trên Ngân hàng mới, thời gian chạy tính toán ước tính dấu thời gian thực tế bằng cách sử dụng dữ liệu dấu thời gian-oracle của validator. Dấu thời gian của Ngân hàng được sửa thành giá trị này nếu nó lớn hơn hoặc bằng dấu thời gian của Ngân hàng trước đó. Có nghĩa là, thời gian không bao giờ được quay ngược trở lại, do đó, các tài khoản bị khóa có thể được giải phóng khi sửa chữa, nhưng một khi được giải phóng, các tài khoản không bao giờ có thể được khóa lại bằng việc sửa thời gian.

### Tính toán dấu thời gian trung bình có tỉ trọng stake

Để tính toán dấu thời gian ước tính cho một Ngân hàng cụ thể, thời gian chạy trước tiên cần lấy dấu thời gian bỏ phiếu gần đây nhất từ ​​bộ validator đang hoạt động. Phương thức `Bank::vote_accounts()` cung cấp trạng thái tài khoản bỏ phiếu và chúng có thể được lọc cho tất cả các tài khoản có dấu thời gian gần đây nhất được cung cấp trong kỷ nguyên trước.

Từ mỗi dấu thời gian bỏ phiếu, ước tính cho Ngân hàng hiện tại được tính bằng cách sử dụng ns_per_slot mục tiêu của kỷ nguyên cho bất kỳ khoảng bằng nào giữa slot Ngân hàng và slot dấu thời gian. Mỗi ước tính dấu thời gian được liên kết với cổ phần được ủy quyền cho tài khoản bỏ phiếu đó và tất cả các dấu thời gian được thu thập để tạo phân phối dấu thời gian có trọng lượng tiề stake.

Từ tập hợp này, dấu thời gian trung bình có trọng số tiền stake - nghĩa là, dấu thời gian mà tại đó 50% số tiền stake ước tính dấu thời gian lớn hơn hoặc bằng và 50% số tiền stake ước tính dấu thời gian nhỏ hơn hoặc bằng - được chọn làm dấu thời gian được sửa chữa tiềm năng.

Dấu thời gian trung bình có tỉ trọng tiền stake này được ưu tiên hơn so với giá trị trung bình có tỉ trọng tiền stake vì việc nhân tiền stake với dấu thời gian được đề xuất trong phép tính trung bình cho phép một node có số tiền stake rất nhỏ vẫn có ảnh hưởng lớn đến dấu thời gian kết quả bằng cách đề xuất một dấu thời gian lớn hoặc rất nhỏ. Ví dụ: sử dụng `calculate_stake_weighted_timestamp()` phương pháp trước, một node có stake 0,00003% đề xuất dấu thời gian `i64::MAX` có thể thay đổi dấu thời gian về phía trước 97 nghìn năm!

### Dấu thời gian giới hạn

Ngoài việc ngăn thời gian lùi lại, chúng tôi có thể ngăn chặn hoạt động độc hại bằng cách giới hạn dấu thời gian đã sửa ở mức độ sai lệch có thể chấp nhận được so với thời gian dự kiến ​​trên lý thuyết.

Đề xuất này đề xuất rằng mỗi dấu thời gian được phép sai lệch tối đa 25% so với thời gian dự kiến ​​kể từ khi bắt đầu kỷ nguyên.

Để tính toán độ lệch dấu thời gian, mỗi Ngân hàng cần đăng nhập `epoch_start_timestamp` hệ thống Đồng hồ. Giá trị này được đặt ở `Clock::unix_timestamp` slot đầu tiên của mỗi kỷ nguyên.

Sau đó, thời gian chạy so sánh thời gian đã trôi qua dự kiến ​​kể từ khi bắt đầu kỷ nguyên với thời gian đã trôi qua được đề xuất dựa trên dấu thời gian đã sửa. Nếu thời gian đã trôi qua đã hiệu chỉnh nằm trong khoảng +/- 25% so với dự kiến, dấu thời gian đã sửa được chấp nhận. Nếu không, nó bị ràng buộc với độ lệch có thể chấp nhận được.
