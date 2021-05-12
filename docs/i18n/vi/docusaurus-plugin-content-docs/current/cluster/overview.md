---
title: Cụm Solana
---

Cụm Solana là một tập hợp các validator làm việc cùng nhau để phục vụ các giao dịch của khách hàng và duy trì tính toàn vẹn của sổ cái. Nhiều cụm có thể cùng tồn tại. Khi hai cụm chia sẻ một khối genesis chung, chúng cố gắng hội tụ. Nếu không, họ chỉ đơn giản là bỏ qua sự tồn tại của người kia. Các giao dịch được gửi đến sai một cách lặng lẽ bị từ chối. Trong phần này, chúng ta sẽ thảo luận về cách một cụm được tạo ra, cách các node tham gia vào cụm, cách chúng chia sẻ sổ cái, cách chúng đảm bảo sổ cái được sao chép và cách chúng đối phó với các node lỗi và độc hại.

## Tạo Một Cụm

Trước khi bắt đầu bất kỳ validator nào, trước tiên ta cần tạo _genesis config_. Cấu hình tham chiếu đến hai public key, một _mint_ và một _bootstrap validator_. Validator giữ private key riêng của validator bootstrap chịu trách nhiệm bổ sung các mục nhập đầu tiên vào sổ cái. Nó khởi tạo trạng thái bên trong bằng tài khoản của người đúc. Tài khoản đó sẽ giữ số lượng mã thông báo gốc được cấu hình genesis xác định. Sau đó, validator thứ hai liên hệ với validator bootstrap để đăng ký làm _validator_. Các validator bổ sung sau đó đăng ký với bất kỳ thành viên đã đăng ký nào của cụm.

Validator nhận tất cả các bài dự thi từ người đứng đầu và gửi phiếu xác nhận các bài dự thi đó là hợp lệ. Sau khi bỏ phiếu, validator sẽ lưu trữ các mục nhập đó. Khi validator nhận thấy có đủ số lượng bản sao tồn tại, nó sẽ xóa bản sao của nó.

## Tham gia một Cụm

Các validator tham gia cụm thông qua tin nhắn đăng ký được gửi đến _control plane_ của nó. Mặt phẳng điều khiển được triển khai bằng giao thức _gossip_, có nghĩa là một node có thể đăng ký với bất kỳ node hiện có nào và mong đợi đăng ký của nó sẽ truyền đến tất cả các node trong cụm. Thời gian cần thiết để tất cả các node đồng bộ hóa tỷ lệ với bình phương của số lượng node tham gia vào cụm. Về mặt thuật toán, điều đó được coi là rất chậm, nhưng đổi lại thời gian đó, một node được đảm bảo rằng cuối cùng nó có tất cả thông tin giống như mọi node khác và thông tin đó không thể bị kiểm duyệt bởi bất kỳ một node nào.

## Gửi các giao dịch đến Cụm

Khách hàng gửi giao dịch đến bất kỳ cổng Đơn vị xử lý giao dịch \(TPU\) của validator. Nếu node ở vai trò validator, nó sẽ chuyển tiếp giao dịch đến leader được chỉ định. Nếu ở vai trò leader, node sẽ nhóm các giao dịch đến, đánh dấu thời gian chúng tạo _entry_ và đẩy chúng lên _data plane_ của cụm. Khi ở trên bình diện dữ liệu, các giao dịch được xác thực bởi các node validator, gắn chúng vào sổ cái một cách hiệu quả.

## Xác nhận Giao dịch

Một cụm Solana có khả năng _confirmation_ dưới giây cho tối đa 150 nút với kế hoạch mở rộng quy mô lên đến hàng trăm nghìn node. Sau khi được triển khai đầy đủ, thời gian xác nhận dự kiến ​​sẽ chỉ tăng lên với logarithm của số lượng validator, trong đó cơ số của các logarithm rất cao. Ví dụ: nếu cơ sở là một nghìn, điều đó có nghĩa là đối với nghìn node đầu tiên, xác nhận sẽ là khoảng thời gian của ba bước nhảy mạng cộng với thời gian để validator chậm nhất trong đa số bỏ phiếu. Đối với một triệu node tiếp theo, xác nhận chỉ tăng một bước mạng.

Solana định nghĩa xác nhận là khoảng thời gian kể từ khi leader đánh dấu thời gian cho một mục nhập mới cho đến thời điểm nó nhận ra phần lớn các phiếu bầu trên sổ cái.

Mạng gossip quá chậm để đạt được xác nhận giây sau khi mạng phát triển vượt quá một kích thước nhất định. Thời gian cần thiết để gửi thông điệp đến tất cả các node tỷ lệ với bình phương số lượng node. Nếu một blockchain muốn đạt được xác nhận thấp và cố gắng thực hiện điều đó bằng cách sử dụng mạng gossip, nó sẽ buộc phải tập trung vào chỉ một số ít các node.

Xác nhận mở rộng có thể đạt được bằng cách sử dụng kết hợp các kỹ thuật sau:

1. Giao dịch dấu thời gian với một mẫu VDF và ký vào dấu thời gian.
2. Chia các giao dịch thành nhiều đợt, gửi từng giao dịch đến các node riêng biệt và có

   mỗi node chia sẻ lô của nó với các node khác.

3. Lặp lại đệ quy bước trước cho đến khi tất cả các node có tất cả các lô.

Solana luân phiên các leader theo những khoảng thời gian cố định, được gọi là _slot_. Mỗi leader chỉ có thể tạo ra các mục trong slot được phân bổ của mình. Do đó, leader đánh dấu thời gian các giao dịch để các validator có thể tra cứu public key của leader được chỉ định. Sau đó, leader ký vào dấu thời gian để validator có thể xác minh chữ ký, chứng minh người ký là chủ sở hữu public key của leader được chỉ định.

Tiếp theo, các giao dịch được chia thành nhiều lô để một node có thể gửi giao dịch cho nhiều bên mà không cần tạo nhiều bản sao. Ví dụ: nếu leader cần gửi 60 giao dịch đến 6 node, nó sẽ chia tập hợp 60 đó thành các lô gồm 10 giao dịch và gửi một giao dịch đến mỗi node. Điều này cho phép leader đặt 60 giao dịch trên dây, không phải 60 giao dịch cho mỗi node. Sau đó, mỗi node chia sẻ lô của nó với các node khác. Khi node đã thu thập tất cả 6 lô, nó sẽ tạo lại tập hợp ban đầu gồm 60 giao dịch.

Một lô giao dịch chỉ có thể được chia nhiều lần trước khi nó nhỏ đến mức thông tin tiêu đề trở thành người tiêu dùng chính của băng thông mạng. Tại thời điểm viết bài này, cách tiếp cận đang mở rộng lên đến khoảng 150 validator. Để mở rộng quy mô lên đến hàng trăm nghìn validator, mỗi node có thể áp dụng kỹ thuật tương tự như node leader cho một tập hợp các node khác có kích thước bằng nhau. Chúng tôi gọi kỹ thuật này là [_Turbine Block Propogation_](turbine-block-propagation.md).
