---
title: "Giao dịch"
---

Việc thực hiện chương trình bắt đầu với một [giao dịch](terminology.md#transaction) được gửi đến cụm. Thời gian chạy Solana sẽ thực thi một chương trình để xử lý từng [hướng dẫn](terminology.md#instruction) có trong giao dịch, theo thứ tự và nguyên tử.

## Phân tích Giao dịch

Phần này bao gồm định dạng nhị phân của một giao dịch.

### Định dạng Giao dịch

Một giao dịch chứa [compact-array](#compact-array-format) của chữ ký, theo sau là một [message](#message-format). Mỗi mục trong mảng chữ ký là một [digital signature](#signature-format) của tin nhắn đã cho. Thời gian chạy Solana xác minh rằng số lượng chữ ký khớp với số trong 8 bit đầu tiên của [tiêu đề thư](#message-header-format). Nó cũng xác minh rằng mỗi chữ ký được ký bởi private key tương ứng với public key tại cùng một chỉ mục trong mảng địa chỉ tài khoản của thư.

#### Định dạng chữ ký

Mỗi chữ ký điện tử ở định dạng nhị phân ed25519 và tiêu thụ 64 byte.

### Định dạng tin nhắn

Thư chứa [tiêu đề](#message-header-format), theo sau là mảng nhỏ gọn gồm [địa chỉ tài khoản](#account-addresses-format), theo sau là [blockhash](#blockhash-format) gần đây, theo sau là một loạt các [hướng dẫn](#instruction-format).

#### Định dạng tiêu đề thư

Tiêu đề thư chứa ba giá trị 8-bit không dấu. Giá trị đầu tiên là số lượng chữ ký được yêu cầu trong giao dịch chứa. Giá trị thứ hai là số địa chỉ tài khoản tương ứng ở chế độ chỉ-đọc. Giá trị thứ ba trong tiêu đề thư là số địa chỉ tài khoản chỉ-đọc không yêu cầu chữ ký.

#### Định dạng địa chỉ tài khoản

Các địa chỉ yêu cầu chữ ký xuất hiện ở đầu mảng địa chỉ tài khoản, với các địa chỉ yêu cầu quyền ghi trước và các tài khoản chỉ-đọc sau đó. Các địa chỉ không yêu cầu chữ ký tuân theo các địa chỉ có, một lần nữa với tài khoản đọc-ghi trước và tài khoản chỉ-đọc sau đó.

#### Định dạng Blockhash

Một blockhash chứa hàm băm SHA-256 32 byte. Nó được sử dụng để chỉ ra thời điểm khách hàng quan sát sổ cái lần cuối. Các validator sẽ từ chối giao dịch khi blockhash quá cũ.

### Định dạng hướng dẫn

Một lệnh chứa chỉ mục id chương trình, theo sau là một mảng nhỏ các chỉ mục địa chỉ tài khoản, theo sau là một mảng dữ liệu 8-bit không rõ ràng. Chỉ mục id chương trình được sử dụng để xác định một chương trình trên chuỗi có thể diễn giải dữ liệu không rõ ràng. Chỉ mục id chương trình là một chỉ mục 8 bit không dấu cho một địa chỉ tài khoản trong mảng địa chỉ tài khoản của thông điệp. Mỗi chỉ mục địa chỉ tài khoản là một chỉ mục 8-bit không dấu vào cùng một mảng đó.

### Định dạng mảng nhỏ gọn

Một mảng nhỏ gọn được tuần tự hóa thành độ dài của mảng, theo sau là từng mục mảng. Độ dài mảng là một mã hóa nhiều byte đặc biệt được gọi là compact-u16.

#### Định dạng Compact-u16

Compact-u16 là một mã hóa nhiều byte gồm 16 bit. Byte đầu tiên chứa 7 bit thấp hơn của giá trị trong 7 bit thấp hơn của nó. Nếu giá trị trên 0x7f, bit cao được đặt và 7 bit tiếp theo của giá trị được đặt vào 7 bit thấp hơn của byte thứ hai. Nếu giá trị trên 0x3fff, bit cao được đặt và 2 bit còn lại của giá trị được đặt vào 2 bit thấp hơn của byte thứ ba.

### Định dạng địa chỉ tài khoản

Địa chỉ tài khoản là 32 byte dữ liệu tùy ý. Khi địa chỉ yêu cầu chữ ký điện tử, thời gian chạy sẽ hiểu nó là khóa công khai của keypair ed25519.

## Hướng dẫn

Mỗi [chỉ dẫn](terminology.md#instruction) chỉ định một chương trình duy nhất, một tập hợp con các tài khoản của giao dịch sẽ được chuyển cho chương trình và một mảng byte dữ liệu được chuyển cho chương trình. Chương trình diễn giải mảng dữ liệu và hoạt động trên các tài khoản được chỉ định bởi hướng dẫn. Chương trình có thể trả về thành công hoặc có mã lỗi. Trả về lỗi khiến toàn bộ giao dịch không thành công ngay lập tức.

Programs typically provide helper functions to construct instructions they support. Ví dụ: chương trình hệ thống cung cấp trình trợ giúp Rust sau đây để xây dựng một chỉ dẫn [`SystemInstruction::CreateAccount`](https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L63):

```rust
pub fn create_account(
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, true),
    ];
    Instruction::new_with_bincode(
        system_program::id(),
        &SystemInstruction::CreateAccount {
            lamports,
            space,
            owner: *owner,
        },
        account_metas,
    )
}
```

Có thể tìm thấy ở đây:

https://github.com/solana-labs/solana/blob/6606590b8132e56dab9e60b3f7d20ba7412a736c/sdk/program/src/system_instruction.rs#L220

### Id chương trình

[id chương trình](terminology.md#program-id) của hướng dẫn chỉ định chương trình nào sẽ xử lý chỉ dẫn này. Chủ sở hữu tài khoản của chương trình chỉ định trình tải nào nên được sử dụng để tải và thực thi chương trình và dữ liệu chứa thông tin về cách thời gian chạy sẽ thực thi chương trình.

In the case of [on-chain BPF programs](developing/on-chain-programs/overview.md), the owner is the BPF Loader and the account data holds the BPF bytecode. Program accounts are permanently marked as executable by the loader once they are successfully deployed. The runtime will reject transactions that specify programs that are not executable.

Unlike on-chain programs, [Native Programs](developing/runtime-facilities/programs) are handled differently in that they are built directly into the Solana runtime.

### Tài khoản

Các tài khoản được tham chiếu bởi một lệnh thể hiện trạng thái trên chuỗi và đóng vai trò là cả đầu vào và đầu ra của một chương trình. Thông tin thêm về Tài khoản có thể được tìm thấy trong phần [Tài khoản](accounts.md).

### Dữ liệu hướng dẫn

Mỗi lệnh chứa một mảng byte mục đích chung được chuyển đến chương trình cùng với các tài khoản. Nội dung của dữ liệu hướng dẫn là chương trình cụ thể và thường được sử dụng để truyền đạt các hoạt động mà chương trình phải thực hiện và bất kỳ thông tin bổ sung nào mà các hoạt động đó có thể cần trên và ngoài những gì tài khoản chứa.

Các chương trình miễn phí chỉ định cách thông tin được mã hóa thành mảng byte dữ liệu hướng dẫn. Việc lựa chọn cách mã hóa dữ liệu nên tính đến chi phí giải mã vì bước đó được thực hiện bởi chương trình trên chuỗi. Người ta quan sát thấy rằng một số mã hóa phổ biến (ví dụ: mã bincode của Rust) rất kém hiệu quả.

[Solana Program Library's Token program](https://github.com/solana-labs/solana-program-library/tree/master/token) đưa ra một ví dụ về cách dữ liệu hướng dẫn có thể được mã hóa một cách hiệu quả, nhưng lưu ý rằng phương pháp này chỉ hỗ trợ các loại có kích thước cố định. Mã thông báo sử dụng [Pack](https://github.com/solana-labs/solana/blob/master/sdk/program/src/program_pack.rs) đặc điểm để mã hóa/giải mã dữ liệu hướng dẫn cho cả lệnh mã thông báo cũng như trạng thái tài khoản mã thông báo.

### Multiple instructions in a single transaction

A transaction can contain instructions in any order. This means a malicious user could craft transactions that may pose instructions in an order that the program has not been protected against. Programs should be hardened to properly and safely handle any possible instruction sequence.

One not so obvious example is account deinitialization. Some programs may attempt to deinitialize an account by setting its lamports to zero, with the assumption that the runtime will delete the account. This assumption may be valid between transactions, but it is not between instructions or cross-program invocations. To harden against this, the program should also explicitly zero out the account's data.

An example of where this could be a problem is if a token program, upon transferring the token out of an account, sets the account's lamports to zero, assuming it will be deleted by the runtime. If the program does not zero out the account's data, a malicious user could trail this instruction with another that transfers the tokens a second time.

## Chữ ký

Mỗi giao dịch liệt kê rõ ràng tất cả các public key của tài khoản được tham chiếu bởi hướng dẫn của giao dịch. Một tập hợp con của các public key đó đi kèm với một chữ ký giao dịch. Những chữ ký đó báo hiệu các chương trình trực tuyến rằng chủ tài khoản đã ủy quyền giao dịch. Thông thường, chương trình sử dụng ủy quyền để cho phép ghi nợ tài khoản hoặc sửa đổi dữ liệu của nó. Thông tin thêm về cách ủy quyền được truyền đạt đến một chương trình có thể được tìm thấy trong [Tài khoản](accounts.md#signers)

## Blockhash gần đây

Một giao dịch bao gồm [blockhash](terminology.md#blockhash) gần đây để ngăn trùng lặp và để cung cấp cho các giao dịch vòng đời. Bất kỳ giao dịch nào hoàn toàn giống với giao dịch trước đó đều bị từ chối, vì vậy việc thêm blockhash mới hơn cho phép nhiều giao dịch lặp lại cùng một hành động chính xác. Các giao dịch cũng có vòng đời được xác định bởi blockhash, vì bất kỳ giao dịch nào có blockhash quá cũ sẽ bị từ chối.
