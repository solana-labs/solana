# RiP Curl: RPC định hướng-giao dịch, độ trễ-thấp

## Sự cố

Việc triển khai RPC ban đầu của Solana được tạo ra với mục đích cho phép người dùng xác nhận các giao dịch vừa mới được gửi đến cụm. Nó được thiết kế với mục đích sử dụng bộ nhớ sao cho bất kỳ validator nào cũng có thể hỗ trợ API mà không cần quan tâm đến các cuộc tấn công DoS.

Sau đó, mong muốn sử dụng cùng một API đó để hỗ trợ trình khám phá Solana. Thiết kế ban đầu chỉ hỗ trợ số phút của lịch sử, vì vậy chúng tôi đã thay đổi nó để thay vào đó lưu trữ trạng thái giao dịch trong phiên bản RocksDB cục bộ và cung cấp số ngày của lịch sử. Sau đó, chúng tôi đã kéo dài thời hạn đó lên 6 tháng thông qua BigTable.

Với mỗi lần sửa đổi, API trở nên phù hợp hơn với các ứng dụng cung cấp nội dung tĩnh và ít hấp dẫn hơn để xử lý giao dịch. Các khách hàng thăm dò tình trạng giao dịch thay vì được thông báo, tạo ấn tượng sai về thời gian xác nhận cao hơn. Hơn nữa, những gì khách hàng có thể thăm dò ý kiến ​​bị hạn chế, ngăn họ đưa ra các quyết định hợp lý trong thời gian thực, chẳng hạn như công nhận một giao dịch được xác nhận ngay khi cụ thể, những validator đáng tin cậy bỏ phiếu cho nó.

## Giải pháp đề xuất

Một API phát trực tuyến, hướng đến giao dịch, thân thiện với web được xây dựng xung quanh ReplayStage của validator.

Cải thiện trải nghiệm khách hàng:

* Hỗ trợ kết nối trực tiếp từ các ứng dụng WebAssembly.
* Khách hàng có thể được thông báo về tiến độ xác nhận trong thời gian-thực, bao gồm số phiếu bầu và tỷ trọng stake của cử tri.
* Các khách hàng có thể được thông báo khi đợt fork nặng nề nhất thay đổi, nếu nó ảnh hưởng đến số lượng xác nhận giao dịch.

Dễ dàng hơn cho những validator hỗ trợ:

* Mỗi validstor hỗ trợ một số kết nối đồng thời và không có ràng buộc tài nguyên đáng kể nào.
* Trạng thái giao dịch không bao giờ được lưu trữ trong bộ nhớ và không thể được thăm dò.
* Chữ ký chỉ được lưu trữ trong bộ nhớ cho đến khi mức cam kết mong muốn hoặc cho đến khi blockhash hết hạn, sẽ xảy ra sau này.

Làm thế nào nó hoạt động:

1. Khách hàng kết nối với validator bằng kênh giao tiếp đáng tin cậy, chẳng hạn như ổ cắm web.
2. Validator đăng ký chữ ký với ReplayStage.
3. Validator gửi giao dịch vào Gulf Stream và thử lại tất cả các fork đã biết cho đến khi blockhash hết hạn (cho đến khi giao dịch được chấp nhận chỉ trên fork nặng nệ nhất). Nếu blockhash hết hạn, chữ ký không được đăng ký, khách hàng được thông báo và kết nối bị đóng.
4. Khi ReplayStage phát hiện các sự kiện ảnh hưởng đến trạng thái của giao dịch, nó sẽ thông báo cho khách hàng trong thời gian thực.
5. Sau khi xác nhận rằng giao dịch được root (`CommitmentLevel::Max`), chữ ký không được đăng ký và máy chủ đóng kênh ngược tuyến.
