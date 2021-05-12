---
title: Transaction Fees
---

**Có thể thay đổi.**

Each transaction sent through the network, to be processed by the current leader validation-client and confirmed as a global state transaction, contains a transaction fee. Phí giao dịch mang lại nhiều lợi ích trong thiết kế kinh tế Solana, ví dụ:

- cung cấp đơn vị bồi thường cho mạng validator cho các tài nguyên CPU/GPU cần thiết để xử lý giao dịch trạng thái,
- giảm spam mạng bằng cách giới thiệu chi phí thực cho các giao dịch,
- và cung cấp sự ổn định kinh tế lâu dài tiềm năng của mạng thông qua số tiền phí tối thiểu do giao thức thu được trên mỗi giao dịch, như được mô tả bên dưới.

Network consensus votes are sent as normal system transfers, which means that validators pay transaction fees to participate in consensus.

Nhiều nền kinh tế blockchain hiện tại ( ví dụ như Bitcoin, Ethereum), dựa vào phần thưởng dựa trên giao thức để hỗ trợ nền kinh tế trong ngắn hạn, với giả định rằng doanh thu được tạo ra thông qua phí giao dịch sẽ hỗ trợ nền kinh tế trong dài hạn, khi phần thưởng bắt nguồn từ giao thức hết hạn. In an attempt to create a sustainable economy through protocol-based rewards and transaction fees, a fixed portion (initially 50%) of each transaction fee is destroyed, with the remaining fee going to the current leader processing the transaction. Tỷ lệ lạm phát toàn cầu theo lịch trình cung cấp nguồn cho phần thưởng được phân phối cho các khách hàng-xác nhận, thông qua quy trình được mô tả ở trên.

Phí giao dịch được đặt bởi cụm mạng dựa trên thông lượng lịch sử gần đây, hãy xem [Phí thúc đẩy tắc nghẽn](implemented-proposals/transaction-fees.md#congestion-driven-fees). This minimum portion of each transaction fee can be dynamically adjusted depending on historical _signatures-per-slot_. Bằng cách này, giao thức có thể sử dụng mức phí tối thiểu để nhắm mục tiêu sử dụng phần cứng mong muốn. By monitoring a protocol specified _signatures-per-slot_ with respect to a desired, target usage amount, the minimum fee can be raised/lowered which should, in turn, lower/raise the actual _signature-per-slot_ per block until it reaches the target amount. Quá trình điều chỉnh này có thể được coi là tương tự như thuật toán điều chỉnh độ khó trong giao thức Bitcoin, tuy nhiên trong trường hợp này, nó đang điều chỉnh phí giao dịch tối thiểu để hướng dẫn việc sử dụng phần cứng xử lý giao dịch đến mức mong muốn.

Như đã đề cập, một tỷ lệ cố định của mỗi khoản phí giao dịch sẽ bị hủy bỏ. The intent of this design is to retain leader incentive to include as many transactions as possible within the leader-slot time, while providing an inflation limiting mechanism that protects against "tax evasion" attacks \(i.e. side-channel fee payments\).

Ngoài ra, các khoản phí bị cháy có thể được cân nhắc khi lựa chọn fork. Trong trường hợp một đợt fork PoH với một kẻ đứng đầu kiểm duyệt, độc hại, chúng tôi hy vọng tổng số phí bị phá hủy sẽ ít hơn một đợt fork trung thực có thể so sánh được, do phí bị mất từ ​​việc kiểm duyệt. Nếu leader kiểm duyệt phải bù đắp cho các khoản phí giao thức bị mất này, họ sẽ phải tự thay thế các khoản phí bị đốt cháy trên fork của họ, do đó có khả năng làm giảm động cơ kiểm duyệt ngay từ đầu.
