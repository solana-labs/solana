---
title: Thuê
---

Các tài khoản trên Solana có thể có trạng thái do chủ sở hữu kiểm soát \(`Account::data`\) tách biệt với số dư tài khoản \(`Account::lamports`\). Vì các validator trên mạng cần duy trì bản sao hoạt động của trạng thái này trong bộ nhớ, mạng tính phí dựa trên thời gian và không gian cho việc tiêu thụ tài nguyên này, còn được gọi là Tiền thuê.

## Chế độ thuê hai tầng

Các tài khoản duy trì số dư tối thiểu tương đương với 2 năm của thanh toán tiền thuê thì được miễn. _2 năm_ được rút ra từ chi phí phần cứng thực tế giảm 50% giá mỗi 2 năm đó là kết quả của sự hội tụ do là một chuỗi hình học. Các tài khoản có số dư giảm xuống dưới ngưỡng này sẽ được tính tiền thuê với tỉ lệ được chỉ định ban đầu, tính bằng lamport mỗi byte/năm. Mạng tính phí tiền thuê trên cơ sở mỗi kỷ nguyên, bằng tín dụng cho kỷ nguyên tiếp theo và `Account::rent_epoch` theo dõi số tiền thuê trong lần tiếp theo sẽ được thu thập từ tài khoản.

Hiện tại, chi phí thuê được cố định như ban đầu. Tuy nhiên, nó được dự đoán là sẽ linh hoạt, phản ánh chi phí lưu trữ phần cứng vào thời điểm đó. Vì vậy, giá được dự kiến sẽ giảm khi chi phí phần cứng giảm khi công nghệ được tiến bộ.

## Thời gian thu tiền thuê

Có hai thời điểm thu tiền thuê từ tài khoản: \(1\) khi được tham chiếu bởi một giao dịch, \(2\) định kỳ một lần mỗi kỷ nguyên. \(1\) bao gồm giao dịch tự tạo tài khoản mới và nó xảy ra trong quá trình xử lý giao dịch bình thường của ngân hàng, như là một phần của giai đoạn tải. \(2\) tồn tại để đảm bảo thu tiền thuê từ các tài khoản cũ, vốn không được đề cập đến trong các kỷ nguyên gần đây. \(2\) yêu cầu quét toàn bộ tài khoản và trải dài trong một kỷ nguyên dựa trên tiền tố địa chỉ tài khoản để tránh tăng đột biến tải do thu tiền thuê này.

Ngược lại, việc thu tiền thuê không áp dụng cho các tài khoản bị thao túng trực tiếp bởi bất kỳ quy trình kế toán cấp giao thức nào bao gồm:

- Bản thân của việc phân phối thu tiền thuê (Nếu không, nó có thể gây ra xử lý thu tiền thuê đệ quy)
- Việc phân phối phần thưởng staking vào đầu mỗi kỷ nguyên (Để giảm xử lý tăng đột biến khi bắt đầu kỷ nguyên mới)
- Việc phân bổ phí giao dịch vào cuối mỗi Slot

Ngay cả khi các quy trình đó nằm ngoài phạm vi thu tiền thuê, tất cả các tài khoản bị thao túng cuối cùng sẽ được xử lý bởi cơ chế \(2\).

## Thực tế xử lý thu tiền thuê

Tiền thuê là do giá trị thời gian của kỷ nguyên, và các tài khoản có `Account::rent_epoch` của `current_epoch` của `current_epoch + 1` tùy thuộc vào chế độ thuê.

Nếu tài khoản ở chế độ miễn trừ, `Account::rent_epoch` chỉ cần cập nhật lên `current_epoch`.

Nếu tài khoản không được miễn trừ, sự chênh lệnh giữa epoch tiếp theo và `Account::rent_epoch` được sử dụng để tính số tiền thuê tài khoản này \(thông qua `Rent::due()`\). Bất kỳ lamport phân số nào của phép tính đều bị cắt bớt. Tiền thuê đến hạn được khấu trừ khỏi `Account::lamports`` và <code>Account::rent_epoch` được cập nhật vào `current_epoch + 1` (= kỷ nguyên tiếp theo). Nếu số tiền thuê đến hạn ít hơn một lamport, không có thay đổi nào được thực hiện đối với tài khoản.

Các tài khoản có số dư không đủ để đáp ứng tiền thuê đến hạn sẽ không tải được.

Một phần trăm tiền thuê sẽ được phá hủy. Phần còn lại được phân phối cho các tài khoản validator theo số lượng stake, phí giao dịch ở cuối mỗi slot.

Cuối cùng, việc thu tiền thuê xảy ra theo các cập nhật tài khoản cấp giao thức như phân phối tiền thuê cho các validator, nghĩa là không có giao dịch tương ứng cho các khoản khấu trừ tiền thuê. Vì vậy, việc thu tiền thuê là khá vô hình, chỉ có thể ngầm quan sát được bởi một giao dịch gần đây hoặc thời gian xác định trước với địa chỉ tài khoản của nó.

## Cân nhắc thiết kế

### Cơ sở lý luận về thiết kế hiện tại

Theo thiết kế trước đây, KHÔNG THỂ có các tài khoản kéo dài, không bị động đến và không phải trả tiền thuê. Các tài khoản luôn được trả tiền thuê chính xác một lần cho mỗi kỷ nguyên, ngoại trừ các tài khoản được miễn tiền thuê, tài khoản sysvar và tài khoản thực thi.

Đây là sự lựa chọn thiết kế dự định. Nếu không, có thể kích hoạt thu tiền thuê trái phép với sự hướng dẫn của `Noop` bởi bất kỳ ai có thể trục lợi tiền thuê ( leader của thời điểm này) hoặc tiết kiệm tiền thuê với chi phí thuê dao động dự kiến.

Một tác dụng phụ khác của lựa chọn này, cũng lưu ý rằng việc thu tiền thuê định kỳ này buộc validator không lưu trữ các tài khoản cũ vào kho lạnh một cách lạc quan và tiết kiệm chi phí lưu trữ, điều này gây bất lợi cho chủ sở hữu tài khoản và có thể khiến các giao dịch trên đó bị đình trệ lâu hơn hơn những cái khác. Mặt khác, điều này ngăn người dùng độc hại nhồi nhét một lượng lớn tài khoản rác, tạo gánh nặng cho các validator.

Như kết quả tổng thể của thiết kế này, tất cả các tài khoản được lưu trữ như nhau dưới dạng hoạt động của validator với các đặc điểm hoạt động giống nhau, phản ánh thẳng thắn cấu trúc định giá tiền thuê đã thống nhất.

### Bộ sưu tập đặc biệt

Việc thu tiền thuê trên cơ sở cần thiết \(tức là bất cứ khi nào tài khoản được tải /truy cập\) đã được xem xét. Các vấn đề với cách tiếp cận này là:

- các tài khoản được nạp dưới dạng "credit only" cho một giao dịch rất có thể được dự kiến sẽ có tiền thuê khi đến hạn,

  nhưng sẽ không thể ghi vào bất kỳ giao dịch nào

- một cơ chế để "beat the bushes" \(tức là đi tìm tài khoản cần trả tiền thuê\) là mong muốn,

  e rằng các tài khoản được nạp không thường xuyên sẽ được đi miễn phí

### Hướng dẫn hệ thống thu tiền thuê

Việc thu tiền thuê thông qua một hệ thống hướng dẫn đã được xem xét, vì nó sẽ phân phối tiền thuê cho các node đang hoạt động và có tỷ trọng-stake và được thực hiện theo từng bước. Tuy nhiên:

- nó sẽ ảnh hưởng xấu đến thông lượng mạng
- nó sẽ yêu cầu cách viết hoa đặc biệt trong thời gian chạy, vì các tài khoản không phải là chủ sở hữu Chương trình Hệ thống có thể bị ghi nợ theo hướng dẫn này
- ai đó sẽ phải phát hành các giao dịch
