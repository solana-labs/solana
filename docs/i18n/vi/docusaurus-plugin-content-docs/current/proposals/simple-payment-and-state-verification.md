---
title: Thanh toán đơn giản và xác minh trạng thái
---

Việc cho phép các khách hàng có nguồn lực thấp tham gia vào một cụm Solana thường rất hữu ích. Việc tham gia này là kinh tế hoặc thực hiện hợp đồng, việc xác minh rằng hoạt động của khách hàng đã được mạng chấp nhận thường rất tốn kém. Đề xuất này đưa ra một cơ chế để các khách hàng đó xác nhận rằng các hành động của họ đã được cam kết với trạng thái sổ cái với chi phí tài nguyên tối thiểu và sự tin tưởng của bên thứ ba.

## Cách tiếp cận ngây thơ

Các validator lưu trữ chữ ký của các giao dịch được xác nhận gần đây trong một khoảng thời gian ngắn để đảm bảo rằng chúng không được xử lý nhiều lần. Các validator cung cấp điểm cuối JSON RPC, mà khách hàng có thể sử dụng để truy vấn cụm nếu giao dịch đã được xử lý gần đây. Các validator cũng cung cấp một thông báo PubSub, theo đó khách hàng đăng ký để được thông báo khi một chữ ký nhất định được quan sát bởi validator. Mặc dù hai cơ chế này cho phép khách hàng xác minh khoản thanh toán, nhưng chúng không phải là bằng chứng và dựa vào việc hoàn toàn tin tưởng vào validator.

Chúng tôi sẽ mô tả một cách để giảm thiểu sự tin tưởng này bằng cách sử dụng Merkle Proofs để neo phản hồi của các validator trong sổ cái, cho phép khách hàng tự xác nhận rằng có đủ số lượng các validator ưa thích của họ đã xác nhận giao dịch. Việc yêu cầu nhiều chứng thực validator càng làm giảm sự tin tưởng vào validator, vì nó làm tăng khó khăn cả về kỹ thuật và kinh tế khi ảnh hưởng đến một số người tham gia mạng khác.

## Light Clients

'light client' là một người tham gia cụm không tự chạy validator. Light client này sẽ cung cấp mức độ bảo mật cao hơn so với việc tin tưởng validator từ xa, mà không yêu cầu light client phải dành nhiều tài nguyên để xác minh sổ cái.

Thay vì cung cấp chữ ký giao dịch trực tiếp cho một khách hàng nhẹ, validator thay vì tạo Bằng chứng Merkle từ giao dịch quan tâm đến gốc của Merkle Tree của tất cả các giao dịch trong khối bao gồm. Merkle Root này được lưu trữ trong một mục nhập sổ cái được các validator bỏ phiếu, cung cấp cho nó tính hợp pháp đồng thuận. Mức độ bảo mật bổ sung cho khách hàng nhẹ phụ thuộc vào một tập hợp các validator hợp quy ban đầu mà khách hàng nhẹ được coi là các bên liên quan của cụm. Khi tập hợp đó được thay đổi, khách hàng có thể cập nhật tập hợp nội bộ của các validator đã biết với [biên nhận](simple-payment-and-state-verification.md#receipts). Điều này có thể trở nên khó khăn với một số lượng lớn stakes được ủy quyền.

Bản thân các validator có thể muốn sử dụng API khách hàng nhẹ vì lý do hiệu suất. Ví dụ: trong lần khởi chạy validator ban đầu, validator có thể sử dụng một điểm kiểm tra được cung cấp theo cụm của trạng thái và xác minh nó bằng biên nhận.

## Biên nhận

Biên nhận là một bằng chứng tối thiểu rằng; một giao dịch đã được bao gồm trong một khối, rằng khối đã được bình chọn bởi các validator ưu tiên của khách hàng và phiếu bầu đã đạt đến độ sâu xác nhận mong muốn.

### Bằng chứng bao gồm giao dịch

Bằng chứng bao gồm giao dịch là cấu trúc dữ liệu có chứa Đường dẫn Merkle từ một giao dịch, thông qua Entry-Merkle đến Block-Merkle, được bao gồm trong Bank-Hash với bộ phiếu validator bắt buộc. Một chuỗi các Mục nhập PoH chứa các phiếu bầu validator tiếp theo, xuất phát từ Bank-Hash, là bằng chứng xác nhận.

#### Giao dịch Merkle

Entry-Merkle là một Merkle Root bao gồm tất cả các giao dịch trong một mục nhất định, được sắp xếp theo chữ ký. Mỗi giao dịch trong một mục đã được kết hợp ở đây: https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/ledger/src/entry.rs#L205. Điều này có nghĩa là chúng tôi có thể hiển thị một giao dịch `T` đã được bao gồm trong một mục nhập `E`.

Một Block-Merkle là Merkle Root của tất cả các Entry-Merkles được sắp xếp theo trình tự trong khối.

![Sơ đồ khối Merkle](/img/spv-block-merkle.svg)

Hai bằng chứng merkle cùng nhau cho thấy một giao dịch `T` đã được bao gồm trong một khối với hàm băm ngân hàng `B`.

Accounts-Hash là hàm băm của sự kết hợp của các hàm băm trạng thái của từng tài khoản được sửa đổi trong slot hiện tại.

Trạng thái giao dịch là cần thiết cho biên nhận vì biên nhận trạng thái được xây dựng cho khối. Hai giao dịch trên cùng một trạng thái có thể xuất hiện trong khối và do đó, không có cách nào để chỉ từ trạng thái xem liệu một giao dịch được cam kết với sổ cái đã thành công hay thất bại trong việc sửa đổi trạng thái dự định. Có thể không cần mã hóa toàn bộ mã trạng thái mà chỉ cần một bit trạng thái duy nhất để cho biết giao dịch thành công.

Hiện tại, Block-Merkle chưa được triển khai, vì vậy để xác minh `E` là một mục nhập trong khối với hàm băm ngân hàng `B`, chúng tôi sẽ cần cung cấp tất cả các băm mục nhập trong khối. Lý tưởng nhất là Block-Merkle này sẽ được cấy ghép, vì giải pháp thay thế rất kém hiệu quả.

#### Tiêu đề khối
Để xác minh bằng chứng bao gồm giao dịch, light clients cần có khả năng suy ra cấu trúc liên kết của các nhánh trong mạng

Cụ thể hơn, light client sẽ cần theo dõi các tiêu đề khối đến sao cho có hai hàm băm ngân hàng cho các khối `A` and `B` chúng có thể xác định xem liệu `A` có phải là tổ tiên của `B` (Phần bên dưới `Optimistic Confirmation Proof` giải thích tại sao!). Nội dung của tiêu đề là các trường cần thiết để tính toán hàm băm ngân hàng.

Bank-Hash là hàm băm của sự kết hợp giữa Block-Merkle và Accounts-Hash được mô tả trong `Transaction Merkle`.

![Sơ đồ hàm băm ngân hàng](/img/spv-bank-hash.svg)

Trong mã:

https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/runtime/src/bank.rs#L3468-L3473

```
        let mut hash = hashv(&[
            // bank hash of the parent block
            self.parent_hash.as_ref(),
            // hash of all the modifed accounts
            accounts_delta_hash.hash.as_ref(),
            // Number of signatures processed in this block
            &signature_count_buf,
            // Last PoH hash in this block
            self.last_blockhash().as_ref(),
        ]);
```

Một nơi tốt để triển khai logic này dọc theo logic phát trực tuyến hiện có trong logic phát lại của validator: https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/core/src/replay_stage.rs#L1092-L1096

#### Bằng chứng xác nhận lạc quan

Hiện tại, xác nhận lạc quan được phát hiện thông qua trình nghe theo dõi gosspip và đường dẫn phát lại cho phiếu bầu: https://github.com/solana-labs/solana/blob/b6bfed64cb159ee67bb6bdbaefc7f833bbed3563/core/src/cluster_info_vote_listener.rs#L604-L614.

Mỗi phiếu bầu là một giao dịch đã ký bao gồm hàm băm ngân hàng của khối mà validator đã bỏ phiếu, tức là `B` từ code>Transaction Merkle</code> phần trên. Khi một ngưỡng nhất định `T` của mạng đã bỏ phiếu cho một khối, khối đó sẽ được coi là đã xác nhận một cách tối ưu. Các phiếu bầu được thực hiện bởi nhóm các validator `T` này là cần thiết để hiển thị khối có hàm băm ngân hàng `B` đã được xác nhận một cách lạc quan.

Tuy nhiên, khác với một số siêu dữ liệu, bản thân các phiếu bầu đã ký hiện không được lưu trữ ở bất kỳ đâu, vì vậy không thể truy xuất chúng theo yêu cầu. Các phiếu bầu này có lẽ cần được lưu giữ trong cơ sở dữ liệu Rocksdb, được lập chỉ mục bởi một khóa `(Slot, Hash, Pubkey)` đại diện cho slot của phiếu bầu, hàm băm ngân hàng của phiếu bầu và tài khoản bỏ phiếu pubkey chịu trách nhiệm cho phiếu bầu.

Cùng với nhau, giao dịch merkle và các bằng chứng xác nhận lạc quan có thể được cung cấp qua RPC cho người đăng ký bằng cách mở rộng logic đăng ký chữ ký hiện có. Khách hàng đăng ký mức xác nhận "SingleGossip" đã được thông báo khi xác nhận lạc quan được phát hiện, một cờ có thể được cung cấp để báo hiệu hai bằng chứng ở trên cũng sẽ được trả lại.

Điều quan trọng cần lưu ý là việc xác nhận một cách lạc quan `B` cũng ngụ ý rằng tất cả các khối tổ tiên của `B` cũng được xác nhận một cách lạc quan và cũng không phải tất cả các khối đều sẽ được xác nhận một cách lạc quan.

```

B -> B'

```

Vì vậy, trong ví dụ trên nếu một khối `B'` được xác nhận một cách tối ưu, `B` thì cũng như vậy. Do đó, nếu một giao dịch nằm trong khối `B`, giao dịch merkle trong bằng chứng sẽ dành cho khối `B`, nhưng số phiếu được trình bày trong bằng chứng sẽ dành cho khối `B'`. Đây là lý do tại sao các tiêu đề trong `Block headers` phần trên lại quan trọng, khách hàng sẽ cần xác minh xem đó `B` có thực sự là tổ tiên của `B'`.

#### Bằng chứng về việc phân phối Stake

Sau khi được trình bày với merkle giao dịch và các bằng chứng xác nhận lạc quan ở trên, khách hàng có thể xác minh một giao dịch `T` đã được xác nhận tối ưu trong một khối với hàm băm ngân hàng `B`. Phần còn thiếu cuối cùng là làm thế nào để xác minh rằng các phiếu bầu trong các bằng chứng lạc quan ở trên thực sự tạo thành `T` tỷ lệ phần trăm hợp lệ của stake cần thiết để duy trì các đảm bảo an toàn của "xác nhận lạc quan".

Một cách để tiếp cận điều này có thể là đối với mọi kỷ nguyên, khi số tiền stake thay đổi, ghi tất cả tiền stake vào tài khoản hệ thống và sau đó yêu cầu các validator đăng ký vào tài khoản hệ thống đó. Các node đầy đủ sau đó có thể cung cấp một merkle chứng minh rằng trạng thái tài khoản hệ thống đã được cập nhật trong một số khối `B` và sau đó cho thấy rằng khối `B` đã được confirmed/rooted một cách lạc quan.

### Xác minh Trạng thái Tài khoản

Trạng thái của tài khoản (số dư hoặc dữ liệu khác) có thể được xác minh bằng cách gửi giao dịch với **_TBD_** tới cụm. Sau đó, khách hàng có thể sử dụng [Bằng chứng bao gồm giao dịch](#transaction-inclusion-proof) để xác minh xem cụm đồng ý rằng tài khoản đã đạt đến trạng thái mong đợi hay chưa.

### Phiếu bầu của Validator

Các Leader nên kết hợp các phiếu bầu của validator theo tỷ trọng stake vào một mục duy nhất. Điều này sẽ giảm số lượng mục nhập cần thiết để tạo biên nhận.

### Chuỗi Mục nhập

Biên lai có liên kết PoH từ gốc thanh toán hoặc trạng thái Merkle Path tới danh sách các phiếu xác nhận liên tiếp.

Nó chứa những thứ sau:

- Transaction -&gt; Entry-Merkle -&gt; Block-Merkle -&gt; Bank-Hash

Và một vector của các mục PoH:

- Mục bỏ phiếu của validator
- Đánh dấu
- Mục Light

```text
/// This Entry definition skips over the transactions and only contains the
/// hash of the transactions used to modify PoH.
LightEntry {
    /// The number of hashes since the previous Entry ID.
    pub num_hashes: u64,
    /// The SHA-256 hash `num_hashes` after the previous Entry ID.
    hash: Hash,
    /// The Merkle Root of the transactions encoded into the Entry.
    entry_hash: Hash,
}
```

Các mục nhập ánh sáng được tạo lại từ Mục nhập và chỉ cần hiển thị mục nhập Merkle Root đã được trộn vào hàm băm PoH, thay vì tập hợp toàn bộ giao dịch.

Khách hàng không cần trạng thái bỏ phiếu bắt đầu. Thuật toán [fork selection](../implemented-proposals/tower-bft.md) được xác định sao cho chỉ những phiếu bầu xuất hiện sau giao dịch mới cung cấp tính chính xác cho giao dịch và tính cuối cùng độc lập với trạng thái bắt đầu.

### Xác minh

Một khách hàng nhẹ nhận thức được các validator tập hợp siêu đa số có thể xác minh biên nhận bằng cách đi theo Merkle Path đến chuỗi PoH. Block-Merkle là Merkle Root và sẽ xuất hiện trong các phiếu bầu có trong Mục nhập. Khách hàng nhẹ có thể mô phỏng [fork selection](../implemented-proposals/tower-bft.md) cho các phiếu bầu liên tiếp và xác minh rằng biên nhận được xác nhận ở ngưỡng khóa mong muốn.

### Trạng thái tổng hợp

Trạng thái tổng hợp phải được tính vào Bank-Hash cùng với trạng thái do ngân hàng tạo.

Ví dụ:

- Các tài khoản kỷ nguyên validator và các stake và trọng lượng của chúng.
- Mức phí tính toán

Các giá trị này phải có mục nhập trong Bank-Hash. Chúng phải nằm trong các tài khoản đã biết và do đó có một chỉ mục trong nối hàm băm.
