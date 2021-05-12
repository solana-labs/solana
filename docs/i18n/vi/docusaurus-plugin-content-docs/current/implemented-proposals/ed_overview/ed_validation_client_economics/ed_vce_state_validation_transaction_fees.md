---
title: Phí giao dịch xác thực theo trạng thái
---

**Có thể thay đổi.**

Mỗi giao dịch được gửi qua mạng, được xử lý bởi leader validation-client hiện tại và được xác nhận là giao dịch trạng thái toàn cầu, phải có phí giao dịch. Phí giao dịch mang lại nhiều lợi ích trong thiết kế kinh tế Solana, ví dụ:

- cung cấp đơn vị bồi thường cho mạng validator cho các tài nguyên CPU/GPU cần thiết để xử lý giao dịch trạng thái,
- giảm spam mạng bằng cách giới thiệu chi phí thực cho các giao dịch,
- mở ra các con đường cho thị trường giao dịch để khuyến khích validation-client thu thập và xử lý các giao dịch đã gửi trong chức năng của họ với tư cách là leader,
- và cung cấp sự ổn định kinh tế lâu dài tiềm năng của mạng thông qua số tiền phí tối thiểu do giao thức thu được trên mỗi giao dịch, như được mô tả bên dưới.

Nhiều nền kinh tế blockchain hiện tại ( ví dụ như Bitcoin, Ethereum), dựa vào phần thưởng dựa trên giao thức để hỗ trợ nền kinh tế trong ngắn hạn, với giả định rằng doanh thu được tạo ra thông qua phí giao dịch sẽ hỗ trợ nền kinh tế trong dài hạn, khi phần thưởng bắt nguồn từ giao thức hết hạn. Trong nỗ lực tạo ra một nền kinh tế bền vững thông qua phần thưởng dựa trên giao thức và phí giao dịch, một phần cố định của mỗi khoản phí giao dịch sẽ bị hủy, với phần phí còn lại sẽ được chuyển cho nhà leader hiện tại xử lý giao dịch. Tỷ lệ lạm phát toàn cầu theo lịch trình cung cấp nguồn cho phần thưởng được phân phối cho các khách hàng-xác nhận, thông qua quy trình được mô tả ở trên.

Phí giao dịch được đặt bởi cụm mạng dựa trên thông lượng lịch sử gần đây, hãy xem [Phí thúc đẩy tắc nghẽn](../../transaction-fees.md#congestion-driven-fees). Phần tối thiểu này của mỗi khoản phí giao dịch có thể được điều chỉnh tùy thuộc vào việc sử dụng gas trước đây. Bằng cách này, giao thức có thể sử dụng mức phí tối thiểu để nhắm mục tiêu sử dụng phần cứng mong muốn. Bằng cách giám sát một giao thức sử dụng gas cụ thể đối với lượng sử dụng mục tiêu, mong muốn, mức phí tối thiểu có thể được nâng lên/hạ xuống, do đó, giảm/nâng mức sử dụng gas thực tế trên mỗi khối cho đến khi đạt đến lượng mục tiêu. Quá trình điều chỉnh này có thể được coi là tương tự như thuật toán điều chỉnh độ khó trong giao thức Bitcoin, tuy nhiên trong trường hợp này, nó đang điều chỉnh phí giao dịch tối thiểu để hướng dẫn việc sử dụng phần cứng xử lý giao dịch đến mức mong muốn.

Như đã đề cập, một tỷ lệ cố định của mỗi khoản phí giao dịch sẽ bị hủy bỏ. Mục đích của thiết kế này là duy trì động lực của leader để bao gồm nhiều giao dịch nhất có thể trong khoảng thời gian dành cho leader, đồng thời cung cấp cơ chế hạn chế lạm phát nhằm bảo vệ chống lại các cuộc tấn công "trốn thuế" \( tức là thanh toán phí kênh phụ\)[1](../ed_references.md).

Ngoài ra, các khoản phí bị cháy có thể được cân nhắc khi lựa chọn fork. Trong trường hợp một đợt fork PoH với một kẻ đứng đầu kiểm duyệt, độc hại, chúng tôi hy vọng tổng số phí bị phá hủy sẽ ít hơn một đợt fork trung thực có thể so sánh được, do phí bị mất từ ​​việc kiểm duyệt. Nếu leader kiểm duyệt phải bù đắp cho các khoản phí giao thức bị mất này, họ sẽ phải tự thay thế các khoản phí bị đốt cháy trên fork của họ, do đó có khả năng làm giảm động cơ kiểm duyệt ngay từ đầu.
