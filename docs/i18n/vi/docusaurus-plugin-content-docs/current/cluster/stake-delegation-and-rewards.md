---
title: Ủy quyền stake và phần thưởng
---

Người tham gia stake được thưởng vì đã giúp xác thực sổ cái. Họ làm điều này bằng cách ủy quyền stake của họ cho các node validator. Những validator đó thực hiện công việc phát lại sổ cái và gửi phiếu bầu đến tài khoản bỏ phiếu cho mỗi node mà những người tham gia stake có thể ủy quyền số tiền stake của họ. Phần còn lại của cụm sử dụng các phiếu bầu có tỷ trọng stake đó để chọn một khối khi các fork phát sinh. Cả validator và người tham gia stake đều cần một số động lực kinh tế để đóng vai trò của họ. Validator cần được bồi thường cho phần cứng của họ và người tham gia stake cần được bồi thường cho rủi ro bị slashing stake. Tính kinh tế được bao hàm trong [staking rewards](../implemented-proposals/staking-rewards.md). Mặt khác, phần này mô tả cơ chế cơ bản của việc triển khai nó.

## Thiết kế cơ bản

Ý tưởng chung là validator sở hữu tài khoản Bỏ phiếu. Tài khoản Bỏ phiếu theo dõi phiếu bầu của validator, đếm các khoản tín dụng do validator tạo và cung cấp bất kỳ trạng thái cụ thể nào của validator bổ sung. Tài khoản Bỏ phiếu không biết về bất kỳ các stake nào được ủy quyền cho nó và không có trọng lượng staking.

Một tài khoản Stake riêng biệt \(do một người tham gia stake tạo\) đặt tên cho một tài khoản Bỏ phiếu mà stake được ủy quyền. Phần thưởng được tạo ra tỷ lệ thuận với số lượng lamport được stake. Tài khoản Stake chỉ thuộc sở hữu của người tham gia stake. Một số phần của các lamport được lưu trữ trong tài khoản này là stake.

## Ủy quyền thụ động

Bất kỳ số lượng tài khoản Stake nào cũng đều có thể ủy quyền cho một tài khoản Bỏ phiếu duy nhất mà không cần hành động tương tác từ danh tính kiểm soát tài khoản Bỏ phiếu hoặc gửi phiếu bầu cho tài khoản.

Tổng số tiền stake được phân bổ cho tài khoản Bỏ phiếu có thể được tính bằng tổng của tất cả các tài khoản Stake có pubkey tài khoản Bỏ phiếu là `StakeState::Stake::voter_pubkey`.

## Bỏ phiếu và tài khoản stake

Quá trình nhận thưởng được chia thành hai chương trình trên chuỗi. Chương trình Bỏ phiếu giải quyết vấn đề làm cho tiền stake có thể giao dịch được. Chương trình Stake đóng vai trò là người giám sát pool phần thưởng và cung cấp cho việc ủy ​​quyền thụ động. Chương trình Stake chịu trách nhiệm trả phần thưởng cho người stake và người bỏ phiếu khi cho thấy rằng đại biểu của người stake đã tham gia xác thực sổ cái.

### VoteState

VoteState là trạng thái hiện tại của tất cả các phiếu bầu mà validator đã gửi cho mạng. VoteState chứa thông tin trạng thái sau:

- `votes` - Cấu trúc dữ liệu phiếu bầu đã gửi.
- `credits` - Tổng số phần thưởng mà chương trình Bình chọn này đã tạo ra trong suốt thời gian hoạt động.
- `root_slot` - Slot cuối cùng để đạt được cam kết lockout đầy đủ cần thiết để nhận phần thưởng.
- `commission` - Hoa hồng do VoteState này thực hiện cho bất kỳ phần thưởng nào được yêu cầu bởi các tài khoản Stake của người tham gia. Đây là trần phần trăm của phần thưởng.
- Account::lamports - Số lượng lamport tích lũy được từ hoa hồng. Đây không được tính là tiền stake.
- `authorized_voter` - Chỉ có danh tính này mới được phép gửi phiếu bầu. Trường này chỉ có thể được sửa đổi bởi danh tính này.
- `node_pubkey` - Node Solana bỏ phiếu trong tài khoản này.
- `authorized_withdrawer` - danh tính của pháp nhân phụ trách các báo cáo của tài khoản này, tách biệt với địa chỉ của tài khoản và người ký biểu quyết được ủy quyền.

### VoteInstruction::Initialize\(VoteInit\)

- `account[0]` - RW - VoteState.

  `VoteInit` mang tài khoản bỏ phiếu mới code>node_pubkey</code>, `authorized_voter`, `authorized_withdrawer`, và `commission`.

  các thành viên khác của VoteState được mặc định.

### VoteInstruction::Authorize\(Pubkey, VoteAuthorize\)

Cập nhật tài khoản với người bỏ phiếu hoặc người rút tiền được ủy quyền mới, theo tham số VoteAuthorize \(`Voter` hoặc `Withdrawer`\). Giao dịch phải được ký bằng tài khoản Vote hiện tại `authorized_voter` hoặc `authorized_withdrawer`.

- `account[0]` - RW - VoteState. `VoteState::authorized_voter` hoặc `authorized_withdrawer` được đặt thành `Pubkey`.

### VoteInstruction::Vote\(Vote\)

- `account[0]` - RW - VoteState. `VoteState::lockouts` và `VoteState::credits` được cập nhật theo quy tắc lockout bỏ phiếu xem [Tower BFT](../implemented-proposals/tower-bft.md).
- `account[1]` - RO - `sysvar::slot_hashes` Danh sách một số N slot gần đây nhất và số hàm băm của chúng để bỏ phiếu được xác minh chống lại.
- `account[2]` - RO - `sysvar::clock` Thời gian mạng hiện tại, được biểu thị bằng các slot, các kỷ nguyên.

### StakeState

Một StakeState có một trong bốn dạng, StakeState::Uninitialized, StakeState::Initialized, StakeState::Stake, và StakeState::RewardsPool. Chỉ có ba hình thức đầu tiên được sử dụng trong việc staking, nhưng chỉ có StakeState::Stake là thú vị. Tất cả các RewardsPools đều được tạo từ ban đầu.

### StakeState::Stake

StakeState::Stake là tùy chọn ủy quyền hiện tại của **staker** và chứa thông tin trạng thái sau:

- Account::lamports - Các lamport có sẵn để staking.
- `stake` - số tiền stake \(tùy thuộc vào thời gian khởi động và thời gian hồi chiêu\) để tạo phần thưởng, luôn nhỏ hơn hoặc bằng Account::lamports.
- `voter_pubkey` - pubkey của phiên bản VoteState mà các lamport được ủy quyền.
- `credits_observed` - Tổng số tín dụng được yêu cầu trong suốt thời gian của chương trình.
- `activated` - kỷ nguyên mà stake này được kích hoạt/ủy quyền. Toàn bộ tiền stake sẽ được tính sau khi khởi động.
- `deactivated` - thời gian mà số tiền stake này bị hủy kích hoạt, một số kỷ nguyên hồi chiêu là cần thiết trước khi tài khoản bị vô hiệu hóa hoàn toàn và số tiền stake có sẵn để rút.
- `authorized_staker` - pubkey của thực thể phải ký các giao dịch ủy quyền, kích hoạt và hủy kích hoạt.
- `authorized_withdrawer` - danh tính của pháp nhân chịu trách nhiệm về các lamport thông tin của tài khoản này, tách biệt với địa chỉ của tài khoản và người đứng tên được ủy quyền.

### StakeState::RewardsPool

Để tránh một lần khóa toàn mạng hoặc tranh chấp trong việc đổi quà, 256 RewardsPools là một phần của sự khởi đầu trong các khóa được xác định trước, mỗi khóa đều có các khoản tín dụng std::u64::MAX để có thể đáp ứng việc đổi thưởng theo giá trị điểm.

Tiền Stake và Phần thưởng là tài khoản thuộc sở hữu của cùng một chương trình `Stake`.

### StakeInstruction::DelegateStake

Tài khoản Stake được chuyển từ dạng Khởi tạo sang StakeState::Stake, hoặc từ dạng (tức là đã nguội hoàn toàn) StakeState::Stake đã StakeState::Stake đã kích hoạt. Đây là cách các nhà phân công chọn tài khoản bỏ phiếu và node validator mà các quyền đăng ký tài khoản stake của họ mà các lamport được ủy quyền. Giao dịch phải được ký bởi cổ phần `authorized_staker`.

- `account[0]` - RW - StakeState::Stake ví dụ. `StakeState::Stake::credits_observed` được khởi tạo thành `VoteState::credits`, `StakeState::Stake::voter_pubkey` được khởi tạo vào `account[1]`. Nếu đây là lần ủy quyền ban đầu của tiền stake, `StakeState::Stake::stake` được khởi tạo vào số dư của tài khoản trong các lamport, `StakeState::Stake::activated` được khởi tạo thành epoch Ngân hàng hiện tại và `StakeState::Stake::deactivated` được khởi tạo thành std::u64::MAX
- `account[1]` - R - VoteState ví dụ.
- `account[2]` - R - sysvar:: clock tài khoản, mang thông tin về epoch Ngân hàng hiện tại.
- `account[3]` - R - sysvar::stakehistory tài khoản, mang thông tin về lịch sử stake.
- `account[4]` - R - stake::Config tài khoản, mang cấu hình khởi động, thời gian hồi chiêu và slashing.

### StakeInstruction::Authorize\(Pubkey, StakeAuthorize\)

Cập nhật tài khoản với người rút tiền hoặc người rút tiền được ủy quyền mới, theo tham số StakeAuthorize \(`Staker` hoặc `Withdrawer`\). Giao dịch phải được ký bởi tài khoản Stake hiện tại `authorized_staker` hoặc `authorized_withdrawer`. Bất kỳ khóa stake nào đã hết hạn hoặc người quản lý khóa cũng phải ký vào giao dịch.

- `account[0]` - RW - StakeState.

  `StakeState::authorized_staker` hoặc `authorized_withdrawer` được đặt thành `Pubkey`.

### StakeInstruction::Deactivate

Người tham gia stake muốn rút tiền khỏi mạng. Để làm như vậy, trước tiên anh ta phải hủy kích hoạt tiền stake của mình và chờ thời gian hồi chiêu. Giao dịch phải được ký bởi `authorized_staker`.

- `account[0]` - RW - StakeState::Stake ví dụ đang hủy kích hoạt.
- `account[1]` - R - sysvar::clock tài khoản từ Ngân hàng mang epoch hiện tại.

StakeState::Stake::deactivated được đặt thành epoch hiện tại + thời gian hồi chiêu. Tiền stake của tài khoản sẽ giảm xuống 0 trong khoảng thời gian đó và Account::lamports sẽ có sẵn để rút tiền.

### StakeInstruction::Withdraw\(u64\)

Các lamport tích lũy theo thời gian trong tài khoản Stake và bất kỳ khoản tiền nào vượt quá số tiền stake đã kích hoạt đều có thể được rút. Giao dịch phải được ký bởi `authorized_withdrawer`.

- `account[0]` - RW - StakeState::Stake từ đó rút tiền.
- `account[1]` - RW - Tài khoản sẽ được ghi có với các lamport đã rút.
- `account[2]` - R - sysvar::clock tài khoản từ Ngân hàng mang kỷ nguyên hiện tại, để tính tiền stake.
- `account[3]` - R - sysvar::stake_history tài khoản từ Ngân hàng có stake khởi động/lịch sử hồi chiêu.

## Lợi ích của thiết kế

- Bỏ phiếu duy nhất cho tất cả các người tham gia stake.
- Không cần thiết phải xóa biến tín dụng để nhận phần thưởng.
- Mỗi stake được ủy quyền có thể nhận phần thưởng của mình một cách độc lập.
- Hoa hồng cho công việc được gửi khi phần thưởng được yêu cầu bởi stake được ủy nhiệm.

## Ví dụ về Callflow

![Quy Trình Staking Callflow](/img/passive-staking-callflow.png)

## Phần thưởng Staking

Cơ chế và quy tắc cụ thể của chế độ phần thưởng validator được nêu ở đây. Phần thưởng kiếm được bằng cách ủy quyền stake cho một validator đang bỏ phiếu chính xác. Việc bỏ phiếu không chính xác khiến tiền stake của validator đó bị [slashing](../proposals/slashing.md).

### Khái niệm cơ bản

Mạng trả phần thưởng từ một phần [inflation](../terminology.md#inflation) của mạng. Số lượng lamport có sẵn để trả phần thưởng cho một kỷ nguyên là cố định và phải được chia đều cho tất cả các node stake theo tỷ trọng tiền stake tương đối và sự tham gia của chúng. Đơn vị trọng số được gọi là [point](../terminology.md#point).

Phần thưởng cho một kỷ nguyên sẽ không có sẵn cho đến khi kết thúc kỷ nguyên đó.

Vào cuối mỗi kỷ nguyên, tổng số điểm kiếm được trong kỷ nguyên được cộng lại và được sử dụng để chia phần thưởng của lạm phát kỷ nguyên để đạt đến một giá trị điểm. Giá trị này được ghi lại trong ngân hàng trong một [sysvar](../terminology.md#sysvar) ánh xạ các kỷ nguyên thành các giá trị điểm.

Trong quá trình đổi thưởng, chương trình stake sẽ tính số điểm kiếm được bằng tiền stake cho mỗi kỷ nguyên, nhân số điểm đó với giá trị điểm của kỷ nguyên đó và chuyển các điểm số bằng số tiền đó từ tài khoản phần thưởng vào tài khoản stake và tài khoản biểu quyết theo cài đặt hoa hồng của tài khoản biểu quyết.

### Kinh tế học

Giá trị điểm cho một kỷ nguyên phụ thuộc vào sự tham gia tổng hợp của mạng. Nếu sự tham gia vào một kỷ nguyên giảm xuống, giá trị điểm sẽ cao hơn đối với những người có tham gia.

### Kiếm các khoản tín dụng

Các validator kiếm được một phiếu tín dụng cho mỗi phiếu bầu đúng vượt quá lockout tối đa, tức là mỗi khi tài khoản phiếu bầu của các validator loại bỏ một slot khỏi danh sách lockout của nó, làm cho phiếu bầu đó trở thành gốc cho node.

Những người tham gia stake đã ủy quyền cho validator đó sẽ kiếm được điểm tương ứng với số tiền stake của họ. Điểm kiếm được là sản phẩm của tín dụng phiếu bầu và tiền stake.

### Khởi động stake, thời gian hồi chiêu, rút ​​tiền

Stake, một khi đã được ủy quyền, không có hiệu lực ngay lập tức. Đầu tiên chúng phải trải qua giai đoạn khởi động. Trong giai đoạn này, một số phần tiền stake được coi là "có hiệu lực", phần còn lại được coi là "kích hoạt". Thay đổi xảy ra trên ranh giới kỷ nguyên.

Chương trình stake giới hạn tỷ lệ thay đổi đối với tổng stake của mạng, được phản ánh trong chương trình stake `config::warmup_rate` \(được đặt thành 25% mỗi kỷ nguyên trong quá trình triển khai hiện tại\).

Số tiền stake có thể được hâm nóng mỗi kỷ nguyên là một chức năng của tổng số tiền stake hiệu quả của kỷ nguyên trước đó, tổng số tiền stake kích hoạt và tỷ lệ khởi động được cấu hình của chương trình stake.

Thời gian hồi chiêu hoạt động theo cùng một cách. Một khi stake bị hủy kích hoạt, một số phần của nó được coi là "có hiệu lực", và cũng là "vô hiệu hóa". Khi tiền stake nguội đi, nó tiếp tục kiếm được phần thưởng và có thể bị slashing, nhưng nó cũng có sẵn để rút.

Stake của Bootstrap không bị khởi động.

Phần thưởng được trả dựa trên phần "hiệu quả" của tiền stake cho kỷ nguyên đó.

#### Ví dụ khởi động

Hãy xem xét tình huống của một stake duy nhất là 1,000 được kích hoạt ở kỷ nguyên N, với tỷ lệ khởi động của mạng là 20% và tổng stake của mạng đang yên lặng ở kỷ nguyên N là 2,000.

Tại kỷ nguyên N+1, số tiền có sẵn để kích hoạt mạng là 400 \(20% của 2000\), và tại kỷ nguyên N, số tiền stake ví dụ này là số tiền stake duy nhất được kích hoạt và do đó được hưởng tất cả các room khởi động có sẵn.

| kỷ nguyên | có hiệu lực | kích hoạt | tổng hiệu quả | tổng kích hoạt |
|:--------- | -----------:| ---------:| -------------:| --------------:|
| N-1       |             |           |         2,000 |              0 |
| N         |           0 |     1,000 |         2,000 |          1,000 |
| N+1       |         400 |       600 |         2,400 |            600 |
| N+2       |         880 |       120 |         2,880 |            120 |
| N+3       |        1000 |         0 |         3,000 |              0 |

Nếu có 2 stake \(X và Y\) được kích hoạt ở kỷ nguyên N, họ sẽ được thưởng một phần trong 20% tương ứng với số tiền stake của họ. Tại kỷ nguyên có hiệu lực và kích hoạt cho stake là một chức năng của trạng thái của các kỷ nguyên trước đó.

| kỷ nguyên | X eff | X act | X eff | X act | tổng hiệu quả | tổng kích hoạt |
|:--------- | -----:| -----:| -----:| -----:| -------------:| --------------:|
| N-1       |       |       |       |       |         2,000 |              0 |
| N         |     0 | 1,000 |     0 |   200 |         2,000 |          1,200 |
| N+1       |   333 |   667 |    67 |   133 |         2,400 |            800 |
| N+2       |   733 |   267 |   146 |    54 |         2,880 |            321 |
| N+3       |  1000 |     0 |   200 |     0 |         3,200 |              0 |

### Rút tiền

Chỉ những lamport vượt quá số tiền stake kích hoạt+có hiệu lực mới có thể rút bất kỳ lúc nào. Điều này có nghĩa là trong quá trình khởi động, không có tiền stake nào có thể được rút. Trong thời gian hồi chiêu, bất kỳ mã thông báo nào vượt quá số tiền stake có hiệu lực đều có thể rút \(kích hoạt == 0\). Vì phần thưởng kiếm được sẽ tự động được thêm vào stake, nên chỉ có thể rút tiền sau khi ngừng kích hoạt.

### Lock-up

Tài khoản stake hỗ trợ khái niệm lock-up, trong đó số dư tài khoản stake không có sẵn để rút cho đến một thời điểm cụ thể. Lock-up được chỉ định là độ cao kỷ nguyên, tức là độ cao kỷ nguyên tối thiểu mà mạng phải đạt được trước khi số dư tài khoản stake có sẵn để rút, trừ khi giao dịch cũng được ký bởi một người giám sát cụ thể. Thông tin này được thu thập khi tài khoản stake được tạo và được lưu trữ trong trường Lockup của trạng thái tài khoản stake. Việc thay đổi người ký quỹ hoặc người rút tiền được ủy quyền cũng có thể bị lock-up, vì một hoạt động như vậy thực sự là một hoạt động chuyển nhượng.
