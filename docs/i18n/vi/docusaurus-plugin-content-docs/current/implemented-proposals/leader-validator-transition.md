---
title: Chuyển đổi Leader sang Validator
---

Một validator thường dành thời gian để xác thực các khối. Tuy nhiên, nếu người tham gia stake ủy quyền cổ phần của mình cho một validator,, thì người đó đôi khi sẽ được chọn làm _slot leader_. Với tư cách là slot leader, validator chịu trách nhiệm sản xuất các khối trong một _slot_ được chỉ định. Một slot có khoảng thời gian của một số _đánh dấu_ được cấu hình sẵn. Thời lượng của những lần đánh dấu đó được ước tính bằng _Máy ghi PoH_ được mô tả sau trong tài liệu này.

## BankFork

BankFork theo dõi các thay đổi đối với trạng thái ngân hàng qua một slot cụ thể. Sau khi đánh dấu cuối cùng đã được đăng ký, trạng thái sẽ bị đóng băng. Mọi nỗ lực ghi đều bị từ chối.

## Trình xác thực

Một validator hoạt động trên nhiều fork đồng thời khác nhau của trạng thái ngân hàng cho đến khi nó tạo ra hàm băm PoH có chiều cao trong leader slot của nó.

## Slot Leader

Một slot leader xây dựng các khối chỉ trên một fork, cái mà người đó bỏ phiếu lần cuối.

## Máy ghi PoH

Các slot leader và các validator sử dụng Trình ghi PoH để ước tính chiều cao slot và để ghi lại các giao dịch.

### Máy ghi PoH khi xác thực

PoH Máy ghi hoạt động như một VDF đơn giản khi xác thực. Nó cho validator biết khi nào nó cần chuyển sang vai trò slot leader. Mỗi khi validator bỏ phiếu cho một đợt fork, nó sẽ sử dụng [blockhash](../terminology.md#blockhash) mới nhất của đợt fork để phân loại lại VDF. Re-seeding giải quyết được hai vấn đề. Đầu tiên, nó đồng bộ hóa VDF của nó với vị trí leader, cho phép nó xác định chính xác hơn thời điểm slot leader của nó bắt đầu. Thứ hai, nếu leader trước đó gặp sự cố, tất cả thời gian ép xung sẽ được tính vào luồng PoH của leader tiếp theo. Ví dụ: nếu thiếu một khối khi khối leader bắt đầu, khối mà nó tạo ra phải có thời lượng PoH là hai khối. Thời lượng dài hơn đảm bảo leader sau không cố gắng cắt tất cả các giao dịch từ slot của leader trước đó.

### Máy ghi PoH khi dẫn đầu

Một slot leader sử dụng PoH Recorder để ghi lại các giao dịch, khóa các vị trí của họ kịp thời. Hàm băm PoH phải được bắt nguồn từ khối cuối cùng của leader trước đó. Nếu không, khối của nó sẽ không xác minh được PoH và bị cụm từ chối.

Máy ghi PoH cũng dùng để thông báo cho slot leader khi slot của nó kết thúc. Leader cần chú ý không sửa đổi ngân hàng của mình nếu việc ghi lại giao dịch sẽ tạo ra chiều cao PoH bên ngoài slot được chỉ định của nó. Do đó, leader không nên thực hiện các thay đổi tài khoản cho đến khi nó tạo ra hàm băm PoH của mục nhập. Khi độ cao PoH nằm ngoài slot của nó, bất kỳ giao dịch nào trong đường dẫn của nó có thể bị giảm hoặc chuyển tiếp đến leader tiếp theo. Chuyển tiếp được ưu tiên hơn, vì nó sẽ giảm thiểu tắc nghẽn mạng, cho phép cụm quảng cáo dung lượng TPS cao hơn.

## Vòng lặp Validator

Máy ghi PoH quản lý quá trình chuyển đổi giữa các chế độ. Sau khi một sổ cái được phát lại, validator có thể chạy cho đến khi trình ghi cho biết nó phải là slot leader. Là một slot leader, node sau đó có thể thực hiện và ghi lại các giao dịch.

Vòng lặp được đồng bộ hóa với PoH và thực hiện khởi động và dừng đồng bộ chức năng slot leader. Sau khi dừng, TVU của validator sẽ tự thấy ở trạng thái giống như thể một leader khác đã gửi nó cùng một khối. Sau đây là mã giả cho vòng lặp:

1. Truy vấn LeaderScheduler cho slot được chỉ định tiếp theo.
2. Chạy TVU trên tất cả các fork. 1. TVU sẽ gửi phiếu bầu cho những gì họ tin là fork "tốt nhất". 2. Sau mỗi lần bỏ phiếu, hãy khởi động lại Máy ghi PoH để chạy cho đến lần được chỉ định tiếp theo

   slot.

3. Khi đến lúc trở thành slot leader, hãy bắt đầu TPU. Chỉ nó đến fork cuối cùng

   TVU bình chọn.

4. Sản xuất các mục nhập cho đến khi kết thúc slot. 1. Trong suốt thời gian của slot, TVU không được bỏ phiếu cho các fork khác. 2. Sau khi slot kết thúc, TPU đóng băng BankFork của nó. Sau khi đóng băng,

   TVU có thể tiếp tục bỏ phiếu.

5. Đi đến 1.
