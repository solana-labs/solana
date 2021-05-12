---
title: Tower BFT
---

Thiết kế này mô tả thuật toán _Tower BFT_ của Solana. Nó giải quyết các vấn đề sau:

- Một số fork có thể không được chấp nhận bởi đa số nhóm và cử tri cần khôi phục sau khi bỏ phiếu cho những fork như vậy.
- Nhiều cử tri khác nhau có thể chọn được nhiều fork và mỗi cử tri có thể thấy một bộ fork khác nhau. Các fork được chọn cuối cùng sẽ hội tụ cho cụm.
- Phiếu bầu dựa trên phần thưởng có rủi ro liên quan. Những người bỏ phiếu phải có khả năng định hình mức độ rủi ro mà họ phải chịu.
- Các [chi phí của rollback](tower-bft.md#cost-of-rollback) cần phải được tính toán. Điều quan trọng đối với khách hàng là dựa vào một số hình thức nhất quán có thể đo lường được. Các chi phí để phá vỡ tính nhất quán cần phải được tính toán và tăng siêu tuyến tính cho các phiếu bầu cũ hơn.
- Tốc độ ASIC khác nhau giữa các node và những kẻ tấn công có thể sử dụng Proof of History ASICS nhanh hơn nhiều so với phần còn lại của cụm. Sự đồng thuận cần phải chống lại các cuộc tấn công khai thác sự thay đổi trong tốc độ ASIC của Proof of History.

Tóm lại, thiết kế này giả định rằng một cử tri duy nhất có stake được triển khai như một validator cá nhân trong cụm.

## Thời gian

Cụm Solana tạo ra nguồn thời gian thông qua Verifiable Delay Function mà chúng tôi gọi là [Proof of History](../cluster/synchronization.md).

Proof of History được sử dụng để tạo ra một lịch thi đấu vòng tròn xác định cho tất cả các leader tích cực. Tại bất kỳ thời điểm nào, chỉ có 1 leader, có thể được tính toán từ chính sổ cái, có thể đề xuất một fork. Để biết thêm chi tiết, hãy xem [fork generation](../cluster/fork-generation.md) và [leader rotation](../cluster/leader-rotation.md).

## Lockouts

Mục đích của lockout là buộc validator cam kết chi phí cho một đợt fork cụ thể. Các lockout được đo bằng các slot và do đó thể hiện độ trễ bắt buộc trong thời gian thực mà validator cần phải đợi trước khi phá vỡ cam kết với một đợt fork.

Những validator vi phạm các lockout và bỏ phiếu cho một fork khác biệt trong lockout sẽ bị trừng phạt. Hình phạt được đề xuất là cắt stake của validator nếu một cuộc bỏ phiếu đồng thời trong một lockout cho một fork không phải con cháu có thể được chứng minh với cụm.

## Thuật toán

Ý tưởng cơ bản của cách tiếp cận này là xếp chồng phiếu đồng thuận và các lockout. Mỗi phiếu bầu trong ngăn xếp là một xác nhận của một fork. Mỗi fork được xác nhận là tổ tiên của fork trên nó. Mỗi phiếu bầu có một đơn vị `lockout` slot trước khi validator có thể gửi một phiếu bầu không chứa fork đã được xác nhận là tổ tiên.

Khi một phiếu bầu được thêm vào ngăn xếp, các lockout của tất cả các phiếu bầu trước đó trong ngăn xếp sẽ được nhân đôi \( nhiều hơn về điều này trong [Rollback](tower-bft.md#Rollback)\). Với mỗi phiếu bầu mới, validator cam kết các phiếu bầu trước đó cho lockout ngày càng tăng. Với 32 phiếu bầu, chúng ta có thể coi phiếu bầu ở mức `max lockout` bất kỳ phiếu bầu nào có lockout bằng hoặc cao hơn `1<<32` được định giá lại \(FIFO\). Bỏ phiếu là yếu tố kích hoạt phần thưởng. Nếu một phiếu bầu hết hạn trước khi nó được xếp lại giá trị, thì phiếu bầu đó và tất cả các phiếu bầu phía trên nó sẽ được xuất hiện \(LIFO\) từ ngăn xếp phiếu bầu. Validator cần bắt đầu xây dựng lại ngăn xếp từ thời điểm đó.

### Khôi phục

Trước khi một phiếu bầu được đẩy vào ngăn xếp, tất cả các phiếu bầu dẫn đến cuộc bỏ phiếu có thời gian khóa thấp hơn phiếu bầu mới sẽ được xuất hiện. Sau khi các lockout không được nhân đôi cho đến validator bắt kịp chiều cao khôi phục của phiếu bầu.

Ví dụ, ngăn xếp phiếu bầu có trạng thái sau:

| phiếu bầu | thời gian bỏ phiếu | lockout | thời gian khóa hết hạn |
| --------: | -----------------: | ------: | ---------------------: |
|         4 |                  4 |       2 |                      6 |
|         3 |                  3 |       4 |                      7 |
|         2 |                  2 |       8 |                     10 |
|         1 |                  1 |      16 |                     17 |

_Vote 5_ ở time 9 và trạng thái kết quả là

| phiếu bầu | thời gian bỏ phiếu | lockout | thời gian khóa hết hạn |
| --------: | -----------------: | ------: | ---------------------: |
|         5 |                  9 |       2 |                     11 |
|         2 |                  2 |       8 |                     10 |
|         1 |                  1 |      16 |                     17 |

_Vote 6_ ở time 10

| phiếu bầu | thời gian bỏ phiếu | lockout | thời gian khóa hết hạn |
| --------: | -----------------: | ------: | ---------------------: |
|         6 |                 10 |       2 |                     12 |
|         5 |                  9 |       4 |                     13 |
|         2 |                  2 |       8 |                     10 |
|         1 |                  1 |      16 |                     17 |

Tại time 10 phiếu bầu mới bắt kịp các phiếu trước. Nhưng _vote 2_ hết hạn ở lúc 10, vì vậy, khi _vote 7_ tại time 11 được áp dụng các phiếu bầu từ _vote 2_ bao gồm trở lên sẽ xuất hiện.

| phiếu bầu | thời gian bỏ phiếu | lockout | thời gian khóa hết hạn |
| --------: | -----------------: | ------: | ---------------------: |
|         7 |                 11 |       2 |                     13 |
|         1 |                  1 |      16 |                     17 |

Việc lockout phiếu bầu 1 sẽ không tăng từ 16 cho đến khi ngăn xếp có 5 phiếu bầu.

### Slashing và phần thưởng

Các validator sẽ được thưởng vì đã chọn fork mà phần còn lại của cụm đã chọn thường xuyên nhất có thể. Điều này phù hợp với việc tạo phần thưởng khi ngăn xếp phiếu bầu đã đầy và phiếu bầu cũ nhất cần được xếp lại. Vì vậy, một phần thưởng sẽ được tạo ra cho mỗi lần rút tiền thành công.

### Chi phí hoàn vốn

Chi phí khôi phục của _fork A_ được định nghĩa là chi phí về lockout để _fork A_ xác nhận bất kỳ fork nào khác không bao gồm _fork A_ như một tổ tiên.

**Tính quyết toán kinh tế** của _fork A_ có thể được tính bằng việc mất tất cả các phần thưởng từ việc khôi phục _fork A_ và con cháu của nó, cộng với chi phí cơ hội của phần thưởng do lockout tăng theo cấp số nhân của các phiếu bầu đã xác nhận _fork A_.

### Ngưỡng

Mỗi validator có thể đặt độc lập ngưỡng cam kết cụm đối với một đợt fork trước khi validator đó cam kết một đợt fork. Ví dụ, ở chỉ số ngăn xếp phiếu bầu 7, lockout là 256 time units. Validator có thể giữ lại phiếu bầu và để phiếu bầu 0-7 hết hạn trừ khi phiếu bầu ở chỉ số 7 có cam kết lớn hơn 50% trong cụm. Điều này cho phép mỗi validator kiểm soát độc lập mức độ rủi ro khi cam kết một đợt fork. Cam kết fork ở tần suất cao hơn sẽ cho phép validator kiếm được nhiều phần thưởng hơn.

### Thông số thuật toán

Các thông số sau cần được điều chỉnh:

- Số phiếu bầu trong ngăn xếp trước khi xếp hàng xảy ra \(32\).
- Tốc độ tăng cho các lần khóa trong ngăn xếp \(2x\).
- Khởi động lockout mặc định \(2\).
- Độ sâu ngưỡng cho cam kết cụm tối thiểu trước khi cam kết với fork \(8\).
- Kích thước cam kết cụm tối thiểu ở độ sâu ngưỡng \(50%+\).

### Free Choice

"Free Choice" là một hành động validator không thể cưỡng lại. Không có cách nào để giao thức mã hóa và thực thi các hành động này vì mỗi validator có thể sửa đổi mã và điều chỉnh thuật toán. Validator tối đa hóa phần thưởng cho bản thân trên tất cả các hợp đồng tương lai có thể có nên hoạt động theo cách sao cho hệ thống ổn định và lựa chọn tham lam cục bộ sẽ dẫn đến lựa chọn tham lam trên tất cả các tương lai có thể có. Một tập hợp validator đang tham gia vào các lựa chọn để phá vỡ giao thức nên bị ràng buộc bởi tỷ trọng stake của họ đối với việc từ chối dịch vụ. Hai tùy chọn thoát cho validator:

- validator có thể chạy nhanh hơn validator trước đó trong thế hệ ảo và gửi fork đồng thời
- validator có thể giữ lại một phiếu bầu để quan sát nhiều fork trước khi bỏ phiếu

Trong cả hai trường hợp, validator trong cụm có một số fork để chọn đồng thời, mặc dù mỗi fork đại diện cho một chiều cao khác nhau. Trong cả hai trường hợp, giao thức không thể phát hiện xem hành vi của validator có cố ý hay không.

### Sự lựa chọn tham lam cho các Fork đồng thời

Khi đánh giá nhiều fork, mỗi validator phải sử dụng các quy tắc sau:

1. Các fork phải đáp ứng quy tắc _Ngưỡng_.
2. Chọn fork để tối đa hóa tổng thời gian lockout cụm cho tất cả các fork trước.
3. Chọn fork có số phí giao dịch cụm lớn nhất.
4. Chọn fork mới nhất của PoH.

Phí giao dịch theo cụm là phí được gửi vào mining pool như được mô tả trong phần [Phần thưởng Staking](staking-rewards.md).

## Kháng PoH ASIC

Số phiếu bầu và số lockout tăng lên theo cấp số nhân trong khi tốc độ ASIC là tuyến tính. Có thể có hai vector tấn công liên quan đến một ASIC nhanh hơn.

### Kiểm duyệt ASIC

Kẻ tấn công tạo ra một fork đồng thời vượt trội hơn các leader trước đó trong nỗ lực kiểm duyệt chúng. Một fork do kẻ tấn công này đề xuất sẽ có sẵn đồng thời với leader sẵn có tiếp theo. Đối với các node chọn fork này, nó phải đáp ứng quy tắc _Greedy Choice_.

1. Fork phải có số phiếu bầu bằng nhau cho fork tổ tiên.
2. Fork không thể là người đứng đầu gây nguyên nhân các phiếu bầu hết hạn.
3. Fork phải có chi phí giao dịch cụm lớn hơn.

Cuộc tấn công này sau đó được giới hạn trong việc kiểm duyệt phí của các leader trước đó và các giao dịch cá nhân. Nhưng nó không thể tạm dừng cụm hoặc giảm bộ validator so với fork đồng thời. Việc kiểm duyệt phí được giới hạn ở các khoản phí truy cập thuộc về các leader nhưng không phải các validator.

### Khôi phục ASIC

Kẻ tấn công tạo ra một fork đồng thời từ một khối cũ hơn để cố gắng khôi phục cụm. Trong cuộc tấn công này, fork đồng thời đang cạnh tranh với các fork đã được bỏ phiếu. Cuộc tấn công này bị hạn chế bởi sự phát triển theo cấp số nhân của các lockout.

- 1 phiếu bầu có lockout cùa 2 slot. Fork đồng thời phải có ít nhất 2 slot phía trước, và được tạo ra trong 1 slot. Do đó, yêu cầu ASIC nhanh hơn gấp 2 lần.
- 2 phiếu bầu có lockout trong 4 slot. Fork đồng thời phải ở phía trước ít nhất 4 slot và được tạo ra trong 2 slot. Do đó, yêu cầu ASIC nhanh hơn gấp 2 lần.
- 3 phiếu bầu có lockout trong 8 slot. Fork đồng thời phải ở phía trước ít nhất 8 slot và được tạo ra trong 3 slot. Do đó, yêu cầu ASIC nhanh hơn gấp 2.6 lần.
- 10 phiếu bầu có lockout trong 1024 slot. 1024/10 hoặc nhanh hơn 102.4 lần ASIC.
- 20 phiếu bầu có lockout trong 2^20 slot. 2^20/20 hoặc nhanh hơn 52,428.8 lần ASIC.
