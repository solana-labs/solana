---
title: Giới thiệu
---

## Solana là gì?

Solana là một dự án mã nguồn mở triển khai một blockchain mới, hiệu suất cao, không cần sự cho phép. Solana Foundation có trụ sở tại Geneva, Thụy Sĩ và duy trì dự án nguồn mở.

## Tại sao lại là Solana?

Cơ sở dữ liệu tập trung có thể xử lý 710,000 giao dịch mỗi giây trên mạng gigabit nếu các giao dịch trung bình không quá 176 byte. Cơ sở dữ liệu tập trung cũng có thể tự tái tạo và duy trì tính sẵn sàng cao mà không ảnh hưởng đáng kể đến tỷ lệ giao dịch bằng cách sử dụng kỹ thuật hệ thống phân tán được gọi là Optimistic Concurrency Control [\[H.T.Kung, J.T.Robinson (1981)\]](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.65.4735). Tại Solana, chúng tôi đang chứng minh rằng những giới hạn lý thuyết tương tự này cũng áp dụng cho blockchain trên một mạng đối địch. Thành phần chính? Tìm cách chia sẻ thời gian khi các node không thể tin cậy lẫn nhau. Một khi các node có thể tin tưởng vào thời gian, ~ 40 năm nghiên cứu hệ thống phân tán được áp dụng cho blockchain!

> Có lẽ sự khác biệt nổi bật nhất giữa các thuật toán thu được bằng phương pháp của chúng tôi và các thuật toán dựa trên thời gian chờ là việc sử dụng thời gian chờ tạo ra một thuật toán phân tán truyền thống trong đó các quy trình hoạt động không đồng bộ, trong khi phương pháp của chúng tôi tạo ra một thuật toán đồng bộ toàn cầu, trong đó mọi quy trình thực hiện cùng một việc tại (xấp xỉ) cùng một lúc. Phương pháp của chúng tôi dường như mâu thuẫn với toàn bộ mục đích của xử lý phân tán, đó là cho phép các quy trình khác nhau hoạt động độc lập và thực hiện các chức năng khác nhau. Tuy nhiên, nếu một hệ thống phân tán thực sự là một hệ thống đơn lẻ, thì các quy trình phải được đồng bộ theo một cách nào đó. Về mặt khái niệm, cách dễ nhất để đồng bộ hóa các quy trình là khiến tất cả chúng làm cùng một việc cùng một lúc. Do đó, phương pháp của chúng tôi được sử dụng để triển khai một hạt nhân thực hiện đồng bộ hóa cần thiết - ví dụ: đảm bảo rằng hai quy trình khác nhau không cố gắng sửa đổi tệp cùng một lúc. Các quá trình có thể chỉ dành một phần nhỏ thời gian của chúng để thực thi hạt nhân đồng bộ hóa; thời gian còn lại, chúng có thể hoạt động độc lập - ví dụ: truy cập các tệp khác nhau. Đây là một cách tiếp cận mà chúng tôi đã ủng hộ ngay cả khi không yêu cầu khả năng chịu lỗi. Sự đơn giản cơ bản của phương pháp giúp dễ dàng hiểu các thuộc tính chính xác của hệ thống, điều này rất quan trọng nếu người ta muốn biết hệ thống có khả năng chịu lỗi như thế nào. [\[L.Lamport (1984)\]](http://citeseerx.ist.psu.edu/viewdoc/summary?doi=10.1.1.71.1078)

Hơn nữa, và chúng tôi rất ngạc nhiên, nó có thể được thực hiện bằng cách sử dụng một cơ chế đã tồn tại trong Bitcoin kể từ ngày đầu tiên. Tính năng Bitcoin được gọi là nLocktime và nó có thể được sử dụng để cập nhật các giao dịch bằng cách sử dụng chiều cao khối thay vì dấu thời gian. Là khách hàng Bitcoin, bạn sẽ sử dụng chiều cao khối thay vì dấu thời gian nếu bạn không tin tưởng vào mạng. Chiều cao khối hóa ra là một ví dụ của cái được gọi là Hàm trì hoãn có thể xác minh được trong vòng tròn mật mã. Đó là một cách an toàn mật mã để nói rằng thời gian đã trôi qua. Trong Solana, chúng tôi sử dụng hàm trì hoãn có thể xác minh chi tiết hơn, chuỗi băm SHA 256, để kiểm tra sổ cái và điều phối sự đồng thuận. Với nó, chúng tôi triển khai Optimistic Concurrency Control và hiện đang trên đường hướng tới giới hạn lý thuyết là 710,000 giao dịch mỗi giây.

## Tổng quan về tài liệu

Tài liệu Solana mô tả dự án mã nguồn mở Solana, một blockchain được xây dựng từ đầu cho quy mô. Họ đề cập đến lý do tại sao Solana hữu ích, cách sử dụng, cách hoạt động và tại sao nó sẽ tiếp tục hoạt động lâu dài sau khi công ty Solana đóng cửa. Mục tiêu của kiến ​​trúc Solana là để chứng minh rằng tồn tại một tập hợp các thuật toán phần mềm khi được sử dụng kết hợp để triển khai một blockchain, loại bỏ phần mềm làm tắc nghẽn hiệu suất, cho phép thông lượng giao dịch tăng tỷ lệ thuận với băng thông mạng. Kiến trúc đáp ứng ba thuộc tính mong muốn của một blockchain: nó có thể mở rộng, an toàn và phi tập trung.

Kiến trúc mô tả giới hạn trên lý thuyết là 710 nghìn giao dịch mỗi giây \(tps\) trên mạng gigabit tiêu chuẩn và 28,4 triệu tps trên 40 gigabit. Hơn nữa, kiến trúc hỗ trợ thực hiện an toàn, đồng thời các chương trình được tạo bằng các ngôn ngữ lập trình phổ biến như C hoặc Rust.

## Cụm Solana là gì?

Cụm là một tập hợp các máy tính hoạt động cùng nhau và có thể được nhìn từ bên ngoài như một hệ thống duy nhất. Cụm Solana là một tập hợp các máy tính độc lập làm việc cùng nhau \(và đôi khi chống lại nhau\) để xác minh đầu ra của các chương trình không đáng tin cậy, do người dùng gửi. Một cụm Solana có thể được sử dụng bất kỳ lúc nào người dùng muốn lưu giữ một bản ghi bất biến về các sự kiện theo thời gian hoặc các diễn giải có lập trình về các sự kiện đó. Công dụng là theo dõi máy tính nào đã làm việc có ý nghĩa để giữ cho cụm hoạt động. Một công dụng khác có thể là theo dõi việc sở hữu các tài sản trong thế giới thực. Trong mỗi trường hợp, cụm tạo ra một bản ghi các sự kiện được gọi là sổ cái. Nó sẽ được bảo tồn trong suốt thời gian tồn tại của cụm. Miễn là ai đó ở đâu đó trên thế giới còn lưu giữ một bản sao của sổ cái, thì kết quả của các chương trình của nó /(có thể chứa hồ sơ về việc ai sở hữu/) sẽ mãi mãi có thể được sao chép, không phụ thuộc vào tổ chức đã phát động nó.

## SOL là gì?

SOL là tên của mã thông báo gốc của Solana, có thể được chuyển tới các node trong một cụm Solana để đổi lấy việc chạy một chương trình trên chuỗi hoặc xác thực đầu ra của nó. Hệ thống có thể thực hiện các khoản thanh toán vi mô của các phân đoạn SOL, được gọi là _lamports_. Chúng được đặt tên để vinh danh người có ảnh hưởng kỹ thuật lớn nhất của Solana, [Leslie Lamport](https://en.wikipedia.org/wiki/Leslie_Lamport). Một lamport có giá trị là 0,000000001 SOL.

## Từ chối trách nhiệm

Tất cả các tuyên bố, nội dung, thiết kế, thuật toán, ước tính, lộ trình, thông số kỹ thuật và phép đo hiệu suất được mô tả trong dự án này đều được thực hiện với nỗ lực cao nhất của tác giả. Người đọc phải kiểm tra và xác nhận tính chính xác và trung thực của chúng. Hơn nữa, không có gì trong dự án này tạo thành một sự mời gọi đầu tư.
