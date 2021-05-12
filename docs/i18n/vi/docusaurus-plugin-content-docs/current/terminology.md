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

A sequence of [validator](terminology.md#validator) [public keys](terminology.md#public-key) mapped to [slots](terminology.md#slot). Cụm sử dụng lịch trình leader để xác định validator nào là [leader](terminology.md#leader) tại bất kỳ lúc nào.

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

## skipped slot

A past [slot](terminology.md#slot) that did not produce a [block](terminology.md#block), because the leader was offline or the [fork](terminology.md#fork) containing the slot was abandoned for a better alternative by cluster consensus. A skipped slot will not appear as an ancestor for blocks at subsequent slots, nor increment the [block height](terminology#block-height), nor expire the oldest `recent_blockhash`.

Whether a slot has been skipped can only be determined when it becomes older than the latest [rooted](terminology.md#root) (thus not-skipped) slot.

## slot

The period of time for which each [leader](terminology.md#leader) ingests transactions and produces a [block](terminology.md#block).

Collectively, slots create a logical clock. Slots are ordered sequentially and non-overlapping, comprising roughly equal real-world time as per [PoH](terminology.md#proof-of-history).

## smart contract

A set of constraints that once satisfied, signal to a program that some predefined account updates are permitted.

## sol

The [native token](terminology.md#native-token) tracked by a [cluster](terminology.md#cluster) recognized by the company Solana.

## stake

Tokens forfeit to the [cluster](terminology.md#cluster) if malicious [validator](terminology.md#validator) behavior can be proven.

## supermajority

2/3 of a [cluster](terminology.md#cluster).

## sysvar

A synthetic [account](terminology.md#account) provided by the runtime to allow programs to access network state such as current tick height, rewards [points](terminology.md#point) values, etc.

## thin client

A type of [client](terminology.md#client) that trusts it is communicating with a valid [cluster](terminology.md#cluster).

## tick

A ledger [entry](terminology.md#entry) that estimates wallclock duration.

## tick height

The Nth [tick](terminology.md#tick) in the [ledger](terminology.md#ledger).

## token

A scarce, fungible member of a set of tokens.

## tps

[Transactions](terminology.md#transaction) per second.

## transaction

One or more [instructions](terminology.md#instruction) signed by the [client](terminology.md#client) using one or more [keypairs](terminology.md#keypair) and executed atomically with only two possible outcomes: success or failure.

## transaction id

The first [signature](terminology.md#signature) in a [transaction](terminology.md#transaction), which can be used to uniquely identify the transaction across the complete [ledger](terminology.md#ledger).

## transaction confirmations

The number of [confirmed blocks](terminology.md#confirmed-block) since the transaction was accepted onto the [ledger](terminology.md#ledger). A transaction is finalized when its block becomes a [root](terminology.md#root).

## transactions entry

A set of [transactions](terminology.md#transaction) that may be executed in parallel.

## validator

A full participant in the [cluster](terminology.md#cluster) responsible for validating the [ledger](terminology.md#ledger) and producing new [blocks](terminology.md#block).

## VDF

See [verifiable delay function](terminology.md#verifiable-delay-function).

## verifiable delay function

A function that takes a fixed amount of time to execute that produces a proof that it ran, which can then be verified in less time than it took to produce.

## vote

See [ledger vote](terminology.md#ledger-vote).

## vote credit

A reward tally for [validators](terminology.md#validator). A vote credit is awarded to a validator in its vote account when the validator reaches a [root](terminology.md#root).

## wallet

A collection of [keypairs](terminology.md#keypair).

## warmup period

Some number of [epochs](terminology.md#epoch) after [stake](terminology.md#stake) has been delegated while it progressively becomes effective. During this period, the stake is considered to be "activating". More info about: [warmup and cooldown](cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal)
