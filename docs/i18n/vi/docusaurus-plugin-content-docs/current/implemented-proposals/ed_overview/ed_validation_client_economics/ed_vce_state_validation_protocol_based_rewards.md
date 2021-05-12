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

As a first step to understanding the impact of the _Inflation Schedule_ on the Solana economy, we’ve simulated the upper and lower ranges of what token issuance over time might look like given the current ranges of Inflation Schedule parameters under study.

Đặc biệt:

- _Initial Inflation Rate_: 7-9%
- _Dis-inflation Rate_: -14-16%
- _Long-term Inflation Rate_: 1-2%

Sử dụng các phạm vi này để mô phỏng một số Lich trình lạm phát có thể có, chúng ta có thể khám phá lạm phát theo thời gian:

![](/img/p_inflation_schedule_ranges_w_comments.png)

Trong biểu đồ trên, các giá trị trung bình của phạm vi được xác định để minh họa sự đóng góp của mỗi tham số. From these simulated _Inflation Schedules_, we can also project ranges for token issuance over time.

![](/img/p_total_supply_ranges.png)

Finally we can estimate the _Staked Yield_ on staked SOL, if we introduce an additional parameter, previously discussed, _% of Staked SOL_:

%~\text{SOL Staked} = \frac{\text{Total SOL Staked}}{\text{Total Current Supply}}

In this case, because _% of Staked SOL_ is a parameter that must be estimated (unlike the _Inflation Schedule_ parameters), it is easier to use specific _Inflation Schedule_ parameters and explore a range of _% of Staked SOL_. Đối với ví dụ dưới đây, chúng tôi đã chọn giữa các phạm vi tham số được khám phá ở trên:

- _Initial Inflation Rate_: 8%
- _Dis-inflation Rate_: -15%
- _Long-term Inflation Rate_: 1.5%

The values of _% of Staked SOL_ range from 60% - 90%, which we feel covers the likely range we expect to observe, based on feedback from the investor and validator communities as well as what is observed on comparable Proof-of-Stake protocols.

![](/img/p_ex_staked_yields.png)

Again, the above shows an example _Staked Yield_ that a staker might expect over time on the Solana network with the _Inflation Schedule_ as specified. This is an idealized _Staked Yield_ as it neglects validator uptime impact on rewards, validator commissions, potential yield throttling and potential slashing incidents. It additionally ignores that _% of Staked SOL_ is dynamic by design - the economic incentives set up by this _Inflation Schedule_.

### Năng suất Staking được điều chỉnh

A complete appraisal of earning potential from staking tokens should take into account staked _Token Dilution_ and its impact on staking yield. For this, we define _adjusted staking yield_ as the change in fractional token supply ownership of staked tokens due to the distribution of inflation issuance. Tức là. tác động pha loãng tích cực của lạm phát.

We can examine the _adjusted staking yield_ as a function of the inflation rate and the percent of staked tokens on the network. Chúng ta có thể thấy điều này được vẽ cho các phân số staking khác nhau ở đây:

![](/img/p_ex_staked_dilution.png)
