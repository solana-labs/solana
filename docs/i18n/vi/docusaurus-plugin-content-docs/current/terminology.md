---
title: Thuật ngữ
---

Các thuật ngữ sau đây được sử dụng trong toàn bộ tài liệu.

## tài khoản

Một tệp liên tục được giải quyết bằng [public key](terminology.md#public-key) và với [các lamport](terminology.md#lamport) theo dõi thời gian tồn tại của nó.

## ứng dụng

Một ứng dụng giao diện người dùng tương tác với một cụm Solana.

## trạng thái ngân hàng

Kết quả của việc diễn giải tất cả các chương trình trên sổ cái ở một [chiều cao đánh dấu](terminology.md#tick-height) nhất định. Nó bao gồm ít nhất một tập hợp tất cả [các tài khoản](terminology.md#account) nắm giữ [các mã thông báo gốc](terminology.md#native-tokens).

## khối

Một tập hợp các [mục nhập](terminology.md#entry) liền kề trên sổ cái được bao phủ bởi [phiếu bầu](terminology.md#ledger-vote). Một [leader](terminology.md#leader) tạo ra nhiều nhất một khối cho mỗi [slot](terminology.md#slot).

## blockhash

Một [ hàm băm](terminology.md#hash) kháng trước hình ảnh của [sổ cái](terminology.md#ledger) tại một [chiều cao khối](terminology.md#block-height). Lấy từ [id mục nhập](terminology.md#entry-id) cuối cùng trong slot

## chiều cao khối

Số lượng [khối](terminology.md#block) bên dưới khối hiện tại. Khối đầu tiên sau [khối genesis](terminology.md#genesis-block) có chiều cao là một.

## validator bootstrap

[validator](terminology.md#validator) đầu tiên tạo ra một [khối](terminology.md#block).

## Khối CBC

Đoạn sổ cái được mã hóa nhỏ nhất, một đoạn sổ cái được mã hóa sẽ được tạo từ nhiều khối CBC. chính xác là `ledger_segment_size / cbc_block_size`.

## khách hàng

Một [node](terminology.md#node) sử dụng [cụm](terminology.md#cluster).

## cụm

Một tập hợp [các validator](terminology.md#validator) duy trì một [sổ cái](terminology.md#ledger).

## thời gian xác nhận

Khoảng thời gian wallclock giữa một [leader](terminology.md#leader) tạo một [đánh dấu mục nhập](terminology.md#tick) và tạo một [khối xác nhận](terminology.md#confirmed-block).

## khối đã xác nhận

Một [khối](terminology.md#block) đã nhận được một [siêu đa số](terminology.md#supermajority) của [các phiếu bầu sổ cái](terminology.md#ledger-vote) với cách giải thích sổ cái phù hợp với thông tin của các leader.

## control plane

Mạng gossip kết nối tất cả [các node](terminology.md#node) của [cluster](terminology.md#cluster).

## thời gian hồi chiêu

Một số [kỷ nguyên](terminology.md#epoch) sau khi [stake](terminology.md#stake) đã bị vô hiệu hóa trong khi dần dần có sẵn để rút tiền. Trong thời gian này, stake được coi là "ngừng hoạt động". Thông tin thêm về: [khởi động và thời gian hồi chiêu](implemented-proposals/staking-rewards.md#stake-warmup-cooldown-withdrawal)

## tín dụng

Xem [tín dụng bỏ phiếu](terminology.md#vote-credit).

## mặt phẳng dữ liệu

Mộ mạng đa hướng được sử dụng để xác thực hiệu quả [các điểm nhập](terminology.md#entry) và đạt được sự đồng thuận.

## máy bay không người lái

Một dịch vụ ngoài chuỗi hoạt động như một người giám sát private key của người dùng. Nó thường dùng để xác thực và ký kết các giao dịch.

## mục nhập

Mục nhập trên [sổ cái](terminology.md#ledger) là [đánh dấu](terminology.md#tick) hoặc một [mục nhập giao dịch](terminology.md#transactions-entry).

## id mục nhập

Một [hàm băm](terminology.md#hash) có khả năng kháng trước hình ảnh trên nội dung cuối cùng của mục nhập, đóng vai trò là mã nhận dạng duy nhất trên toàn cầu của [các mục nhập](terminology.md#entry). Hàm băm đóng vai trò là bằng chứng về:

- Mục nhập được tạo sau một khoảng thời gian
- [các giao dịch](terminology.md#transaction) được chỉ định là những giao dịch được bao gồm trong mục nhập
- Vị trí của mục này so với các mục khác trong [sổ cái](terminology.md#ledger)

Xem [Proof of History](terminology.md#proof-of-history).

## kỷ nguyên

Thời gian, tức là số lượng [slot](terminology.md#slot) mà một [lịch trình của leader](terminology.md#leader-schedule) là hợp lệ.

## phí tài khoản

Phí tài khoản trong giao dịch là tài khoản phải thanh toán chi phí bao gồm cả giao dịch trên sổ cái. Đây là tài khoản đầu tiên trong giao dịch. Tài khoản này phải được khai báo là Đọc-Ghi (có thể ghi) trong giao dịch vì việc thanh toán cho giao dịch làm giảm số dư tài khoản.

## tính chất dứt khoát

Khi các node đại diện cho 2/3 của [stake](terminology.md#stake) có chung một [gốc](terminology.md#root).

## fork

Một [sổ cái](terminology.md#ledger) bắt nguồn từ các mục nhập chung nhưng sau đó được phân kỳ.

## khối genesis

[block](terminology.md#block) đầu tiên trong chuỗi.

## cấu hình genesis

Tệp cấu hình chuẩn bị [sổ cái](terminology.md#ledger) cho [khối genesis](terminology.md#genesis-block).

## hàm băm

Dấu vân tay kỹ thuật số của một chuỗi các byte.

## lạm phát

Sự gia tăng nguồn cung mã thông báo theo thời gian được sử dụng để tài trợ phần thưởng cho việc xác thực và tài trợ cho sự phát triển liên tục của Solana.

## hướng dẫn

Đơn vị nhỏ nhất của chương trình mà khách hàng có thể đưa vào giao dịch. Đơn vị nhỏ nhất của [chương trình](terminology.md#program) mà [khách hàng](terminology.md#client) có thể bao gồm trong [giao dịch](terminology.md#transaction).

## keypair

Một [public key](terminology.md#public-key) và [private key](terminology.md#private-key) tương ứng.

## lamport

Một [mã thông báo gốc](terminology.md#native-token) phân đoạn có giá trị là 0.000000001 [sol](terminology.md#sol).

## leader

Vai trò của [validator](terminology.md#validator) khi nó thêm [các mục nhập](terminology.md#entry) vào [sổ cái](terminology.md#ledger).

## lịch trình leader

Một chuỗi [các public key](terminology.md#public-key) của [validator](terminology.md#validator)/. Cụm sử dụng lịch trình leader để xác định validator nào là [leader](terminology.md#leader) tại bất kỳ lúc nào.

## sổ cái

Một danh sách [các mục nhập](terminology.md#entry) chứa [các giao dịch](terminology.md#transaction) được ký bởi [các khách hàng](terminology.md#client). Về mặt khái niệm, điều này có thể được truy ngược trở lại [khối genesis](terminology.md#genesis-block), nhưng sổ cái của [các validator](terminology.md#validator) thực tế có thể chỉ có [các khối block](terminology.md#block) mới hơn để tiết kiệm việc sử dụng bộ nhớ vì các khối cũ hơn không cần thiết để xác thực các khối trong tương lai theo thiết kế.

## biểu quyết sổ cái

Một [ hàm băm](terminology.md#hash) của [trạng thái của các validator](terminology.md#bank-state) tại một [chiều cao đánh dấu](terminology.md#tick-height). Nó bao gồm một lời khẳng định của [các validator](terminology.md#validator) rằng một [khối](terminology.md#block) mà nó nhận được đã được xác minh, cũng như lời hứa không bỏ phiếu cho một [khối](terminology.md#block) xung đột \(tức là [fork](terminology.md#fork) \) trong một khoảng thời gian cụ thể, khoảng thời gian [lockout](terminology.md#lockout).

## khách hàng nhẹ

Một loại [khách hàng](terminology.md#client) có thể xác minh rằng nó đang trỏ đến một [cụm](terminology.md#cluster) hợp lệ. Nó thực hiện nhiều xác minh sổ cái hơn một [thin client](terminology.md#thin-client) và ít hơn [validator](terminology.md#validator).

## bộ nạp

Một [chương trình](terminology.md#program) có khả năng diễn giải mã nhị phân của các chương trình trên chuỗi khác.

## khóa

Khoảng thời gian mà một [validator](terminology.md#validator) không thể [bỏ phiếu](terminology.md#ledger-vote) cho một [fork](terminology.md#fork).

## mã thông báo gốc

[Mã thông báo](terminology.md#token) được sử dụng để theo dõi công việc được thực hiện bởi [các node](terminology.md#node) trong một cụm.

## node

Một máy tính tham gia vào một [cụm](terminology.md#cluster).

## số lượng node

Số lượng [validator](terminology.md#validator) tham gia vào một [cụm](terminology.md#cluster).

## PoH

Xem [Proof of History](terminology.md#proof-of-history).

## point

Một khoản [tín dụng](terminology.md#credit) có tỉ trọng trong một chế độ khen thưởng. Trong [chế độ phần thưởng](cluster/stake-delegation-and-rewards.md) của [validator](terminology.md#validator), số điểm có được từ tiền [stake](terminology.md#stake) trong quá trình đổi thưởng là tích số của [bỏ phiếu tín dụng](terminology.md#vote-credit) kiếm được và số lượng lamport được stake.

## private key

Private key của [keypair](terminology.md#keypair).

## chương trình

Code diễn giải [các hướng dẫn](terminology.md#instruction).

## id chương trình

Public key của [tài khoản](terminology.md#account) chứa [chương trình](terminology.md#program).

## Bằng chứng lịch sử

Một chồng các bằng chứng, mỗi bằng chứng chứng minh rằng một số dữ liệu đã tồn tại trước khi bằng chứng được tạo ra và một khoảng thời gian chính xác đã trôi qua trước bằng chứng trước đó. Giống như [VDF](terminology.md#verifiable-delay-function), Proof of History có thể được xác minh trong thời gian ngắn hơn thời gian sản xuất.

## public key

Public key của một [keypair](terminology.md#keypair).

## gốc

Một [khối](terminology.md#block) hoặc [slot](terminology.md#slot) đã đạt đến tối đa [lockout](terminology.md#lockout) trên một [validator](terminology.md#validator). Root là khối cao nhất là tổ tiên của tất cả các fork đang hoạt động trên validator. Tất cả các khối tổ tiên của một gốc cũng là một gốc. Các khối không phải là tổ tiên và không phải là hậu duệ của root bị loại trừ khỏi việc xem xét để có được sự đồng thuận và có thể bị loại bỏ.

## thời gian chạy

Thành phần của [validator](terminology.md#validator) chịu trách nhiệm thực thi [chương trình](terminology.md#program).

## mảnh nhỏ

Một phần nhỏ của một [khối](terminology.md#block); đơn vị nhỏ nhất được gửi giữa các [validator](terminology.md#validator).

## chữ ký

Chữ ký ed25519 64 byte của R (32 byte) và S (32 byte). Với yêu cầu R là điểm Edwards được đóng gói không có bậc nhỏ và S là đại lượng vô hướng trong khoảng 0 <= S < L. Yêu cầu này đảm bảo không có tính dễ uốn chữ ký. Mỗi giao dịch phải có ít nhất một chữ ký cho [ phí tài khoản](terminology#fee-account). Do đó, chữ ký đầu tiên trong giao dịch có thể được coi là [id giao dịch](terminology.md#transaction-id)

## slot

Khoảng thời gian mà [leader](terminology.md#leader) nhập các giao dịch và tạo ra một [khối](terminology.md#block).

## hợp đồng thông minh

Một tập hợp các ràng buộc khi đã thỏa mãn, báo hiệu cho một chương trình rằng một số bản cập nhật tài khoản được xác định trước được cho phép.

## sol

[Mã thông báo gốc](terminology.md#native-token) được theo dõi bởi một [cụm](terminology.md#cluster) được công ty Solana công nhận.

## stake

Mã thông báo sẽ bị loại bỏ đối với [cụm](terminology.md#cluster) nếu hành vi [validator](terminology.md#validator) độc hại có thể được chứng minh.

## siêu đa số

2/3 của một [cụm](terminology.md#cluster).

## sysvar

Một [tài khoản](terminology.md#account) tổng hợp được cung cấp bởi thời gian chạy để cho phép các chương trình truy cập trạng thái mạng như chiều cao đánh dấu hiện tại, các giá trị [điểm](terminology.md#point) thưởng, v.v.

## khách hàng mỏng

Một kiểu [khách hàng](terminology.md#client) tin cậy rằng nó đang giao tiếp với một [cụm](terminology.md#cluster) hợp lệ.

## đánh dấu

Một [mục nhập](terminology.md#entry) sổ cái ước tính thời lượng wallclock.

## chiều cao đánh dấu

Đánh dấu/a> thứ N trong [sổ cái](terminology.md#ledger).</p> 



## mã thông báo

Một khan hiếm, thành viên có thể thay thế của một tập hợp các mã thông báo.



## tps

[Giao dịch](terminology.md#transaction) mỗi giây.



## giao dịch

Một hoặc nhiều [hướng dẫn](terminology.md#instruction) được ký bởi [khách hàng](terminology.md#client) bằng một hoặc nhiều [keypair](terminology.md#keypair) và được thực thi nguyên tử chỉ với hai kết quả có thể xảy ra: thành công hoặc thất bại.



## id giao dịch

[chữ ký](terminology.md#signature) đầu tiên trong [giao dịch](terminology.md#transaction), có thể được sử dụng để xác định duy nhất giao dịch trên [sổ cái](terminology.md#ledger) hoàn chỉnh.



## xác nhận giao dịch

Số lượng [khối được xác nhận](terminology.md#confirmed-block) kể từ khi giao dịch được chấp nhận trên [sổ cái](terminology.md#ledger). Giao dịch được hoàn tất khi khối của nó trở thành [gốc](terminology.md#root).



## nhập giao dịch

Một tập hợp các [giao dịch](terminology.md#transaction) có thể được thực hiện song song.



## người xác thực

Người tham gia đầy đủ trong [ cụm ](terminology.md#cluster) chịu trách nhiệm xác thực [ sổ cái ](terminology.md#ledger) và tạo các [khối](terminology.md#block).



## VDF

Xem [chức năng trì hoãn có thể xác minh](terminology.md#verifiable-delay-function).



## chức năng trì hoãn có thể xác minh

Một hàm cần một khoảng thời gian cố định để thực thi tạo ra bằng chứng rằng nó đã chạy, sau đó có thể được xác minh trong thời gian ngắn hơn so với thời gian tạo ra.



## phiếu bầu

Xem [bỏ phiếu sổ cái](terminology.md#ledger-vote).



## phiếu tín dụng

Một kiểm đếm phần thưởng cho các [validator](terminology.md#validator). Tín dụng biểu quyết được trao cho một validator trong tài khoản bỏ phiếu của họ khi validator đạt được [gốc](terminology.md#root).



## ví

Một bộ sưu tập các [keypair](terminology.md#keypair).



## thời kỳ ấm lên

Một số [kỷ nguyên](terminology.md#epoch) sau khi [stake](terminology.md#stake) đã được ủy quyền trong khi nó dần dần có hiệu lực. Trong thời gian này, tiền stake được coi là "kích hoạt". Thông tin thêm về: [khởi động và thời gian hồi chiêu](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
