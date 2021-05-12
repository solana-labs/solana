---
title: Gọi giữa các chương trình
---

## Lời mời Chương trình-chéo

Thời gian chạy Solana cho phép các chương trình gọi nhau thông qua một cơ chế gọi là lệnh gọi chương trình-chéo.  Việc gọi giữa các chương trình được thực hiện bằng cách một chương trình gọi một lệnh của chương trình kia.  Chương trình gọi được tạm dừng cho đến khi chương trình được gọi kết thúc quá trình xử lý lệnh.

Ví dụ, một khách hàng có thể tạo một giao dịch sửa đổi hai tài khoản, mỗi tài khoản được sở hữu bởi các chương trình trên chuỗi riêng biệt:

```rust,ignore
let message = Message::new(vec![
    token_instruction::pay(&alice_pubkey),
    acme_instruction::launch_missiles(&bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

Thay vào đó, khách hàng có thể cho phép chương trình `acme` thay mặt khách hàng gọi ra các hướng dẫn `token` một cách thuận tiện:

```rust,ignore
let message = Message::new(vec![
    acme_instruction::pay_and_launch_missiles(&alice_pubkey, &bob_pubkey),
]);
client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], &message);
```

Cung cấp hai chương trình trên chuỗi `token` và `acme`, mỗi chương trình hướng dẫn triển khai `pay()` và `launch_missiles()` tương ứng, acme có thể được triển khai bằng một lệnh gọi đến một hàm được xác định trong mô-đun `token` bằng cách đưa ra lời gọi chương trình chéo:

```rust,ignore
mod acme {
    use token_instruction;

    fn launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        ...
    }

    fn pay_and_launch_missiles(accounts: &[AccountInfo]) -> Result<()> {
        let alice_pubkey = accounts[1].key;
        let instruction = token_instruction::pay(&alice_pubkey);
        invoke(&instruction, accounts)?;

        launch_missiles(accounts)?;
    }
```

` invoke()` được tích hợp trong thời gian chạy của Solana và chịu trách nhiệm định tuyến lệnh đã cho đến chương trình `token` thông qua trường `program_id` của lệnh.

Lưu ý rằng `invoke` yêu cầu người gọi phải vượt qua tất cả các tài khoản được yêu cầu bởi hướng dẫn đang được gọi.  Điều này có nghĩa là cả tài khoản thực thi (những tài khoản khớp với id chương trình của hướng dẫn) và các tài khoản được chuyển cho trình xử lý lệnh.

Trước khi gọi `pay()`, thời gian chạy phải đảm bảo rằng `acme` đã không sửa đổi bất kỳ tài khoản nào thuộc sở hữu của `token`. Nó thực hiện điều này bằng cách áp dụng chính sách của thời gian chạy cho trạng thái hiện tại của tài khoản tại thời điểm `acme` gọi `invoke` so với trạng thái ban đầu của tài khoản khi bắt đầu hướng dẫn `acme`. Sau khi `pay()` hoàn tất, thời gian chạy lại phải đảm bảo rằng `token` không sửa đổi bất kỳ tài khoản nào thuộc sở hữu của `acme` bằng cách áp dụng lại chính sách của thời gian chạy, nhưng lần này là với `token` ID chương trình. Cuối cùng, sau khi `pay_and_launch_missiles()` hoàn tất, thời gian chạy phải áp dụng chính sách thời gian chạy một lần nữa, nơi nó thường áp dụng, sử dụng tất cả các biến `pre_*` đã cập nhật. Nếu việc thực thi `pay_and_launch_missiles()` lên đến `pay()` không thực hiện thay đổi tài khoản không hợp lệ, thì `pay()` không thực hiện thay đổi không hợp lệ và thực hiện từ `pay()` cho đến khi `pay_and_launch_missiles()` trả về không có thay đổi không hợp lệ nào, khi đó thời gian chạy có thể tạm thời giả sử `pay_and_launch_missiles()` như toàn bộ không thực hiện thay đổi tài khoản không hợp lệ và do đó cam kết tất cả các sửa đổi tài khoản này.

### Hướng dẫn yêu cầu đặc quyền

Thời gian chạy sử dụng các đặc quyền được cấp cho chương trình người gọi để xác định những đặc quyền nào có thể được mở rộng cho callee. Đặc quyền trong ngữ cảnh này đề cập đến người ký và tài khoản có thể ghi. Ví dụ, nếu lệnh mà người gọi đang xử lý có chứa người ký hoặc tài khoản có thể ghi, thì người gọi có thể gọi một lệnh cũng chứa người ký/hoặc tài khoản có thể ghi đó.

Phần mở rộng đặc quyền này dựa trên thực tế là các chương trình là bất biến. Trong trường hợp của chương trình `acme`, thời gian chạy có thể coi chữ ký của giao dịch một cách an toàn là chữ ký của một hướng dẫn `token`. Khi thời gian chạy thấy các `token` tham chiếu hướng dẫn `alice_pubkey`, nó sẽ tra cứu khóa trong `acme` hướng dẫn để xem liệu khóa đó có tương ứng với tài khoản đã ký hay không. Trong trường hợp này, nó thực hiện và do đó cho phép `token` chương trình sửa đổi tài khoản của Alice.

### Các tài khoản đã ký chương trình

Các chương trình có thể đưa ra các hướng dẫn chứa các tài khoản đã ký chưa được đăng nhập trong giao dịch ban đầu bằng cách sử dụng các địa chỉ do [Địa chỉ do chương trình tạo ra](#program-derived-addresses).

Để ký một tài khoản với các địa chỉ xuất phát từ chương trình, một chương trình `invoke_signed()` có thể.

```rust,ignore
        invoke_signed(
            &instruction,
            accounts,
            &[&["First addresses seed"],
              &["Second addresses first seed", "Second addresses second seed"]],
        )?;
```

### Độ sâu cuộc gọi

Các lệnh gọi chương trình chéo cho phép các chương trình gọi các chương trình khác một cách trực tiếp nhưng độ sâu hiện bị hạn chế ở mức 4.

### Reentrancy

Reentrancy hiện được giới hạn ở giới hạn tự đệ quy trực tiếp ở độ sâu cố định. Hạn chế này ngăn chặn các tình huống trong đó một chương trình có thể gọi một chương trình khác từ một trạng thái trung gian mà sau này nó có thể được gọi lại. Đệ quy trực tiếp cung cấp cho chương trình toàn quyền kiểm soát trạng thái của nó tại điểm mà nó được gọi trở lại.

## Địa chỉ có nguồn gốc từ chương trình

Địa chỉ có nguồn gốc từ chương trình cho phép sử dụng chữ ký được tạo theo chương trình khi [gọi giữa các chương trình](#cross-program-invocations).

Sử dụng một địa chỉ có nguồn gốc từ chương trình, một chương trình có thể được cấp quyền đối với một tài khoản và sau đó chuyển quyền đó cho một tài khoản khác. Điều này có thể thực hiện được vì chương trình có thể đóng vai trò là người ký trong giao dịch cung cấp quyền hạn.

Ví dụ: nếu hai người dùng muốn đặt cược vào kết quả của một trò chơi ở Solana, mỗi người phải chuyển tài sản cá cược của họ cho một số trung gian tuân theo thỏa thuận của họ. Hiện tại, không có cách nào để triển khai chương trình trung gian này như một chương trình ở Solana vì chương trình trung gian không thể chuyển tài sản cho người chiến thắng.

Khả năng này là cần thiết cho nhiều ứng dụng DeFi vì chúng yêu cầu tài sản phải được chuyển cho một đại lý ký quỹ cho đến khi một số sự kiện xảy ra xác định chủ sở hữu mới.

- Các sàn giao dịch phi tập trung chuyển tài sản giữa các lệnh đặt mua và đặt mua phù hợp.

- Đấu giá chuyển giao tài sản cho người chiến thắng.

- Trò chơi hoặc thị trường dự đoán thu thập và phân phối lại giải thưởng cho người chiến thắng.

Địa chỉ bắt nguồn từ chương trình:

1. Cho phép các chương trình kiểm soát các địa chỉ cụ thể, được gọi là địa chỉ chương trình, theo cách mà không người dùng bên ngoài nào có thể tạo ra các giao dịch hợp lệ với chữ ký cho các địa chỉ đó.

2. Cho phép các chương trình ký chương trình cho các địa chỉ chương trình có trong hướng dẫn được gọi thông qua [Lời mời chương trình chéo](#cross-program-invocations).

Với hai điều kiện trên, người dùng có thể chuyển một cách an toàn hoặc chỉ định quyền hạn của nội dung trên chuỗi cho các địa chỉ chương trình và sau đó chương trình có thể chỉ định quyền hạn đó ở nơi khác theo quyết định của mình.

### Private key cho địa chỉ chương trình

Một địa chỉ Chương trình không nằm trên đường cong ed25519 và do đó không có private key hợp lệ nào được liên kết với nó, và do đó, việc tạo chữ ký cho nó là không thể.  Mặc dù nó không có private key, nhưng nó có thể được chương trình sử dụng để đưa ra chỉ thị bao gồm địa chỉ Chương trình làm người ký.

### Địa chỉ chương trình được tạo dựa trên-hàm băm

Địa chỉ chương trình có nguồn gốc xác định từ một tập hợp các hạt giống và một id chương trình bằng cách sử dụng một hàm băm có khả năng chống hình ảnh trước 256-bit.  Địa chỉ chương trình không được nằm trên đường cong ed25519 để đảm bảo không có private key liên quan. Trong quá trình tạo, một lỗi sẽ được trả về nếu địa chỉ được tìm thấy nằm trên đường cong.  Có khoảng 50/50 thay đổi về điều này xảy ra đối với một bộ sưu tập hạt giống và id chương trình nhất định.  Nếu điều này xảy ra, một tập hợp hạt giống khác hoặc một hạt giống (hạt giống 8 bit bổ sung) có thể được sử dụng để tìm địa chỉ chương trình hợp lệ ngoài đường cong.

Địa chỉ chương trình xác định cho các chương trình tuân theo một đường dẫn dẫn xuất tương tự như Tài khoản được tạo với `SystemInstruction::CreateAccountWithSeed` nó được triển khai với `system_instruction::create_address_with_seed`.

Để tham khảo, cách triển khai như sau:

```rust,ignore
pub fn create_address_with_seed(
    base: &Pubkey,
    seed: &str,
    program_id: &Pubkey,
) -> Result<Pubkey, SystemError> {
    if seed.len() > MAX_ADDRESS_SEED_LEN {
        return Err(SystemError::MaxSeedLengthExceeded);
    }

    Ok(Pubkey::new(
        hashv(&[base.as_ref(), seed.as_ref(), program_id.as_ref()]).as_ref(),
    ))
}
```

Các chương trình có thể lấy ra một cách xác định bất kỳ số lượng địa chỉ nào bằng cách sử dụng hạt giống. Những hạt giống này có thể xác định một cách tượng trưng cách các địa chỉ được sử dụng.

Từ `Pubkey`::

```rust,ignore
/// Generate a derived program address
///     * seeds, symbolic keywords used to derive the key
///     * program_id, program that the address is derived for
pub fn create_program_address(
    seeds: &[&[u8]],
    program_id: &Pubkey,
) -> Result<Pubkey, PubkeyError>
```

### Sử dụng địa chỉ chương trình

Khách hàng có thể sử dụng chức năng `create_program_address` này để tạo địa chỉ đích.

```rust,ignore
// deterministically derive the escrow key
let escrow_pubkey = create_program_address(&[&["escrow"]], &escrow_program_id);

// construct a transfer message using that key
let message = Message::new(vec![
    token_instruction::transfer(&alice_pubkey, &escrow_pubkey, 1),
]);

// process the message which transfer one 1 token to the escrow
client.send_and_confirm_message(&[&alice_keypair], &message);
```

Các chương trình có thể sử dụng cùng một chức năng để tạo ra cùng một địa chỉ. Trong chức năng bên dưới, chương trình đưa một địa chỉ `token_instruction::transfer` từ chương trình như thể nó có private key để ký giao dịch.

```rust,ignore
fn transfer_one_token_from_escrow(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount]
) -> Result<()> {

    // User supplies the destination
    let alice_pubkey = keyed_accounts[1].unsigned_key();

    // Deterministically derive the escrow pubkey.
    let escrow_pubkey = create_program_address(&[&["escrow"]], program_id);

    // Create the transfer instruction
    let instruction = token_instruction::transfer(&escrow_pubkey, &alice_pubkey, 1);

    // The runtime deterministically derives the key from the currently
    // executing program ID and the supplied keywords.
    // If the derived address matches a key marked as signed in the instruction
    // then that key is accepted as signed.
    invoke_signed(&instruction,  &[&["escrow"]])?
}
```

### Hướng dẫn yêu cầu người ký

Các địa chỉ được tạo bằng `create_program_address` không thể phân biệt được với bất kỳ public key nào khác. Cách duy nhất để thời gian chạy xác minh rằng địa chỉ thuộc về một chương trình là chương trình cung cấp các hạt giống được sử dụng để tạo địa chỉ.

Thời gian chạy sẽ gọi nội bộ `create_program_address`, và so sánh kết quả với các địa chỉ được cung cấp trong hướng dẫn.

## Ví dụ

Tham khảo [Phát triển với Rust](developing/deployed-programs/../../../deployed-programs/developing-rust.md#examples) và [Phát triển với C](developing/deployed-programs/../../../deployed-programs/developing-c.md#examples) cho các ví dụ về cách sử dụng lệnh gọi chương trình chéo.
