---
title: Cấu trúc tài khoản Stake
---

Tài khoản stake trên Solana có thể được sử dụng để ủy quyền mã thông báo cho các validator trên mạng để có thể kiếm được phần thưởng cho chủ sở hữu của tài khoản stake. Stake accounts are created and managed differently than a traditional wallet address, known as a _system account_. Tài khoản hệ thống chỉ có thể gửi và nhận SOL từ các tài khoản khác trên mạng, trong khi tài khoản stake hỗ trợ các hoạt động phức tạp hơn để quản lý ủy quyền mã thông báo.

Tài khoản stake trên Solana cũng hoạt động khác so với các tài khoản của các mạng blockchain Proof-of-Stake khác mà bạn đã quen thuộc. Tài liệu này mô tả cấu trúc cao cấp và các chức năng của tài khoản stake Solana.

#### Địa Chỉ Tài Khoản

Mỗi tài khoản stake có một địa chỉ duy nhất, có thể được sử dụng để tra cứu thông tin tài khoản trong dòng lệnh hoặc trong bất kỳ công cụ khám phá mạng. Tuy nhiên, không giống như địa chỉ ví mà người nắm giữ keypair của địa chỉ kiểm soát ví, keypair được liên kết với địa chỉ tài khoản stake không nhất thiết phải có bất kỳ quyền kiểm soát nào đối với tài khoản. Trên thực tế, một keypair hoặc private key thậm chí có thể không tồn tại cho địa chỉ của tài khoản stake.

Lần duy nhất địa chỉ của tài khoản stake có tệp keypair là khi [creating a stake account using the command line tools](../cli/delegate-stake.md#create-a-stake-account), một tệp keypair mới được tạo trước tiên chỉ để đảm bảo rằng tài khoản stake là mới và duy nhất.

#### Tìm hiểu các chức trách tài khoản

Certain types of accounts may have one or more _signing authorities_ associated with a given account. Cơ quan tài khoản được sử dụng để ký các giao dịch nhất định cho tài khoản mà cơ quan này kiểm soát. Điều này khác với một số mạng blockchain khác trong đó chủ sở hữu keypair được liên kết với địa chỉ của tài khoản kiểm soát tất cả hoạt động của tài khoản.

Mỗi tài khoản stake có hai cơ quan ký tên được chỉ định theo địa chỉ tương ứng của họ, mỗi cơ quan được ủy quyền để thực hiện các hoạt động nhất định trên tài khoản stake.

The _stake authority_ is used to sign transactions for the following operations:

- Ủy quyền stake
- Hủy kích hoạt ủy quyền stake
- Tách tài khoản stake, tạo một tài khoản stake mới với một phần tiền trong tài khoản đầu tiên
- Merging two stake accounts into one
- Đặt cơ quan quyền sở hữu stake mới

The _withdraw authority_ signs transactions for the following:

- Rút stake chưa được ủy quyền vào một địa chỉ ví
- Đặt thẩm quyền rút tiền mới
- Đặt thẩm quyền stake mới

Thẩm quyền stake và thẩm quyền rút tiền được đặt khi tài khoản stake được tạo và chúng có thể được thay đổi để cho phép địa chỉ ký mới bất kỳ lúc nào. Thẩm quyền stake và rút tiền có thể là cùng một địa chỉ hoặc hai địa chỉ khác nhau.

Keypair thẩm quyền rút tiền giữ quyền kiểm soát nhiều hơn đối với tài khoản vì nó cần thiết để thanh lý các mã thông báo trong tài khoản stake và có thể được sử dụng để đặt lại thẩm quyền stake nếu keypair thẩm quyền stake bị mất hoặc bị xâm phạm.

Đảm bảo thẩm quyền rút tiền chống lại mất mát hoặc trộm cắp là điều quan trọng khi quản lý tài khoản stake.

#### Nhiều ủy quyền

Mỗi tài khoản stake chỉ có thể được sử dụng để ủy quyền cho một validator tại một thời điểm. Tất cả các mã thông báo trong tài khoản đều được ủy quyền hoặc không được ủy quyền, hoặc đang trong quá trình được ủy quyền hoặc không được ủy quyền. Để ủy quyền một phần nhỏ mã thông báo của bạn cho một validator hoặc ủy quyền cho nhiều validator, bạn phải tạo nhiều tài khoản stake.

Điều này có thể được thực hiện bằng cách tạo nhiều tài khoản stake từ một địa chỉ ví có chứa mã thông báo hoặc bằng cách tạo một tài khoản stake lớn duy nhất và sử dụng stake authority để chia tài khoản thành nhiều tài khoản với số dư mã thông báo bạn chọn.

Có thể chỉ định cùng một thẩm quyền stake và rút tiền cho nhiều tài khoản stake.

#### Merging stake accounts

Two stake accounts that have the same authorities and lockup can be merged into a single resulting stake account. A merge is possible between two stakes in the following states with no additional conditions:

- two deactivated stakes
- an inactive stake into an activating stake during its activation epoch

For the following cases, the voter pubkey and vote credits observed must match:

- two activated stakes
- two activating accounts that share an activation epoch, during the activation epoch

All other combinations of stake states will fail to merge, including all "transient" states, where a stake is activating or deactivating with a non-zero effective stake.

#### Delegation Warmup and Cooldown

When a stake account is delegated, or a delegation is deactivated, the operation does not take effect immediately.

A delegation or deactivation takes several [epochs](../terminology.md#epoch) to complete, with a fraction of the delegation becoming active or inactive at each epoch boundary after the transaction containing the instructions has been submitted to the cluster.

There is also a limit on how much total stake can become delegated or deactivated in a single epoch, to prevent large sudden changes in stake across the network as a whole. Since warmup and cooldown are dependent on the behavior of other network participants, their exact duration is difficult to predict. Details on the warmup and cooldown timing can be found [here](../cluster/stake-delegation-and-rewards.md#stake-warmup-cooldown-withdrawal).

#### Lockups

Stake accounts can have a lockup which prevents the tokens they hold from being withdrawn before a particular date or epoch has been reached. While locked up, the stake account can still be delegated, un-delegated, or split, and its stake and withdraw authorities can be changed as normal. Only withdrawal into a wallet address is not allowed.

A lockup can only be added when a stake account is first created, but it can be modified later, by the _lockup authority_ or _custodian_, the address of which is also set when the account is created.

#### Destroying a Stake Account

Like other types of accounts on the Solana network, a stake account that has a balance of 0 SOL is no longer tracked. If a stake account is not delegated and all of the tokens it contains are withdrawn to a wallet address, the account at that address is effectively destroyed, and will need to be manually re-created for the address to be used again.

#### Viewing Stake Accounts

Stake account details can be viewed on the Solana Explorer by copying and pasting an account address into the search bar.

- http://explorer.solana.com/accounts
