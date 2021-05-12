---
title: Cấu trúc tài khoản Stake
---

Tài khoản stake trên Solana có thể được sử dụng để ủy quyền mã thông báo cho các validator trên mạng để có thể kiếm được phần thưởng cho chủ sở hữu của tài khoản stake. Tài khoản stake được tạo và quản lý khác với địa chỉ ví truyền thống, được gọi là *system account*.  Tài khoản hệ thống chỉ có thể gửi và nhận SOL từ các tài khoản khác trên mạng, trong khi tài khoản stake hỗ trợ các hoạt động phức tạp hơn để quản lý ủy quyền mã thông báo.

Tài khoản stake trên Solana cũng hoạt động khác so với các tài khoản của các mạng blockchain Proof-of-Stake khác mà bạn đã quen thuộc.  Tài liệu này mô tả cấu trúc cao cấp và các chức năng của tài khoản stake Solana.

#### Địa Chỉ Tài Khoản
Mỗi tài khoản stake có một địa chỉ duy nhất, có thể được sử dụng để tra cứu thông tin tài khoản trong dòng lệnh hoặc trong bất kỳ công cụ khám phá mạng.  Tuy nhiên, không giống như địa chỉ ví mà người nắm giữ keypair của địa chỉ kiểm soát ví, keypair được liên kết với địa chỉ tài khoản stake không nhất thiết phải có bất kỳ quyền kiểm soát nào đối với tài khoản.  Trên thực tế, một keypair hoặc private key thậm chí có thể không tồn tại cho địa chỉ của tài khoản stake.

Lần duy nhất địa chỉ của tài khoản stake có tệp keypair là khi [creating a stake account using the command line tools](../cli/delegate-stake.md#create-a-stake-account), một tệp keypair mới được tạo trước tiên chỉ để đảm bảo rằng tài khoản stake là mới và duy nhất.

#### Tìm hiểu các chức trách tài khoản
Một số loại tài khoản nhất định có thể có một hoặc nhiều *signing authorities* được liên kết với một tài khoản nhất định. Cơ quan tài khoản được sử dụng để ký các giao dịch nhất định cho tài khoản mà cơ quan này kiểm soát.  Điều này khác với một số mạng blockchain khác trong đó chủ sở hữu keypair được liên kết với địa chỉ của tài khoản kiểm soát tất cả hoạt động của tài khoản.

Mỗi tài khoản stake có hai cơ quan ký tên được chỉ định theo địa chỉ tương ứng của họ, mỗi cơ quan được ủy quyền để thực hiện các hoạt động nhất định trên tài khoản stake.

*stake authority* được sử dụng để ký giao dịch cho các hoạt động sau:
 - Ủy quyền stake
 - Hủy kích hoạt ủy quyền stake
 - Tách tài khoản stake, tạo một tài khoản stake mới với một phần tiền trong tài khoản đầu tiên
 - Hợp nhất hai tài khoản stake không giảm giá trị thành một
 - Đặt cơ quan quyền sở hữu stake mới

*withdraw authority* ký các giao dịch sau:
 - Rút stake chưa được ủy quyền vào một địa chỉ ví
 - Đặt thẩm quyền rút tiền mới
 - Đặt thẩm quyền stake mới

Thẩm quyền stake và thẩm quyền rút tiền được đặt khi tài khoản stake được tạo và chúng có thể được thay đổi để cho phép địa chỉ ký mới bất kỳ lúc nào. Thẩm quyền stake và rút tiền có thể là cùng một địa chỉ hoặc hai địa chỉ khác nhau.

Keypair thẩm quyền rút tiền giữ quyền kiểm soát nhiều hơn đối với tài khoản vì nó cần thiết để thanh lý các mã thông báo trong tài khoản stake và có thể được sử dụng để đặt lại thẩm quyền stake nếu keypair thẩm quyền stake bị mất hoặc bị xâm phạm.

Đảm bảo thẩm quyền rút tiền chống lại mất mát hoặc trộm cắp là điều quan trọng khi quản lý tài khoản stake.

#### Nhiều ủy quyền
Mỗi tài khoản stake chỉ có thể được sử dụng để ủy quyền cho một validator tại một thời điểm. Tất cả các mã thông báo trong tài khoản đều được ủy quyền hoặc không được ủy quyền, hoặc đang trong quá trình được ủy quyền hoặc không được ủy quyền.  Để ủy quyền một phần nhỏ mã thông báo của bạn cho một validator hoặc ủy quyền cho nhiều validator, bạn phải tạo nhiều tài khoản stake.

Điều này có thể được thực hiện bằng cách tạo nhiều tài khoản stake từ một địa chỉ ví có chứa mã thông báo hoặc bằng cách tạo một tài khoản stake lớn duy nhất và sử dụng stake authority để chia tài khoản thành nhiều tài khoản với số dư mã thông báo bạn chọn.

Có thể chỉ định cùng một thẩm quyền stake và rút tiền cho nhiều tài khoản stake.

Hai tài khoản stake không được ủy quyền và có cùng quyền hạn và khóa có thể được hợp nhất thành một tài khoản stake duy nhất.

#### Khởi động và hồi chiêu ủy quyền
Khi một tài khoản stake được ủy quyền, hoặc một ủy quyền bị hủy kích hoạt, hoạt động sẽ không có hiệu lực ngay lập tức.

Việc ủy ​​quyền hoặc hủy kích hoạt mất vài [epochs](../terminology.md#epoch) để hoàn thành, với một phần nhỏ ủy quyền trở nên hoạt động hoặc không hoạt động ở mỗi ranh giới kỷ nguyên sau khi giao dịch chứa các hướng dẫn đã được gửi đến cụm.

Ngoài ra còn có giới hạn về tổng số stake có thể được ủy quyền hoặc ngừng hoạt động trong một kỷ nguyên duy nhất, để ngăn chặn những thay đổi lớn đột ngột về stake trên toàn mạng. Vì khởi động và thời gian hồi chiêu phụ thuộc vào hành vi của những người tham gia mạng khác, nên rất khó dự đoán thời gian chính xác của chúng. Thông tin chi tiết về thời gian khởi động và thời gian hồi chiêu có thể xem [tại đây](../cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal).

#### Khóa
Tài khoản stake có thể bị khóa để ngăn không cho các mã thông báo họ nắm giữ bị rút trước khi đạt đến một ngày hoặc kỷ nguyên cụ thể.  Trong khi bị khóa, tài khoản stake vẫn có thể được ủy quyền, không được ủy quyền hoặc chia tách, và các quyền hạn về stake và rút tiền của nó có thể thực hiện như bình thường.  Không được phép rút tiền vào địa chỉ ví.

Khóa chỉ có thể được thêm vào khi tài khoản tiền cược được tạo lần đầu tiên, nhưng nó có thể được sửa đổi sau đó, bởi *lockup authority* hoặc *custodian*, địa chỉ của nó cũng được đặt khi tài khoản được tạo.

#### Hủy Tài Khoản Stake
Giống như các loại tài khoản khác trên mạng Solana, tài khoản stake có số dư là 0 SOL sẽ không còn được theo dõi.  Nếu một tài khoản stake không được ủy quyền và tất cả các mã thông báo trong đó được rút đến một địa chỉ ví, thì tài khoản tại địa chỉ đó sẽ bị phá hủy và sẽ cần được tạo lại theo cách thủ công để sử dụng lại địa chỉ đó.

#### Xem Tài Khoản Stake
Bạn có thể xem chi tiết tài khoản stake trên Solana Explorer bằng cách sao chép và dán địa chỉ tài khoản vào thanh tìm kiếm.
 - http://explorer.solana.com/accounts
