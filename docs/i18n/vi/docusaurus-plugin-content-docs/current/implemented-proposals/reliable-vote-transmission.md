---
title: Truyền phiếu bầu đáng tin cậy
---

Phiếu bầu validator là thông điệp có chức năng quan trọng đối với sự đồng thuận và hoạt động liên tục của mạng. Do đó, điều quan trọng là chúng phải được phân phối một cách đáng tin cậy và được mã hóa vào sổ cái.

## Thử thách

1. Vòng quay leader được kích hoạt bởi PoH, là đồng hồ có độ trôi cao. Vì vậy, nhiều node có thể có chế độ xem không chính xác nếu leader tiếp theo có hoạt động trong thời gian thực hay không.
2. Leadet tiếp theo có thể dễ dàng bị ngập lụt. Do đó, DDOS sẽ không chỉ ngăn chặn việc phân phối các giao dịch thông thường mà còn cả các thông điệp đồng thuận.
3. UDP không đáng tin cậy và giao thức không đồng bộ của chúng tôi yêu cầu bất kỳ thông điệp nào được truyền đi phải được truyền lại cho đến khi nó được quan sát thấy trong sổ cái. Việc truyền lại có thể gây ra _thundering herd_ không chủ ý chống lại leader với một số lượng lớn các validator. Trường hợp xấu nhất lũ lụt sẽ là `(num_nodes * num_retransmits)`.
4. Việc theo dõi xem phiếu bầu đã được truyền đi hay chưa thông qua sổ cái không đảm bảo nó sẽ xuất hiện trong một khối đã được xác nhận. Khối được quan sát hiện tại có thể chưa được cuộn. Các validator sẽ cần duy trì trạng thái cho mỗi phiếu bầu và fork.

## Thiết kế

1. Gửi phiếu bầu như một thông điệp thúc đẩy thông qua gossip. Điều này đảm bảo việc bỏ phiếu cho tất cả các leader tiếp theo, không chỉ cho những leader tiếp theo trong tương lai.
2. Các leader sẽ đọc bảng Crds cho các phiếu bầu mới và mã hóa mọi phiếu bầu mới nhận được vào các khối mà họ đề xuất. Điều này cho phép tất cả các leader tương lai bao gồm các phiếu bầu của validator trong các nhánh lùi.
3. Các validator nhận được phiếu bầu trong sổ cái sẽ thêm chúng vào bảng crds cục bộ của họ, không phải là một yêu cầu đẩy mà chỉ cần thêm chúng vào bảng. Điều này làm tắt giao thức thông báo đẩy, vì vậy các thông báo xác thực không cần phải được truyền lại hai lần trong mạng.
4. CrdsValue cho phiếu bầu sẽ giống như sau `Votes(Vec<Transaction>)`

Mỗi giao dịch bỏ phiếu phải duy trì một `wallclock` trong dữ liệu của nó. Chiến lược hợp nhất cho Phiếu bầu sẽ giữ N bộ phiếu cuối cùng như được cấu hình bởi máy khách cục bộ. Đối với push/pull, vector được duyệt đệ quy và mỗi Giao dịch được coi như một CrdsValue riêng lẻ với ép xung cục bộ và chữ ký của riêng nó.

Gossip được thiết kế để truyền bá trạng thái hiệu quả. Các thông điệp được gửi thông qua gossip-push được phân lô và truyền với một cây bao trùm tối thiểu tới phần còn lại của mạng. Mọi lỗi cục bộ trong cây đều được sửa chữa tích cực bằng giao thức gossip-pull đồng thời giảm thiểu lượng dữ liệu được truyền giữa bất kỳ node nào.

## Cách thiết kế này giải quyết các Thử thách

1. Bởi vì không có cách nào dễ dàng để các validator đồng bộ với các leader ở trạng thái "hoạt động" của leader, gossip cho phép phân phối cuối cùng bất kể trạng thái đó.
2. Gossip sẽ gửi thông điệp đến tất cả các leader tiếp theo, vì vậy nếu leader hiện tại bị ngập lụt, leader tiếp theo sẽ nhận được những phiếu bầu này và có thể mã hóa chúng.
3. Gossip giảm thiểu số lượng yêu cầu thông qua mạng bằng cách duy trì một cây bao trùm hiệu quả và sử dụng bộ lọc nở để sửa chữa trạng thái. Vì vậy, việc truyền ngược lại là không cần thiết và các tin nhắn sẽ được chia thành từng đợt.
4. Các leader đọc bảng crds cho các phiếu bầu sẽ mã hóa tất cả các phiếu bầu hợp lệ mới xuất hiện trong bảng. Ngay cả khi khối của leader này không được ghi danh, leader tiếp theo sẽ cố gắng thêm các phiếu bầu tương tự mà không cần validator thực hiện thêm bất kỳ công việc nào. Do đó, đảm bảo không chỉ phân phối cuối cùng, mà còn mã hóa cuối cùng vào sổ cái.

## Hiệu suất

1. Trường hợp xấu nhất là thời gian truyền đến leader tiếp theo là bước nhảy Log\(N\) với cơ sở phụ thuộc vào fanout. Với fanout mặc định hiện tại của chúng tôi là 6, nó là khoảng 6 bước nhảy đến 20k node.
2. Leader sẽ nhận được 20k phiếu xác nhận được tổng hợp bằng gossip-push thành các shred cỡ MTU. Điều này sẽ làm giảm số lượng gói cho mạng 20k xuống còn 80 shred.
3. Mỗi phiếu bầu của các validator được nhân rộng trên toàn bộ mạng. Để duy trì hàng đợi 5 phiếu bầu trước đó, bảng Crds sẽ tăng thêm 25 megabyte. `(20,000 nodes * 256 bytes * 5)`.

## Triển khai hai bước

Ban đầu mạng có thể thực hiện một cách đáng tin cậy chỉ với 1 phiếu bầu được truyền và duy trì qua mạng với việc triển khai Phiếu bầu hiện tại. Đối với các mạng nhỏ, 6 fanout là đủ. Với mạng nhỏ, bộ nhớ và chi phí đẩy là nhỏ.

### Mạng validator phụ 1k

1. Crds chỉ duy trì phiếu bầu mới nhất của các validator.
2. Các phiếu bầu được đẩy và truyền lại bất kể chúng có xuất hiện trong sổ cái hay không.
3. Fanout của 6.
4. Trường hợp xấu nhất là chi phí bộ nhớ 256kb trên mỗi node.
5. Trường hợp xấu nhất là 4 bước nhảy để truyền đến mọi node.
6. Leader sẽ nhận được toàn bộ phiếu bầu của validator được đặt trong 4 mẩu tin nhắn đẩy.

### Mạng phụ 20k

Mọi thứ ở trên cộng với những thứ sau:

1. Bảng CRDS duy trì một vector gồm 5 phiếu bầu validator mới nhất.
2. Phiếu bầu mã hóa wallclock. CrdsValue:: Votes là một kiểu đệ quy thành vector giao dịch cho tất cả các giao thức gossip.
3. Tăng fanout lên 20.
4. Trường hợp xấu nhất là chi phí bộ nhớ 25mb cho mỗi node.
5. Sub 4 bước trong trường hợp xấu nhất để cung cấp cho toàn bộ mạng.
6. 80 shred được nhận bởi leader cho tất cả các tin nhắn validator.
