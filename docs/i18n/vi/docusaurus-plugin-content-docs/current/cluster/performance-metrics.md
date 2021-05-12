---
title: Chỉ số Hiệu suất
---

Hiệu suất cụm Solana được đo bằng số lượng giao dịch trung bình mỗi giây mà mạng có thể duy trì \(TPS\). Và, mất bao lâu để một giao dịch được xác nhận bởi phần lớn cụm \(Thời gian xác nhận\).

Mỗi nút cụm duy trì các bộ đếm khác nhau được tăng lên trên các sự kiện nhất định. Các bộ đếm này được tải lên định kỳ lên cơ sở dữ liệu dựa trên đám mây. Bảng điều khiển chỉ số của Solana tìm nạp các bộ đếm này và tính toán các chỉ số hiệu suất và hiển thị nó trên trang tổng quan.

## TPS

Thời gian chạy ngân hàng của mỗi node duy trì số lượng giao dịch mà nó đã xử lý. Trước tiên, bảng điều khiển sẽ tính toán số lượng giao dịch trung bình trên tất cả các node được kích hoạt chỉ số trong cụm. Sau đó, số lượng giao dịch cụm trung bình được tính trung bình trong khoảng thời gian 2 giây và được hiển thị trong biểu đồ chuỗi thời gian TPS. Trang tổng quan cũng hiển thị thống kê TPS trung bình, TPS tối đa và Tổng số giao dịch, tất cả đều được tính toán từ số lượng giao dịch trung bình.

## Thời gian xác nhận

Mỗi node validator duy trì một danh sách các fork sổ cái đang hoạt động hiển thị cho node. Một fork được coi là bị đóng băng khi nút đã nhận và xử lý tất cả các mục nhập tương ứng với fork. Một fork được coi là được xác nhận khi nó nhận được số phiếu siêu đa số tích lũy và khi một trong những fork con của nó bị đóng băng.

Node chỉ định dấu thời gian cho mỗi lần fork mới và tính thời gian cần thiết để xác nhận đợt fork. Thời gian này được phản ánh là thời gian xác nhận validator trong chỉ số hiệu suất. Bảng điều khiển hiệu suất hiển thị mức trung bình của thời gian xác nhận của các node validator dưới dạng biểu đồ chuỗi thời gian.

## Thiết lập Phần cứng

Phần mềm xác thực được triển khai cho các phiên bản GCP n1-standard-16 với ổ đĩa cứng pd-ssd 1TB và GPU Nvidia V100 2x. Chúng được triển khai ở khu vực phía tây-1 của chúng tôi.

solana-bench-tps được khởi động sau khi mạng hội tụ từ một máy khách có phiên bản chỉ CPU n1-standard-16 với các đối số sau: `--tx\_count=50000 --thread-batch-sleep 1000`

TPS và các chỉ số xác nhận được ghi lại từ các con số trên bảng điều khiển trong 5 phút trung bình khi giai đoạn chuyển đổi dự phòng bắt đầu.
