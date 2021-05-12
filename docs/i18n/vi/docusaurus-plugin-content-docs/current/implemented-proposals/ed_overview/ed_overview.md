---
title: Kinh tế Cụm
---

**Có thể thay đổi. Theo dõi các cuộc thảo luận kinh tế gần đây nhất trên diễn đàn Solana: https://forums.solana.com**

Hệ thống kinh tế tiền điện tử của Solana được thiết kế để thúc đẩy một nền kinh tế lành mạnh, tự duy trì lâu dài với các động lực khuyến khích người tham gia phù hợp với tính bảo mật và phi tập trung của mạng. Những người tham gia chính trong nền kinh tế này là validation-clients. Những đóng góp của họ cho mạng, xác nhận trạng thái và các cơ chế khuyến khích cần thiết của họ được thảo luận dưới đây.

Các kênh chuyển tiền chính của người tham gia được gọi là phần thưởng và phí giao dịch dựa trên giao thức (lạm phát). Phần thưởng dựa trên giao thức là các đợt phát hành từ tỷ lệ lạm phát toàn cầu, do giao thức xác định. Những phần thưởng này sẽ tạo thành tổng phần thưởng được giao cho validation-clients, phần còn lại được lấy từ phí giao dịch. Trong những ngày đầu của mạng, có khả năng phần thưởng dựa trên giao thức, được triển khai dựa trên lịch trình phát hành được xác định trước, sẽ thúc đẩy khuyến khích người tham gia tham gia vào mạng.

Các phần thưởng dựa trên giao thức này, được phân phối trên các mã thông báo được stake tích cực trên mạng, là kết quả của tỷ lệ lạm phát nguồn cung toàn cầu, được tính theo kỷ nguyên Solana và được phân phối giữa các validator đang hoạt động. Như được thảo luận thêm bên dưới, tỷ lệ lạm phát hàng năm dựa trên một lịch trình phi lạm phát được xác định trước. Điều này cung cấp cho mạng khả năng dự đoán cung ứng tiền tệ, hỗ trợ sự ổn định và an ninh kinh tế lâu dài.

Phí giao dịch là chuyển khoản từ người tham gia sang người tham gia dựa trên thị trường, gắn liền với các tương tác mạng như một động lực cần thiết và bồi thường cho việc bao gồm và thực hiện một giao dịch được đề xuất. Một cơ chế để ổn định kinh tế lâu dài và tăng cường bảo vệ thông qua việc đốt một phần phí giao dịch cũng được thảo luận dưới đây.

Sơ đồ cấp cao về thiết kế kinh tế tiền điện tử của Solana được thể hiện dưới đây trong **Hình 1**. Các chi tiết cụ thể của kinh tế học validation-client được mô tả trong các phần: [Kinh tế học validation-client](ed_validation_client_economics/ed_vce_overview.md),[Lịch trình lạm phát](ed_validation_client_economics/ed_vce_state_validation_protocol_based_rewards.md) và [Phí giao dịch](ed_validation_client_economics/ed_vce_state_validation_transaction_fees.md). Ngoài ra, phần có tiêu đề [Validation Stake Delegation](ed_validation_client_economics/ed_vce_validation_stake_delegation.md) kết thúc với cuộc thảo luận về các cơ hội ủy quyền validator và thị trường. Ngoài ra, trong [Storage Rent Economics](ed_storage_rent_economics.md), chúng tôi mô tả việc triển khai tiền thuê lưu trữ để tính cho các chi phí ngoại ứng của việc duy trì trạng thái hoạt động của sổ cái. Phần phác thảo các tính năng của thiết kế kinh tế MVP được thảo luận trong phần [Economic Design MVP](ed_mvp.md).

![](/img/economic_design_infl_230719.png)

**Hình 1**: Sơ đồ tổng quan về thiết kế khuyến khích kinh tế Solana.
