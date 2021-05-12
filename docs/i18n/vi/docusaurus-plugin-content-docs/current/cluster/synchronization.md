---
title: Đồng bộ hóa
---

Đồng bộ hóa nhanh chóng, đáng tin cậy là lý do lớn nhất khiến Solana có thể đạt được thông lượng cao như vậy. Blockchain truyền thống đồng bộ hóa trên các khối giao dịch lớn được gọi là khối. Bằng cách đồng bộ hóa trên các khối, một giao dịch không thể được xử lý cho đến khi một khoảng thời gian, được gọi là "block time", đã trôi qua. Trong sự đồng thuận của Proof of Work, thời gian khối này cần phải rất lớn \(~10 phút\) để giảm thiểu tỷ lệ cược của nhiều validator tạo ra một khối hợp lệ mới cùng một lúc. Không có ràng buộc nào như vậy trong sự đồng thuận Proof of Stake, nhưng nếu không có dấu thời gian đáng tin cậy, validator không thể xác định thứ tự của các khối đến. Cách giải quyết phổ biến là gắn thẻ mỗi khối bằng [wallclock timestamp](https://en.bitcoin.it/wiki/Block_timestamp). Do độ trễ của đồng hồ và sự khác biệt về độ trễ mạng, dấu thời gian chỉ chính xác trong vòng một hoặc hai giờ. Để giải quyết giải pháp, các hệ thống này kéo dài thời gian khối để cung cấp sự chắc chắn hợp lý rằng dấu thời gian trung bình trên mỗi khối luôn tăng.

Solana có một cách tiếp cận rất khác, nó gọi là _Proof of History_ hoặc _PoH_. Các khối "timestamp" của các node leader với các bằng chứng mật mã mà một số khoảng thời gian đã trôi qua kể từ lần chứng minh cuối cùng. Tất cả dữ liệu được băm thành bằng chứng chắc chắn nhất đã xảy ra trước khi bằng chứng được tạo. Sau đó, node chia sẻ khối mới với các node validator, có thể xác minh các bằng chứng đó. Các khối có thể đến các validator theo bất kỳ thứ tự nào hoặc thậm chí có thể được phát lại nhiều năm sau đó. Với các đảm bảo đồng bộ hóa đáng tin cậy như vậy, Solana có thể chia khối thành các lô giao dịch nhỏ hơn được gọi là _entries_. Các điểm nhập được truyền trực tuyến tới các validator trong thời gian thực, trước bất kỳ khái niệm nào về sự đồng thuận của khối.

Về mặt kỹ thuật, Solana không bao giờ gửi một _khối_, nhưng sử dụng thuật ngữ này để mô tả chuỗi các điểm nhập mà các validator bỏ phiếu để đạt được _xác nhận_. Bằng cách đó, thời gian xác nhận của Solana có thể được so sánh giữa quả táo với quả táo với các hệ thống dựa trên khối. Việc triển khai hiện tại đặt thời gian khối thành 800ms.

Điều đang xảy ra là các mục nhập được truyền trực tuyến tới các validator nhanh chóng như một node dẫn đầu có thể đưa một tập hợp các giao dịch hợp lệ vào một mục nhập. Các validator xử lý những mục nhập đó rất lâu trước khi đến lúc bỏ phiếu về tính hợp lệ của chúng. Bằng cách xử lý các giao dịch một cách lạc quan, không có sự chậm trễ nào giữa thời điểm nhận được mục nhập cuối cùng và thời điểm node có thể bỏ phiếu. Trong trường hợp **không** đạt được sự đồng thuận, một node chỉ cần khôi phục trạng thái của nó. Kỹ thuật xử lý tối ưu này được giới thiệu vào năm 1981 và được gọi là [Optimistic Concurrency Control](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.65.4735). Nó có thể được áp dụng cho kiến ​​trúc blockchain trong đó một cụm bỏ phiếu trên một hàm băm đại diện cho sổ cái đầy đủ lên đến một số _chiều cao khối_. Trong Solana, nó được thực hiện một cách bình thường bằng cách sử dụng hàm băm PoH của mục nhập cuối cùng.

## Mối quan hệ với VDFs

Kỹ thuật Proof of History lần đầu tiên được Solana mô tả để sử dụng trong blockchain vào tháng 11 năm 2017. Vào tháng 6 năm sau, một kỹ thuật tương tự đã được mô tả tại Stanford và được gọi là [verifiable delay function](https://eprint.iacr.org/2018/601.pdf) hoặc _VDF_.

Đặc tính mong muốn của VDF là thời gian xác minh rất nhanh. Cách tiếp cận của Solana để kiểm tra chức năng trì hoãn của nó tỷ lệ thuận với thời gian tạo ra nó. Được chia thành GPU 4000 lõi, nó đủ nhanh cho nhu cầu của Solana, nhưng nếu bạn hỏi các tác giả của bài báo được trích dẫn ở trên, họ có thể cho bạn biết \([là có](https://github.com/solana-labs/solana/issues/388)\) rằng cách tiếp cận của Solana chậm về mặt thuật toán và nó không nên được gọi là VDF. Chúng tôi lập luận rằng thuật ngữ VDF nên đại diện cho danh mục các hàm trễ có thể xác minh được và không chỉ là tập hợp con với các đặc tính hiệu suất nhất định. Cho đến khi điều đó được giải quyết, Solana có thể sẽ tiếp tục sử dụng thuật ngữ PoH cho VDF dành riêng cho ứng dụng của nó.

Một sự khác biệt khác giữa PoH và VDF là VDF chỉ được sử dụng cho thời lượng theo dõi. Mặt khác, chuỗi băm của PoH bao gồm các hàm băm của bất kỳ dữ liệu nào mà ứng dụng quan sát được. Dữ liệu đó là một con dao hai lưỡi. Một mặt, dữ liệu "chứng minh lịch sử" - rằng dữ liệu chắc chắn tồn tại trước các hàm băm sau nó. Mặt khác, điều đó có nghĩa là ứng dụng có thể thao tác chuỗi băm bằng cách thay đổi _khi_ dữ liệu được băm. Do đó, chuỗi PoH không đóng vai trò là một nguồn ngẫu nhiên tốt trong khi VDF không có dữ liệu đó có thể. Ví dụ, [thuật toán vòng xoay leader](synchronization.md#leader-rotation) của Solana chỉ bắt nguồn từ ​​_độ cao_ VDF chứ không phải hàm băm của nó ở độ cao đó.

## Mối quan hệ với Cơ chế đồng thuận

Proof of History không phải là một cơ chế đồng thuận, nhưng nó được sử dụng để cải thiện hiệu suất của sự đồng thuận Proof of Stake của Solana. Nó cũng được sử dụng để cải thiện hiệu suất của các giao thức mặt phẳng dữ liệu.

## Tìm hiểu thêm về Proof of History

- [tương tự đồng hồ nước](https://medium.com/solana-labs/proof-of-history-explained-by-a-water-clock-e682183417b8)
- [Tổng quan về Proof of History](https://medium.com/solana-labs/proof-of-history-a-clock-for-blockchain-cf47a61a9274)
