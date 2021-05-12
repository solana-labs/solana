---
title: Lịch trình lạm phát
---

**Có thể thay đổi. Theo dõi các cuộc thảo luận kinh tế gần đây nhất trong diễn đàn Solana: https://forums.solana.com**

Validator-client có hai vai trò chức năng trong mạng Solana:

- Xác thực \(bỏ phiếu\) trạng thái toàn cầu hiện tại của PoH được quan sát của họ.
- Được bầu làm 'leader' theo lịch trình quay vòng có tỷ trọng stake trong thời gian đó họ chịu trách nhiệm thu thập các giao dịch chưa thanh toán và kết hợp chúng vào PoH được quan sát của họ, cập nhật trạng thái toàn cầu của mạng và cung cấp tính liên tục của chuỗi.

Phần thưởng của validator khách hàng cho các dịch vụ này sẽ được phân phối vào cuối mỗi kỷ nguyên Solana. Như đã thảo luận trước đây, khoản bồi thường cho khách hàng của validator được cung cấp thông qua một khoản hoa hồng tính theo tỷ lệ lạm phát hàng năm dựa trên giao thức được phân tán tương ứng với tỷ trọng stake của mỗi validator-node ( xem bên dưới) cùng với phí giao dịch do leader xác nhận có sẵn trong mỗi luân chuyển leader. Tức là. trong thời gian một validator-client nhất định được bầu làm leader, nó có cơ hội giữ một phần của mỗi khoản phí giao dịch, trừ đi số tiền do giao thức chỉ định bị phá hủy \(xem [Phí giao dịch trạng thái khách hàng xác thực](ed_vce_state_validation_transaction_fees.md)\).

Lợi suất staking hàng năm dựa trên giao thức hiệu quả \(%\) trên mỗi kỷ nguyên mà các khách hàng xác thực nhận được là một chức năng của:

- tỷ lệ lạm phát toàn cầu hiện tại, xuất phát từ lịch trình phát hành phi lạm phát được xác định trước \(xem [Kinh tế khách hàng xác thực](ed_vce_overview.md)\)
- phần SOL được stake trong tổng nguồn cung lưu hành hiện tại,
- hoa hồng được tính bởi dịch vụ xác thực,
- thời gian hoạt động/tham gia \[% slot có sẵn mà validator có cơ hội bỏ phiếu\] của một validator nhất định so với kỷ nguyên trước đó.

Yếu tố đầu tiên là một chức năng của chỉ các tham số giao thức \(tức là không phụ thuộc vào hành vi của validator trong một kỷ nguyên nhất định\) và dẫn đến một lịch trình lạm phát được thiết kế để khuyến khích sự tham gia sớm, cung cấp sự ổn định tiền tệ rõ ràng và cung cấp bảo mật tối ưu trong mạng.

Là bước đầu tiên để hiểu tác động của _Lịch trình lạm phát _ đối với nền kinh tế Solana, chúng tôi đã mô phỏng phạm vi trên và dưới của việc phát hành mã thông báo theo thời gian có thể trông như thế nào phạm vi hiện tại của các tham số Lịch trình Lạm phát đang được nghiên cứu.

Đặc biệt:

- _Tỷ lệ lạm phát ban đầu_: 7-9%
- _Tỷ lệ phi lạm phát_: -14-16%
- _Tỷ lệ Lạm phát dài hạn_: 1-2%

Sử dụng các phạm vi này để mô phỏng một số Lich trình lạm phát có thể có, chúng ta có thể khám phá lạm phát theo thời gian:

![](/img/p_inflation_schedule_ranges_w_comments.png)

Trong biểu đồ trên, các giá trị trung bình của phạm vi được xác định để minh họa sự đóng góp của mỗi tham số. Từ các _Lich trình lạm phát_ mô phỏng này, chúng tôi cũng có thể dự đoán phạm vi phát hành mã thông báo theo thời gian.

![](/img/p_total_supply_ranges.png)

Cuối cùng, chúng tôi có thể ước tính _Lợi nhuận đã stake_ trên SOL đã stake, nếu chúng tôi giới thiệu một tham số bổ sung, đã được thảo luận trước đó, _% của SOL đã stake_:

%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}

Trong trường hợp này, vì _% trong số SOL đã stake_ là một tham số phải được ước tính (không giống như các tham số _Lịch trình lạm phát_), sẽ dễ dàng hơn khi sử dụng các tham số _Lịch trình lạm phát_ cụ thể và khám phá phạm vi _% trong số SOL đã stake_. Đối với ví dụ dưới đây, chúng tôi đã chọn giữa các phạm vi tham số được khám phá ở trên:

- _Tỷ lệ lạm phát ban đầu_: 8%
- _Tỷ lệ phi lạm phát_: -15%
- _Tỷ lệ Lạm phát dài hạn_: 1.5%

Giá trị _% trong số SOL đã stake_ nằm trong khoảng từ 60% - 90%, mà chúng tôi cảm thấy bao gồm phạm vi chúng tôi mong đợi để quan sát, dựa trên phản hồi từ nhà đầu tư và cộng đồng validator cũng như những gì quan sát được trên các giao thức Proof-of-Stake tương đương.

![](/img/p_ex_staked_yields.png)

Một lần nữa, phần trên cho thấy một ví dụ về _Lợi nhuận stake_ mà một người tham gia stake có thể mong đợi theo thời gian trên mạng Solana với _Lịch trình lạm phát_ như được chỉ định. Đây là một _Lợi nhuận cố định_ được lý tưởng hóa vì nó bỏ qua tác động của thời gian hoạt động của validator đối với phần thưởng, hoa hồng của validator, điều chỉnh lợi nhuận tiềm năng và các sự cố slashing tiềm ẩn. Nó cũng bỏ qua rằng _% trong số SOL đã stake_ là động theo thiết kế - các động lực kinh tế được thiết lập bởi _Lịch trình lạm phát_ này.

### Năng suất Staking được điều chỉnh

Đánh giá đầy đủ về tiềm năng kiếm tiền từ việc staking mã thông báo phải tính đến _Pha loãng mã thông báo_ và tác động của nó đối với lợi nhuận staking. Đối với điều này, chúng tôi xác định _điều chỉnh năng suất Staking_ là sự thay đổi trong quyền sở hữu nguồn cung mã thông báo phân đoạn đối với các mã thông báo đã stake do phân phối phát hành lạm phát. Tức là. tác động pha loãng tích cực của lạm phát.

Chúng tôi có thể kiểm tra _năng suất staking được điều chỉnh_ như một hàm của tỷ lệ lạm phát và phần trăm mã thông báo đã stake trên mạng. Chúng ta có thể thấy điều này được vẽ cho các phân số staking khác nhau ở đây:

![](/img/p_ex_staked_dilution.png)
