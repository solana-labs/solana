---
title: Leader không có ngân hàng
---

Một leader không có ngân hàng thực hiện số lượng công việc tối thiểu để tạo ra một khối hợp lệ. Leaer được giao nhiệm vụ nhập các giao dịch, phân loại và lọc các giao dịch hợp lệ, sắp xếp chúng thành các mục nhập, shred các mục nhập và phát sóng các giao dịch đó. Trong khi validator chỉ cần tập hợp lại khối và phát lại việc thực thi các mục nhập đã được định hình tốt. Leader thực hiện các hoạt động bộ nhớ nhiều hơn gấp 3 lần trước khi thực hiện bất kỳ ngân hàng nào so với validator trên mỗi giao dịch được xử lý.

## Cơ sở lý luận

Hoạt động ngân hàng bình thường cho một khoản chi tiêu cần thực hiện 2 lần tải và 2 cửa hàng. Với thiết kế này leader chỉ cần thực hiện 1 tải. vì vậy account_db ít hơn gấp 4 lần hoạt động trước khi tạo khối. Các hoạt động của cửa hàng có thể sẽ đắt hơn so với việc đọc.

Khi giai đoạn phát lại bắt đầu xử lý các giao dịch giống nhau, nó có thể giả định rằng PoH là hợp lệ và tất cả các mục nhập đều an toàn để thực hiện song song. Các tài khoản phí đã được tải để tạo ra khối có khả năng vẫn còn trong bộ nhớ, vì vậy tải bổ sung sẽ ấm và chi phí có thể được khấu hao.

## Tài khoản phí

[tài khoản phí](../terminology.md#fee_account) thanh toán cho giao dịch được bao gồm trong khối. Leader chỉ cần xác thực tài khoản phí còn số dư để nộp phí.

## Cân bằng bộ nhớ đệm

Trong thời gian của các khối liên tiếp của các leader, leader duy trì một bộ nhớ đệm số dư tạm thời cho tất cả các tài khoản phí đã xử lý. Bộ nhớ đệm là một bản đồ của các pubkey đến các lamport.

Khi bắt đầu khối đầu tiên, bộ nhớ đệm có số dư trống. Vào cuối khối cuối cùng, bộ nhớ đệm bị phá hủy.

Các tra cứu số dư bộ nhớ đệm phải tham chiếu đến cùng một fork cơ sở trong toàn bộ thời gian của bộ nhớ đệm. Tại ranh giới khối, bộ nhớ đệm có thể được đặt lại cùng với fork cơ sở sau khi giai đoạn phát lại kết thúc xác minh khối trước đó.

## Kiểm tra số dư

Trước khi kiểm tra số dư, leader xác nhận tất cả các chữ ký trong giao dịch.

1. Xác minh tài khoản không được sử dụng và BlockHash hợp lệ.
2. Kiểm tra xem tài khoản phí có trong bộ đệm hay không hoặc tải tài khoản từ account_db và lưu trữ số dư của lamport trong bộ đệm.
3. Nếu số dư nhỏ hơn phí, hãy bỏ giao dịch.
4. Trừ phí khỏi số dư.
5. Đối với tất cả các khóa trong giao dịch là Ghi nợ-Tín dụng và được tham chiếu bởi một hướng dẫn, hãy giảm số dư của chúng xuống 0 trong bộ nhớ đệm. Phí tài khoản được khai báo là Ghi nợ-Tín dụng, miễn là phí này không được sử dụng trong bất kỳ hướng dẫn nào, số dư của tài khoản sẽ không giảm xuống 0.

## Phát lại Leader

Các leader sẽ cần phát lại các khối của họ như một phần của hoạt động giai đoạn phát lại tiêu chuẩn.

## Phát lại Leader với các khối liên tiếp

Một leader có thể được lên lịch để sản xuất nhiều khối liên tiếp. Trong kịch bản đó, leader có thể sẽ tạo ra khối tiếp theo trong khi giai đoạn phát lại cho khối đầu tiên đang diễn ra.

Khi leader kết thúc giai đoạn phát lại, nó có thể đặt lại bộ nhớ đệm cân bằng bằng cách xóa nó và đặt một fork mới làm cơ sở cho bộ nhớ đệm có thể hoạt động trên khối tiếp theo.

## Đặt lại số dư Bộ nhớ đệm

1. Khi bắt đầu khối, nếu bộ đệm ẩn số dư chưa được khởi tạo, hãy đặt fork cơ sở cho bộ đệm ẩn số dư là cha mẹ của khối và tạo bộ đệm trống.
2. nếu bộ nhớ đệm được khởi tạo, hãy kiểm tra xem cha mẹ của khối có một ngân hàng bị đóng băng mới hơn so với fork cơ sở hiện tại cho bộ nhớ đệm số dư hay không.
3. nếu tồn tại một nguồn gốc mới hơn fork cơ sở của bộ nhớ đệm, hãy đặt lại bộ nhớ đệm cho nguồn gốc.

## Tác động đến khách hàng

Tài khoản phí giống nhau có thể được sử dụng lại nhiều lần trong cùng một khối cho đến khi nó được sử dụng một lần dưới dạng Ghi nợ-Tín dụng theo một hướng dẫn.

Các khách hàng truyền một số lượng lớn giao dịch mỗi giây nên sử dụng một tài khoản phí chuyên dụng không được sử dụng như Ghi nợ-Tín dụng trong bất kỳ hướng dẫn nào.

Khi phí tài khoản được sử dụng làm Ghi nợ-Tín dụng, phí này sẽ không thể kiểm tra số dư cho đến khi bộ nhớ đệm ẩn số dư được đặt lại.
