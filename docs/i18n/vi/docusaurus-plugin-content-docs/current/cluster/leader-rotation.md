---
title: Vòng xoay leader
---

Tại bất kỳ thời điểm nào, một cụm chỉ mong đợi một validator tạo các mục nhập sổ cái. Bởi chỉ có một leader tại một thời điểm, tất cả các validator đều có thể phát lại các bản sao giống hệt nhau của sổ cái. Tuy nhiên, hạn chế của việc chỉ một leader tại một thời điểm là một leader độc hại có khả năng kiểm duyệt các phiếu bầu và giao dịch. Vì không thể phân biệt được việc kiểm duyệt với việc mạng bỏ các gói tin, cụm không thể chỉ chọn một node duy nhất để giữ vai trò leader vô thời hạn. Thay vào đó, cụm giảm thiểu ảnh hưởng của một leader độc hại bằng cách xoay node nào dẫn đầu.

Mỗi validator chọn leader dự kiến ​​bằng cách sử dụng cùng một thuật toán, được mô tả bên dưới. Khi validator nhận được một mục nhập sổ cái mới đã ký, có thể chắc chắn rằng một mục nhập đã được tạo bởi leader dự kiến. Thứ tự của các slot mà mỗi leader được chỉ định một slot được gọi là một _lịch trình của leader_.

## Vòng xoay lịch trình của leader

Validator từ chối các khối không có chữ ký bởi _slot leader_. Danh sách danh tính của tất cả các slot leader được gọi là lịch _lịch trình của leader_. Lịch trình của leader được tính toán lại cục bộ và định kỳ. Nó chỉ định các slot leader trong một khoảng thời gian được gọi là _kỷ nguyên_. Lịch trình phải được tính toán trước các slot mà nó chỉ định, sao cho trạng thái sổ cái mà nó sử dụng để tính toán lịch trình được hoàn tất. Khoảng thời gian đó được gọi là _lịch bù của leader_. Solana đặt độ lệch thành khoảng thời gian của các slot cho đến kỷ nguyên tiếp theo. Nghĩa là, lịch trình của leader cho một kỷ nguyên được tính toán từ trạng thái sổ cái vào đầu kỷ nguyên trước đó. Sự bù đắp của một kỷ nguyên là khá tùy ý và được giả định là đủ dài để tất cả các validator sẽ hoàn thành trạng thái sổ cái của họ trước khi lịch trình tiếp theo được tạo. Một cụm có thể chọn rút ngắn thời gian bù để giảm thời gian giữa các thay đổi stake và cập nhật lịch trình của leader.

Trong khi hoạt động mà không có phân vùng kéo dài hơn một kỷ nguyên, lịch trình chỉ cần được tạo khi fork gốc vượt qua ranh giới kỷ nguyên. Vì lịch trình là cho kỷ nguyên tiếp theo, bất kỳ stake mới nào được cam kết với fork gốc sẽ không hoạt động cho đến kỷ nguyên tiếp theo. Khối được sử dụng để tạo lịch trình của leader là khối đầu tiên vượt qua ranh giới kỷ nguyên.

Nếu không có phân vùng kéo dài hơn một kỷ nguyên, cụm sẽ hoạt động như sau:

1. Validator liên tục cập nhật fork gốc của chính nó khi nó bỏ phiếu.
2. Validator cập nhật lịch trình hàng đầu của nó mỗi khi chiều cao slot vượt qua ranh giới kỷ nguyên.

Ví dụ:

Thời lượng kỷ nguyên là 100 slot. Fork gốc được cập nhật từ fork được tính toán ở độ cao slot 99 thành một fork được tính toán ở độ cao slot 102. Các fork có slot ở độ cao 100, 101 đã bị bỏ qua do lỗi. Lịch trình của leader mới được tính bằng cách sử dụng fork ở độ cao slot 102. Nó hoạt động từ slot 200 cho đến khi được cập nhật lại.

Không có sự mâu thuẫn nào có thể tồn tại bởi vì mọi validator đang bỏ phiếu với cụm đã bỏ qua 100 và 101 khi gốc của nó vượt qua 102. Tất cả validator, bất kể hình thức biểu quyết, sẽ cam kết với gốc là 102 hoặc con của 102.

### Vòng xoay lịch trình của leader với các phân vùng theo quy mô của Epoch.

Khoảng thời gian lịch trình của leader có mối quan hệ trực tiếp đến khả năng một nhóm có cái nhìn không nhất quán về lịch trình của leader chính xác.

Hãy xem xét tình huống sau:

Hai phân vùng đang tạo ra một nửa số khối mỗi phân vùng. Cả hai đều không đi đến một fork siêu đa số dứt khoát. Cả hai sẽ vượt qua kỷ nguyên 100 và 200 mà không thực sự cam kết gốc và do đó cam kết toàn cụm đối với lịch leader mới.

Trong trường hợp không ổn định này, tồn tại nhiều lịch trình hợp lệ của leader.

- Một lịch trình của leader được tạo cho mọi fork có công ty mẹ trực tiếp trong kỷ nguyên trước đó.
- Lịch trình của leader có hiệu lực sau khi bắt đầu kỷ nguyên tiếp theo cho các fork con cho đến khi nó được cập nhật.

Lịch trình của mỗi phân vùng sẽ khác nhau sau khi phân vùng kéo dài hơn một kỷ nguyên. Vì lý do này, thời lượng kỷ nguyên nên được chọn lớn hơn nhiều so với thời gian slot và độ dài dự kiến ​​để một fork được cam kết đến gốc.

Sau khi quan sát cụm trong một khoảng thời gian đủ, độ lệch lịch trình của leader có thể được chọn dựa trên thời lượng phân vùng trung bình và độ lệch chuẩn của nó. Ví dụ, một khoảng chênh lệch lâu hơn thì thời lượng phân vùng trung bình cộng với sáu độ lệch chuẩn sẽ làm giảm khả năng có lịch trình sổ cái không nhất quán trong cụm xuống còn 1 trên 1 triệu.

## Tạo lịch trình lãnh đạo tại Genesis

Cấu hình genesis khai báo leader đầu tiên cho kỷ nguyên đầu tiên. Leader này kết thúc được lên lịch cho hai kỷ nguyên đầu tiên vì lịch biểu của leader cũng được tạo ở slot 0 cho kỷ nguyên tiếp theo. Độ dài của hai kỷ nguyên đầu tiên cũng có thể được chỉ định trong cấu hình genesis. Độ dài tối thiểu của kỷ nguyên đầu tiên phải lớn hơn hoặc bằng độ sâu khôi phục tối đa như được xác định trong [Tower BFT](../implemented-proposals/tower-bft.md).

## Thuật toán tạo lịch trình leader

Lịch trình leader được tạo bằng cách sử dụng một hạt giống xác định trước. Quá trình này như sau:

1. Định kỳ sử dụng chiều cao đánh dấu PoH \(một bộ đếm tăng đơn điệu\) để tạo ra một thuật toán giả-ngẫu nhiên ổn định.
2. Ở độ cao đó, hãy lấy mẫu ngân hàng cho tất cả các tài khoản đã stake có danh tính leader đã bỏ phiếu trong một số lượng đánh dấu được định cấu hình theo cụm. Mẫu được gọi là _tập hợp hoạt động_.
3. Sắp xếp tập hợp hoạt động theo tỉ trọng stake.
4. Sử dụng hạt giống ngẫu nhiên để chọn các node được tính theo tỷ trọng stake để tạo một thứ tự theo tỷ trọng stake.
5. Thứ tự này trở nên hợp lệ sau một số lượng đánh dấu được cấu hình theo cụm.

## Lập lịch các vectơ tấn công

### Hạt giống

Hạt giống được chọn là có thể dự đoán được nhưng không thể dự đoán được. Không có cuộc tấn công tàn phá nào ảnh hưởng đến kết quả của nó.

### Tập hợp hoạt động

Một leader có thể thiên vị tập hợp hoạt động bằng cách kiểm duyệt phiếu bầu của validator. Có hai cách khả thi để các leader kiểm duyệt tập hợp đang hoạt động:

- Bỏ qua phiếu bầu từ các validator
- Từ chối bỏ phiếu cho các khối có phiếu bầu từ các validator

Để giảm khả năng bị kiểm duyệt, tập hợp hoạt động được tính toán ở ranh giới bù đắp lịch trình lãnh đạo trên một _Khoảng thời gian lấy mẫu tập hợp hoạt động_. Khoảng thời gian lấy mẫu tập hợp hoạt động đủ dài để các phiếu bầu sẽ được thu thập bởi nhiều leader.

### Staking

Các leader có thể kiểm duyệt các giao dịch staking mới hoặc từ chối xác thực các khối với số tiền stake mới. Cuộc tấn công này tương tự như việc kiểm duyệt phiếu bầu của validator.

### Mất chìa khóa hoạt động của validator

Các leader và những validator phải sử dụng các chìa khóa tạm thời để hoạt động và chủ sở hữu stake ủy quyền cho những validator thực hiện công việc với stake của họ thông qua ủy quyền.

Cụm sẽ có thể khôi phục sau khi mất tất cả các chìa khóa tạm thời được sử dụng bởi các leader và các validator, điều này có thể xảy ra thông qua một lỗ hổng phần mềm chung được chia sẻ bởi tất cả các node. Chủ sở hữu stake có thể bỏ phiếu trực tiếp bằng cách đồng ký vào một phiếu bầu của một validator mặc dù stake hiện được ủy quyền cho một validator.

## Mục nhập sắp xếp

Thời gian tồn tại của lịch trình của leader được gọi là _kỷ nguyên_. Kỷ nguyên được chia thành các _slot_, trong đó mỗi slot có thời hạn là các đánh dấu PoH `T`.

Một leader truyền các mục nhập trong slot của nó. Sau khi đánh dấu `T`, tất cả các validator chuyển sang leader được lên lịch tiếp theo. Các validator phải bỏ qua các mục nhập được gửi bên ngoài slot được chỉ định của leader.

Tất cả các đánh dấu `T` phải được quan sát bởi leader tiếp theo để nó xây dựng các mục của riêng mình trên đó. Nếu các mục nhập không được quan sát \(leader bị thất bại\) hoặc các mục nhập không hợp lệ \(leader bị lỗi hoặc độc hại\), leader tiếp theo phải tạo ra các đánh dấu để lấp đầy slot của leader trước đó. Lưu ý rằng leader tiếp theo nên thực hiện các yêu cầu sửa chữa song song và hoãn việc gửi đánh dấu cho đến khi chắc chắn rằng những validator khác cũng không theo dõi được các mục nhập của leader trước đó. Nếu một leader xây dựng sai trên các đánh dấu của riêng mình, leader theo sau nó phải thay thế tất cả các đánh dấu của nó.
