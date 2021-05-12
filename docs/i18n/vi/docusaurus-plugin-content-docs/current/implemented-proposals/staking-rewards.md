---
title: Phần thưởng Staking
---

Proof of Stake \(PoS\), \(tức là sử dụng tài sản trong giao thức, SOL, để cung cấp sự đồng thuận an toàn) được nêu ở đây. Solana triển khai một chương trình bảo mật/phần thưởng bằng chứng stake cho các node validator trong cụm. Mục tiêu gấp ba lần:

- Điều chỉnh các ưu đãi của validator với ưu đãi của cụm lớn hơn thông qua

  tiền gửi trong trò chơi có rủi ro

- Tránh các vấn đề 'không có tại stake' về bỏ phiếu fork bằng cách thực hiện các quy tắc slashing

  nhằm thúc đẩy sự hội tụ fork

- Cung cấp một con đường cho phần thưởng validator được cung cấp như một chức năng của validator

  tham gia vào cụm.

Trong khi nhiều chi tiết của việc triển khai cụ thể hiện đang được xem xét và dự kiến ​​sẽ được tập trung thông qua các nghiên cứu mô hình cụ thể và thăm dò thông số trên Solana testnet, chúng tôi phác thảo ở đây suy nghĩ hiện tại của chúng tôi về các thành phần chính của hệ thống PoS. Phần lớn suy nghĩ này dựa trên tình trạng hiện tại của Casper FFG, với các tối ưu hóa và các thuộc tính cụ thể sẽ được sửa đổi theo sự cho phép của cấu trúc dữ liệu Proof of History \(PoH\) của blockchain Solana.

## Tổng quan chung

Thiết kế xác thực sổ cái của Solana dựa trên các giao dịch phát sóng của leader được lựa chọn xoay vòng, có tỉ trọng stake trong cấu trúc dữ liệu PoH tới các node xác thực. Các node này, khi nhận được quảng bá của các leader, có cơ hội bỏ phiếu về trạng thái hiện tại và chiều cao PoH bằng cách ký một giao dịch vào luồng PoH.

Để trở thành validator Solana, một người phải deposit/lock-up một số lượng SOL trong hợp đồng. SOL này sẽ không thể truy cập được trong một khoảng thời gian cụ thể. Khoảng thời gian chính xác của khoảng thời gian khóa staking vẫn chưa được xác định. Tuy nhiên, chúng ta có thể xem xét ba giai đoạn của thời gian này mà các thông số cụ thể sẽ cần thiết:

- _Giai đoạn khởi động_: SOL được ký gửi và không thể truy cập vào node,

  tuy nhiên xác thực giao dịch PoH chưa bắt đầu. Theo thứ tự của

  ngày đến tuần

- _Thời gian xác thực_: thời hạn tối thiểu mà SOL đã ký gửi sẽ là

  không thể tiếp cận, có nguy cơ bị slashing \(xem quy tắc slashing bên dưới \) và kiếm tiền

  phần thưởng cho sự tham gia của validator. Khoảng thời gian có thể vài tháng đến một

  năm.

- _Giai đoạn hạ nhiệt_: khoảng thời gian sau khi nộp một

  giao dịch 'rút tiền'. Trong khoảng thời gian này, trách nhiệm xác nhận đã

  được xóa bỏ và không thể truy cập được tiền. Phần thưởng tích lũy

  sẽ được giao vào cuối giai đoạn này, cùng với sự trở lại của

  tiền gửi ban đầu.

Không tin cậy của Solana về thời gian và thứ tự được cung cấp bởi cấu trúc dữ liệu PoH của nó, cùng với thiết kế truyền và truyền dữ liệu [turbine](https://www.youtube.com/watch?v=qt_gDRXHrHQ&t=1s) của nó, nên cung cấp thời gian xác nhận giao dịch phụ thứ hai với quy mô đó với nhật ký số lượng node trong cụm. Điều này có nghĩa là chúng ta không cần phải hạn chế số lượng các node xác thực với 'khoản tiền gửi tối thiểu' bị cấm và hy vọng các node có thể trở thành các validator với số lượng SOL danh nghĩa được stake. Đồng thời, việc Solana tập trung vào thông lượng-cao sẽ tạo ra động lực cho các khách hàng xác thực để cung cấp phần cứng hiệu suất-cao và đáng tin cậy. Kết hợp với ngưỡng tốc độ mạng tối thiểu tiềm năng để tham gia với tư cách là khách hàng-xác thực, chúng tôi hy vọng sẽ xuất hiện một thị trường ủy quyền xác thực lành mạnh. Để đạt được mục tiêu này, testnet của Solana sẽ dẫn đến một cuộc thi khách hàng-xác thực "Tour de SOL", tập trung vào thông lượng và thời gian hoạt động để xếp hạng và thưởng cho những validator testnet.

## Các hình phạt

Như đã thảo luận trong phần [Thiết Kế Kinh Tế](ed_overview/ed_overview.md), lãi suất validator hàng năm phải được chỉ định dưới dạng hàm của tổng phần trăm nguồn cung luân chuyển đã được stake. Cụm thưởng cho những validator trực tuyến và tích cực tham gia vào quá trình xác thực trong suốt _thời gian xác thực _ của họ. Đối với những validator hoạt động ngoại tuyến/không xác thực được các giao dịch trong thời gian này, phần thưởng hàng năm của họ sẽ bị giảm đi.

Tương tự, chúng tôi có thể xem xét việc giảm theo thuật toán trong số tiền stake đang hoạt động của validator trong trường hợp họ ngoại tuyến. Tức là. nếu một validator không hoạt động trong một khoảng thời gian, do phân vùng hoặc do cách khác, số tiền stake 'đang hoạt động' của họ \(đủ điều kiện để kiếm phần thưởng\) có thể bị giảm. Thiết kế này sẽ được cấu trúc để giúp các phân vùng tồn tại lâu dài cuối cùng đạt được tính cuối cùng trên các chuỗi tương ứng của chúng vì % tổng số cổ phần không có quyền biểu quyết sẽ giảm theo thời gian cho đến khi các validator tích cực có thể đạt được mức siêu lớn trong mỗi phân vùng. Tương tự như vậy, khi tương tác lại, số tiền stake "hoạt động" sẽ trực tuyến trở lại với một số tỷ lệ xác định. Các tỷ lệ giảm tiền stake khác nhau có thể được xem xét tùy thuộc vào kích thước của phân vùng/tập hợp hoạt động.
