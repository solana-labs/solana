---
title: Thế hệ Fork
---

Phần này mô tả cách các fork xảy ra một cách tự nhiên do [luân chuyển leader](leader-rotation.md).

## Tổng quát

Các nút thay phiên nhau trở thành leader và tạo ra PoH mã hóa các thay đổi trạng thái. Cụm có thể chịu đựng sự mất kết nối với bất kỳ leader nào bằng cách tổng hợp những gì leader _**sẽ**_ tạo ra nếu nó được kết nối nhưng không nhập vào bất kỳ thay đổi trạng thái nào. Do đó, số lượng fork có thể được giới hạn trong danh sách bỏ qua "ở đó/không ở đó" có thể phát sinh trên ranh giới slot luân chuyển leader. Tại bất kỳ slot nhất định nào, chỉ một giao dịch của một leader duy nhất được chấp nhận.

## Luồng tin nhắn

1. Các giao dịch được nhập bởi leader hiện tại.
2. Leader lọc các giao dịch hợp lệ.
3. Leader thực hiện các giao dịch hợp lệ cập nhật trạng thái của nó.
4. Leader đóng gói các giao dịch thành các mục dựa trên slot PoH hiện tại của nó.
5. Leader truyền các mục nhập đến các node validator \(trong các shred đã ký\)
   1. Luồng PoH bao gồm các tick; các mục trống cho biết khả năng hoạt động của leader và thời gian trôi qua trên cụm.
   2. Luồng của leader bắt đầu với các mục đánh dấu cần thiết để hoàn thành PoH trở lại leader slot trước đó được quan sát gần đây nhất của leader.
6. Các validator truyền lại các mục nhập đến các đồng nghiệp trong tập hợp của chúng và tới các node hạ lưu xa hơn.
7. Các validator xác thực các giao dịch và thực hiện chúng trên trạng thái của chúng.
8. Trình xác thực tính toán hàm băm của trạng thái.
9. Vào những thời điểm cụ thể, tức là số lần đánh dấu PoH cụ thể, các validator sẽ truyền phiếu bầu cho leader.
   1. Phiếu bầu là chữ ký của hàm băm của trạng thái được tính toán tại số lần đánh dấu PoH đó.
   2. Phiếu bầu cũng được tuyên truyền thông qua gossip.
10. Leader thực hiện các phiếu bầu, giống như bất kỳ giao dịch nào khác và phát chúng đến cụm.
11. Các validator quan sát các phiếu bầu của họ và tất cả các phiếu bầu từ cụm.

## Phân vùng, Fork

Các fork có thể phát sinh ở số lần đánh dấu PoH tương ứng với một phiếu bầu. Leader tiếp theo có thể đã không quan sát vùng bỏ phiếu cuối cùng và có thể bắt đầu vùng của họ bằng các mục PoH ảo đã tạo. Các đánh dấu trống này được tạo ra bởi tất cả các node trong cụm với tốc độ được định cấu hình theo cụm cho các hàm băm/mỗi/đánh dấu `Z`.

Chỉ có hai phiên bản có thể có của PoH trong thời gian biểu quyết: PoH với các đánh dấu `T` và mục nhập được tạo bởi leader hiện tại hoặc PoH chỉ bằng các đánh dấu. Phiên bản "chỉ các đánh dấu" của PoH có thể được coi là một sổ cái ảo, một sổ cái mà tất cả các node trong cụm có thể thu được từ đánh dấu cuối cùng trong slot trước đó.

Các validator có thể bỏ qua fork ở những điểm khác \(ví dụ: từ leader sai\), hoặc gạch chéo leader chịu trách nhiệm về đợt fork.

Các validator bỏ phiếu dựa trên sự lựa chọn tham lam để tối đa hóa phần thưởng của họ được mô tả trong [Tower BFT](../implemented-proposals/tower-bft.md).

### Chế độ xem của validator

#### Tiến trình thời gian

Biểu đồ bên dưới thể hiện chế độ xem của một validator về luồng PoH với các fork có thể có theo thời gian. L1, L2, v. v. là các slot leader và `E` đại diện cho các mục nhập từ leader đó trong leader slot đó. Các `x` chỉ đại diện cho các đánh dấu, và thời gian đi xuống trong biểu đồ.

![Thế hệ Fork](/img/fork-generation.svg)

Lưu ý rằng một `E` xuất hiện trên 2 fork tại cùng một slot là điều kiện có thể tháo rời, vì vậy validator quan sát `E3` và `E3'` có thể slash L3 và chọn `x` một cách an toàn cho slot đó. Sau khi validator cam kết đến một fork, các fork khác có thể bị loại bỏ dưới số lần đánh dấu đó. Đối với bất kỳ slot nào, các validator chỉ cần xem xét một chuỗi "có các mục nhập" hoặc chuỗi "chỉ các đánh dấu" được đề xuất bởi một leader. Nhưng nhiều mục nhập ảo có thể chồng chéo lên nhau khi chúng liên kết trở lại slot trước đó.

#### Phân chia thời gian

Sẽ rất hữu ích khi coi việc luân chuyển leader qua số lần tick PoH như là sự phân chia thời gian của công việc mã hóa trạng thái cho cụm. Bảng sau đây trình bày cây của các fork ở trên dưới dạng một sổ cái phân chia theo thời gian.

| leader slot           | L1  | L2  | L3  | L4  | L5  |
| :-------------------- | :-- | :-- | :-- | :-- | :-- |
| dữ liệu               | E1  | E2  | E3  | E4  | E5  |
| các tick trở về trước |     |     |     | x   | xx  |

Lưu ý rằng chỉ dữ liệu từ leader L3 mới được chấp nhận trong leader slot L3. Dữ liệu từ L3 có thể bao gồm các tick "bắt kịp" trở lại một slot khác ngoài L2 nếu L3 không quan sát dữ liệu của L2. Truyền của L4 và L5 bao gồm các mục PoH "các tick để chiếm ưu thế".

Sự sắp xếp các luồng dữ liệu mạng này cho phép các node lưu chính xác thông tin này vào sổ cái để phát lại, khởi động lại và các điểm kiểm tra.

### Chế độ xem của Leader

Khi một leader mới bắt đầu một slot, trước tiên người đó phải truyền bất kỳ PoH \(các tick\) nào cần thiết để liên kết slot mới với slot được quan sát và bình chọn gần đây nhất. Fork mà leader đề xuất sẽ liên kết slot hiện tại với một fork trước đó mà leader đã bỏ phiếu bằng các đánh dấu ảo.
