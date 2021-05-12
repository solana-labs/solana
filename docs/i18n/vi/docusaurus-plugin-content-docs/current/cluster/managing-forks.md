---
title: Quản lý Fork
---

Sổ cái được phép fork tại các ranh giới slot. Cấu trúc dữ liệu kết quả tạo thành một cây được gọi là _blockstore_. Khi validator thông dịch blockstore, nó phải duy trì trạng thái cho từng fork trong chuỗi. Chúng tôi gọi mỗi trường hợp này là _fork hoạt động_. Một validator có trách nhiệm cân nhắc những fork đó, để cuối cùng nó có thể chọn một fork.

Một validator lựa chọn một đợt fork bằng cách gửi phiếu bầu cho một slot leader trên đợt fork đó. Biểu quyết cam kết validator trong một khoảng thời gian được gọi là _lockout period_. Validator không được phép bỏ phiếu về một fork khác cho đến khi lockout đó hết hạn. Mỗi lần bỏ phiếu tiếp theo trên cùng một đợt fork sẽ tăng gấp đôi lockout. Sau khi một số phiếu bầu được định cấu hình theo cụm \(hiện tại là 32\), độ dài của khoảng thời gian khóa đạt đến mức được gọi là _max lockout_. Cho đến khi đạt đến lockout tối đa, validator có tùy chọn đợi cho đến khi lockout kết thúc và sau đó bỏ phiếu cho một đợt fork khác. Khi nó bỏ phiếu trên một fork khác, nó thực hiện một hoạt động được gọi là _rollback_, theo đó trạng thái quay ngược thời gian đến một trạm kiểm soát được chia sẻ và sau đó nhảy về phía đầu của fork mà nó vừa bỏ phiếu. Khoảng cách tối đa mà một fork có thể quay trở lại được gọi là _rollback depth_. Rollback depth là số phiếu cần thiết để đạt được lockout tối đa. Bất cứ khi nào một validator bỏ phiếu, bất kỳ điểm kiểm tra nào vượt quá rollback depth sẽ không thể truy cập được. Có nghĩa là, không có tình huống nào mà validator sẽ cần phải quay lại quá rollback depth. Do đó, nó có thể _prune_ một cách an toàn các fork không thể truy cập và _squash_ tất cả các điểm kiểm tra vượt quá rollback depth vào điểm kiểm tra gốc.

## Các fork đang hoạt động

Một fork đang hoạt động là một chuỗi các điểm kiểm tra có chiều dài dài hơn ít nhất một lần so với rollback depth. Fork ngắn nhất sẽ có chiều dài dài hơn rollback depth chính xác một lần. Ví dụ:

![Các fork](/img/forks.svg)

Các chuỗi sau đây là _các fork hoạt động_:

- {4, 2, 1}
- {5, 2, 1}
- {6, 3, 1}
- {7, 3, 1}

## Pruning và Squashing

Một validator có thể bỏ phiếu cho bất kỳ điểm kiểm tra nào trong cây. Trong sơ đồ trên, đó là mọi node ngoại trừ các lá của cây. Sau khi bỏ phiếu, validator sẽ lược bỏ các node phân nhánh từ một khoảng cách xa hơn rollback depth và sau đó tận dụng cơ hội để giảm thiểu việc sử dụng bộ nhớ của nó bằng cách thu gọn bất kỳ node nào mà nó có thể đưa vào thư mục gốc.

Bắt đầu từ ví dụ trên, với một rollback depth của 2, hãy xem xét một phiếu bầu trên 5 so với một phiếu trên 6. Đầu tiên, một cuộc bỏ phiếu trên 5:

![Các fork sau khi pruning](/img/forks-pruned.svg)

Gốc mới là 2 và bất kỳ các fork nào đang hoạt động không phải là con cháu từ 2 sẽ bị cắt bớt.

Ngoài ra, một phiếu bầu trên 6:

![Các fork](/img/forks-pruned2.svg)

Cây vẫn có gốc là 1, vì fork hoạt động bắt đầu từ 6 chỉ cách gốc 2 điểm kiểm tra.
