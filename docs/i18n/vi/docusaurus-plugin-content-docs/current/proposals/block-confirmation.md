---
title: Xác nhận khối
---

Người xác nhận bỏ phiếu cho một hàm băm PoH cho hai mục đích. Đầu tiên, cuộc bỏ phiếu cho thấy nó tin rằng sổ cái có hiệu lực cho đến thời điểm đó. Thứ hai, vì nhiều fork hợp lệ có thể tồn tại ở một độ cao nhất định, nên cuộc bỏ phiếu cũng chỉ ra sự hỗ trợ độc quyền cho fork. Tài liệu này chỉ mô tả trước đây. Sau đó được mô tả trong [Tower BFT](../implemented-proposals/tower-bft.md).

## Thiết kế hiện hành

Để bắt đầu bỏ phiếu, trước tiên validator đăng ký một tài khoản mà nó sẽ gửi phiếu bầu của mình. Sau đó, nó sẽ gửi phiếu bầu đến tài khoản đó. Phiếu bầu chứa chiều cao đánh dấu của khối mà nó đang biểu quyết. Tài khoản lưu trữ 32 chiều cao lớn nhất.

### Sự cố

- Chỉ validator mới biết cách trực tiếp tìm phiếu bầu của chính mình.

  Các thành phần khác, chẳng hạn như thành phần tính toán thời gian xác nhận, cần được đưa vào code validator. Code validator truy vấn ngân hàng cho tất cả các tài khoản thuộc sở hữu của chương trình bỏ phiếu.

- Phiếu bầu không chứa hàm băm PoH. Validator chỉ biểu quyết rằng nó đã quan sát thấy một khối tùy ý ở một số độ cao.

- Các lá phiếu bầu cử không chứa hàm băm của trạng thái ngân hàng. Nếu không có hàm băm đó, không có bằng chứng cho thấy validator đã thực hiện các giao dịch và xác minh rằng không có chi tiêu kép.

## Thiết kế Đề xuất

### Không có trạng thái khối chéo ban đầu

Tại thời điểm một khối được tạo ra, leader sẽ thêm giao dịch NewBlock vào sổ cái với một số mã thông báo đại diện cho phần thưởng xác thực. Nó thực sự là một giao dịch đa ký tự gia tăng gửi mã thông báo từ pool mining đến các validatior. Tài khoản chỉ nên phân bổ đủ không gian để thu thập các phiếu bầu cần thiết để đạt được đa số. Khi một validator quan sát giao dịch NewBlock, nó có tùy chọn gửi một phiếu bầu bao gồm một hàm băm của trạng thái sổ cái của nó (trạng thái ngân hàng). Sau khi tài khoản có đủ phiếu bầu, chương trình bỏ phiếu sẽ phân tán mã thông báo cho những validator, điều này khiến tài khoản bị xóa.

#### Thời gian xác nhận ghi nhật ký

Ngân hàng sẽ cần phải biết về chương trình bỏ phiếu. Sau mỗi giao dịch, cần kiểm tra xem đó có phải là giao dịch bỏ phiếu hay không và nếu có, hãy kiểm tra trạng thái của tài khoản đó. Nếu giao dịch đạt được phần lớn, thì nó sẽ ghi lại thời gian kể từ khi giao dịch NewBlock được gửi.

### Số tiền cuối cùng và các khoản thanh toán

[Tower BFT](../implemented-proposals/tower-bft.md) là thuật toán lựa chọn fork được đề xuất. Nó đề xuất rằng việc thanh toán cho các thợ đào được hoãn lại cho đến khi _stack_ phiếu của validator đạt đến một độ sâu nhất định, tại thời điểm đó, việc hoàn trả là không khả thi về mặt kinh tế. Do đó, chương trình bình chọn có thể thực hiện Tower BFT. Hướng dẫn bỏ phiếu sẽ cần tham chiếu đến tài khoản Tower toàn cầu để tài khoản này có thể theo dõi trạng thái khối-chéo.

## Những thách thức

### Bỏ phiếu trên chuỗi

Sử dụng các chương trình và tài khoản để thực hiện điều này là một chút tẻ nhạt. Phần khó nhất là tìm ra bao nhiêu không gian để phân bổ trong NewBlock. Hai biến là _bộ hoạt động_ và các stake của những validator đó. Nếu chúng tôi tính toán bộ hoạt động tại thời điểm NewBlock được gửi, số lượng các validator thực để phân bổ không gian được biết trước. Tuy nhiên, nếu chúng tôi cho phép những validator mới bỏ phiếu cho các khối cũ, thì chúng tôi sẽ cần một cách để phân bổ không gian động.

Về tinh thần tương tự, nếu leader lưu trữ stake tại thời điểm NewBlock, chương trình bỏ phiếu không cần phải tương tác với ngân hàng khi nó xử lý phiếu bầu. Nếu không, thì chúng tôi có tùy chọn cho phép stake trôi nổi cho đến khi một phiếu bầu được gửi đi. Một validator có thể liên tưởng đến tài khoản staking của chính mình, nhưng đó sẽ là giá trị tài khoản hiện tại thay vì giá trị tài khoản của trạng thái ngân hàng được tổng kết gần đây nhất. Ngân hàng hiện không cung cấp phương tiện để tham chiếu tài khoản từ các thời điểm cụ thể.

### Ý nghĩa biểu quyết đối với các khối trước đó

Một cuộc bỏ phiếu cho một độ cao có ngụ ý một cuộc bỏ phiếu cho tất cả các khối có độ cao thấp hơn của fork đó không? Nếu đúng như vậy, chúng tôi sẽ cần một cách để tra cứu tài khoản của tất cả các khối chưa đạt đến khối siêu lớn. Nếu không, validator có thể gửi phiếu bầu cho tất cả các khối một cách rõ ràng để nhận phần thưởng khối.
