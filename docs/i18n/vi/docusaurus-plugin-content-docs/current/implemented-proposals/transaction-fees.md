---
title: Phí giao dịch xác định
---

Các giao dịch hiện tại bao gồm trường phí cho biết trường phí tối đa mà một slot leader được phép tính phí để xử lý giao dịch. Mặt khác, cụm đồng ý về một khoản phí tối thiểu. Nếu mạng bị tắc nghẽn, slot leader có thể ưu tiên các giao dịch cung cấp phí cao hơn. Điều đó có nghĩa là khách hàng sẽ không biết số tiền đã được thu thập cho đến khi giao dịch được xác nhận bởi cụm và số dư còn lại được kiểm tra. Nó có mùi chính xác những gì chúng tôi không thích về "gas", không xác định của Ethereum.

## Phí do tắc nghẽn

Mỗi validator sử dụng _chữ ký trên mỗi slot_ \ (SPS \) để ước tính tắc nghẽn mạng và _mục tiêu SPS_ để ước tính khả năng xử lý mong muốn của cụm. Validator học mục tiêu SPS từ cấu hình genesis, trong khi nó tính toán SPS từ các giao dịch được xử lý gần đây. Cấu hình genesis cũng xác định mục tiêu `lamports_per_signature`, là phí tính cho mỗi chữ ký khi cụm đang hoạt động tại_mục tiêu SPS_.

## Tính phí

Khách hàng sử dụng JSON RPC API để truy vấn cụm về các tham số phí hiện tại. Các tham số đó được gắn thẻ bằng blockhash và vẫn có hiệu lực cho đến khi blockhash đó đủ cũ để bị slot leader từ chối.

Trước khi gửi giao dịch đến cụm, khách hàng có thể gửi dữ liệu tài khoản giao dịch và phí tới mô-đun SDK có tên là _máy tính phí_. Miễn là phiên bản SDK của khách hàng khớp với phiên bản của slot leader, khách hàng được đảm bảo rằng tài khoản của họ sẽ được thay đổi chính xác cùng số lamport mà máy tính phí trả lại.

## Tham số phí

Trong lần triển khai đầu tiên của thiết kế này, tham số phí duy nhất là `lamports_per_signature`. Cụm càng nhiều chữ ký cần xác minh thì phí càng cao. Số lượng lamport chính xác được xác định bằng tỷ lệ SPS so với mục tiêu SPS. Ở cuối mỗi slot, cụm giảm xuống `lamports_per_signature` khi SPS ở dưới mục tiêu và tăng lên khi ở trên mục tiêu. Giá trị tối thiểu cho `lamports_per_signature` là 50% mục tiêu `lamports_per_signature` và giá trị tối đa là 10 lần mục tiêu \`lamports_per_signature'

Các tham số trong tương lai có thể bao gồm:

- `lamports_per_pubkey` - chi phí để tải tài khoản
- `lamports_per_slot_distance` - chi phí cao hơn để tải các tài khoản rất cũ
- `lamports_per_byte` - chi phí cho mỗi kích thước của tài khoản được tải
- `lamports_per_bpf_instruction` - chi phí để chạy một chương trình

## Các cuộc tấn công

### Đánh cắp mục tiêu SPS

Một nhóm validator có thể tập trung hóa cụm nếu họ có thể thuyết phục nó nâng Mục tiêu SPS lên trên một điểm mà các validator còn lại có thể theo kịp. Việc nâng mục tiêu sẽ khiến phí giảm, có thể tạo ra nhiều nhu cầu hơn và do đó TPS cao hơn. Nếu validator không có phần cứng có thể xử lý nhiều giao dịch nhanh như vậy, thì các phiếu xác nhận của nó cuối cùng sẽ dài đến mức cụm sẽ buộc phải khởi động nó.
