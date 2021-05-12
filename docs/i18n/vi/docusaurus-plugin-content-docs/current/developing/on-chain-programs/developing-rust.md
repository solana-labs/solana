---
title: "Phát triển với Rust"
---

Solana hỗ trợ viết các chương trình trên chuỗi bằng ngôn ngữ lập trình [Rust](https://www.rust-lang.org/).

## Bố cục dự án

Các chương trình Solana Rust tuân theo [bố cục dự án Rust](https://doc.rust-lang.org/cargo/guide/project-layout.html) điển hình:

```
/inc/
/src/
/Cargo.toml
```

Nhưng cũng phải bao gồm:

```
/Xargo.toml
```

Nó phải chứa:

```
[target.bpfel-unknown-unknown.dependencies.std]
features = []
```

Solana Rust programs may depend directly on each other in order to gain access to instruction helpers when making [cross-program invocations](developing/programming-model/calling-between-programs.md#cross-program-invocations). Khi làm như vậy, điều quan trọng là không kéo các ký hiệu điểm vào của chương trình phụ thuộc vào vì chúng có thể xung đột với ký hiệu của chương trình. To avoid this, programs should define an `exclude_entrypoint` feature in `Cargo.toml` and use to exclude the entrypoint.

- [Xác định tính năng](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/Cargo.toml#L12)
- [Loại trừ điểm vào](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token/program/src/lib.rs#L12)

Sau đó, khi các chương trình khác bao gồm chương trình này như một phần phụ thuộc, chúng nên làm như vậy bằng cách sử dụng tính năng `exclude_entrypoint`.

- [Bao gồm không có điểm vào](https://github.com/solana-labs/solana-program-library/blob/a5babd6cbea0d3f29d8c57d2ecbbd2a2bd59c8a9/token-swap/program/Cargo.toml#L19)

## Sự phụ thuộc của dự án

Ở mức tối thiểu, các chương trình Solana Rust phải kéo vào thùng [chương trình-solana](https://crates.io/crates/solana-program).

Các chương trình Solana BPF có một số [hạn chế](#restrictions) có thể ngăn việc đưa một số thùng vào làm phụ thuộc hoặc yêu cầu xử lý đặc biệt.

Ví dụ:

- Các thùng yêu cầu kiến ​​trúc là một tập hợp con của các thùng được hỗ trợ bởi chuỗi công cụ chính thức. Không có cách giải quyết nào cho điều này trừ khi thùng đó được chia nhỏ và BPF được thêm vào để kiểm tra kiến ​​trúc đó.
- Các thùng có thể phụ thuộc vào `rand` không được hỗ trợ trong môi trường chương trình xác định của Solana. Để bao gồm một `rand` thùng phụ thuộc, hãy tham khảo [Tùy thuộc vào Rand](#depending-on-rand).
- Các thùng có thể làm tràn ngăn xếp ngay cả khi mã tràn ngăn xếp không được bao gồm trong chính chương trình. Để biết thêm thông tin, hãy tham khảo [Stack](overview.md#stack).

## Cách xây dựng

Đầu tiên thiết lập môi trường:

- Cài đặt bản Rust ổn định mới nhất từ https://rustup.rs/
- Cài đặt các công cụ dòng lệnh Solana mới nhất từ https://docs.solana.com/cli/install-solana-cli-tools

Cấu trúc hàng hóa thông thường có sẵn để xây dựng các chương trình chống lại máy chủ của bạn, có thể được sử dụng để kiểm tra đơn vị:

```bash
$ cargo build
```

Để xây dựng một chương trình cụ thể, chẳng hạn như Mã thông báo SPL, cho mục tiêu Solana BPF có thể được triển khai cho cụm:

```bash
$ cd <the program directory>
$ cargo build-bpf
```

## Cách kiểm tra

Các chương trình Solana có thể được kiểm tra đơn vị thông qua cơ chế `cargo test` truyền thống bằng cách thực hiện trực tiếp các chức năng của chương trình.

Để giúp tạo điều kiện thử nghiệm trong một môi trường phù hợp hơn với một cụm trực tiếp, các nhà phát triển có thể sử dụng thùng [`program-test`](https://crates.io/crates/solana-program-test). Thùng `program-test` khởi động phiên bản cục bộ của thời gian chạy và cho phép các bài kiểm tra gửi nhiều giao dịch trong khi vẫn giữ trạng thái trong suốt thời gian kiểm tra.

Để biết thêm thông tin, [bài kiểm tra trong ví dụ sysvar](https://github.com/solana-labs/solana-program-library/blob/master/examples/rust/sysvar/tests/functional.rs) cho thấy cách chương trình gửi và xử lý một lệnh chứa tài khoản syavar.

## Điểm vào chương trình

Các chương trình xuất một biểu tượng điểm vào đã biết mà thời gian chạy Solana sẽ tra cứu và gọi khi gọi một chương trình. Solana hỗ trợ [nhiều phiên bản của trình tải BPF](overview.md#versions) và các điểm nhập có thể khác nhau giữa chúng. Các chương trình phải được viết và triển khai cho cùng một trình tải. Để biết thêm chi tiết, hãy xem [overview](overview#loaders).

Hiện tại, có hai trình tải được hỗ trợ là [Trình tải BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) và [Trình tải BPF không được dùng nữa](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

Cả hai đều có cùng định nghĩa điểm nhập thô, sau đây là ký hiệu thô mà thời gian chạy tra cứu và gọi:

```rust
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64;
```

Điểm nhập này nhận một mảng byte chung chứa các tham số chương trình được tuần tự hóa (id chương trình, các tài khoản, dữ liệu hướng dẫn, v. v.). Để giải mã hóa các tham số, mỗi trình tải chứa macro trình bao bọc của riêng nó xuất khẩu điểm nhập thô, giải mã hóa các tham số, gọi hàm xử lý lệnh do người dùng xác định và trả về kết quả.

Bạn có thể tìm thấy các macro điểm vào tại đây:

- [Macro điểm vào của Trình tải BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L46)
- [Trình tải BPF không dùng macro của điểm vào](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L37)

Chương trình đã xác định chức năng xử lý lệnh mà lệnh gọi macro điểm vào phải có dạng sau:

```rust
pub type ProcessInstruction =
    fn(program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult;
```

Tham khảo [việc sử dụng điểm vào của helloworld](https://github.com/solana-labs/example-helloworld/blob/c1a7247d87cd045f574ed49aec5d160aefc45cf2/src/program-rust/src/lib.rs#L15) như một ví dụ về cách mọi thứ khớp với nhau.

### Deserialization tham số

Mỗi trình tải cung cấp một chức năng trợ giúp giúp giải mã hóa các tham số đầu vào của chương trình thành các kiểu Rust. Các macro điểm vào sẽ tự động gọi trình trợ giúp deserialization:

- [Deserialization Trình tải BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint.rs#L104)
- [Trình tải BPF không được dùng nữa trong quá trình deserialization](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/entrypoint_deprecated.rs#L56)

Một số chương trình có thể muốn tự thực hiện quá trình deserialization và chúng có thể bằng việc cung cấp cách triển khai [raw entrypoint](#program-entrypoint) của riêng mình. Hãy lưu ý rằng các hàm deserialization được cung cấp giữ lại các tham chiếu trở lại mảng byte tuần tự hóa cho các biến mà chương trình được phép sửa đổi (các lamport, dữ liệu tài khoản). Lý do cho điều này là khi trở lại trình tải sẽ đọc những sửa đổi đó để chúng có thể được cam kết. Nếu một chương trình thực hiện chức năng deserialization của riêng chúng, chúng cần đảm bảo rằng bất kỳ sửa đổi nào mà chương trình muốn cam kết sẽ được ghi lại vào mảng byte đầu vào.

Bạn có thể tìm thấy chi tiết về cách trình tải tuần tự hóa các đầu vào chương trình trong các tài liệu [Tuần Tự Hóa Thông Số Đầu Vào](overview.md#input-parameter-serialization).

### Các loại dữ liệu

Macro điểm vào của trình tải gọi hàm bộ xử lý lệnh do chương trình xác định với các tham số sau:

```rust
program_id: &Pubkey,
accounts: &[AccountInfo],
instruction_data: &[u8]
```

Id chương trình là public key của chương trình hiện đang thực thi.

Các tài khoản là một phần có thứ tự của các tài khoản được tham chiếu bởi hướng dẫn và được biểu thị dưới dạng cấu trúc [AccountInfo](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/account_info.rs#L10). Vị trí của một tài khoản trong mảng biểu thị ý nghĩa của nó, ví dụ, khi chuyển các lamport, một lệnh có thể xác định tài khoản đầu tiên là nguồn và tài khoản thứ hai là đích.

Các thành viên của `AccountInfo` là cấu trúc chỉ-đọc ngoại trừ `các lamport` và `data`. Cả hai đều có thể được chương trình sửa đổi theo [chính sách thực thi thời gian chạy](developing/programming-model/accounts.md#policy). Cả hai thành viên này đều được bảo vệ bởi cấu trúc Rust `RefCell`, vì vậy chúng phải được mượn để đọc hoặc ghi chúng. Lý do cho điều này là cả hai đều trỏ về mảng byte đầu vào ban đầu, nhưng có thể có nhiều mục nhập trong lát tài khoản trỏ đến cùng một tài khoản. Việc sử dụng `RefCell` đảm bảo rằng chương trình không vô tình thực hiện đọc/ghi chồng chéo lên cùng một dữ liệu cơ bản thông qua nhiều cấu trúc `AccountInfo`. Nếu một chương trình triển khai chức năng deserialization của riêng họ, thì nên cẩn thận để xử lý các tài khoản trùng lặp một cách thích hợp.

Dữ liệu hướng dẫn là mảng byte mục đích chung từ [dữ liệu hướng dẫn của lệnh](developing/programming-model/transactions.md#instruction-data) đang được xử lý.

## Heap

Các chương trình Rust thực hiện heap trực tiếp bằng cách xác định một tùy chỉnh [`global_allocator`](https://github.com/solana-labs/solana/blob/8330123861a719cd7a79af0544617896e7f00ce3/sdk/program/src/entrypoint.rs#L50)

Các chương trình có thể triển khai `global_allocator` của riêng mình dựa trên nhu cầu cụ thể của chương trình. Tham khảo [ví dụ về heap tùy chỉnh](#examples) để biết thêm thông tin.

## Những hạn chế

Các chương trình Rust trên chuỗi hỗ trợ hầu hết libstd, libcore và liballoc của Rust, cũng như nhiều thùng của bên thứ ba.

Có một số hạn chế vì các chương trình này chạy trong điều kiện hạn chế về tài nguyên, môi trường đơn luồng và phải xác định:

- Không có quyền truy cập vào
  - `rand`
  - `std::fs`
  - `std::net`
  - `std::os`
  - `std::future`
  - `std::net`
  - `std::process`
  - `std::sync`
  - `std::task`
  - `std::thread`
  - `std::time`
- Quyền truy cập hạn chế vào:
  - `std::hash`
  - `std::os`
- Bincode cực kỳ tốn kém về mặt tính toán trong cả chu kỳ và độ sâu cuộc gọi và nên tránh
- Nên tránh định dạng chuỗi vì nó cũng tốn kém về mặt tính toán.
- Không hỗ trợ cho `println!`, `print!`, [trình trợ giúp ghi nhật ký](#logging) của Solana nên được sử dụng thay thế.
- Thời gian chạy thực thi một giới hạn về số lượng lệnh mà một chương trình có thể thực hiện trong quá trình xử lý một lệnh. See [computation budget](developing/programming-model/runtime.md#compute-budget) for more information.

## Tùy thuộc vào Rand

Các chương trình bị ràng buộc để chạy một cách xác định, vì vậy các số ngẫu nhiên không có sẵn. Đôi khi một chương trình có thể phụ thuộc vào một thùng phụ thuộc vào `rand` ngay cả khi chương trình không sử dụng bất kỳ chức năng số ngẫu nhiên nào. Nếu một chương trình phụ thuộc vào `rand`, quá trình biên dịch sẽ không thành công vì không có `get-random` hỗ trợ cho Solana. Lỗi thường sẽ giống như sau:

```
error: target is not supported, for more information see: https://docs.rs/getrandom/#unsupported-targets
   --> /Users/jack/.cargo/registry/src/github.com-1ecc6299db9ec823/getrandom-0.1.14/src/lib.rs:257:9
    |
257 | /         compile_error!("\
258 | |             target is not supported, for more information see: \
259 | |             https://docs.rs/getrandom/#unsupported-targets\
260 | |         ");
    | |___________^
```

Để khắc phục sự cố phụ thuộc này, hãy thêm phần phụ thuộc sau vào `Cargo.toml` của chương trình:

```
getrandom = { version = "0.1.14", features = ["dummy"] }
```

## Ghi nhật ký

Macro `println!` của Rust đắt về mặt tính toán và không được hỗ trợ. Thay vào đó, macro trợ giúp [`msg!`](https://github.com/solana-labs/solana/blob/6705b5a98c076ac08f3991bb8a6f9fcb280bf51e/sdk/program/src/log.rs#L33) được cung cấp.

`msg!` có hai dạng:

```rust
msg!("A string");
```

hoặc là

```rust
msg!(0_64, 1_64, 2_64, 3_64, 4_64);
```

Cả hai biểu mẫu đều xuất kết quả vào nhật ký chương trình. Nếu một chương trình muốn như vậy, họ có thể mô phỏng `println!` bằng cách sử dụng `format!`:

```rust
msg!("Some variable: {:?}", variable);
```

Phần [gỡ lỗi](debugging.md#logging) có thêm thông tin về cách làm việc với nhật ký chương trình. [Các ví dụ về Rust](#examples) chứa một ví dụ ghi nhật ký.

## Hoảng loạn

Các Rust `panic!`, `assert!` và kết quả hoảng loạn nội bộ được in vào [các nhật ký chương trình](debugging.md#logging) theo mặc định.

```
INFO  solana_runtime::message_processor] Finalized account CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Call BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ
INFO  solana_runtime::message_processor] Program log: Panicked at: 'assertion failed: `(left == right)`
      left: `1`,
     right: `2`', rust/panic/src/lib.rs:22:5
INFO  solana_runtime::message_processor] BPF program consumed 5453 of 200000 units
INFO  solana_runtime::message_processor] BPF program CGLhHSuWsp1gT4B7MY2KACqp9RUwQRhcUFfVSuxpSajZ failed: BPF program panicked
```

### Trình xử lý hoảng loạn tùy chỉnh

Các chương trình có thể ghi đè trình xử lý hoảng loạn mặc định bằng cách cung cấp triển khai của riêng chúng.

Trước tiên, hãy xác định tính năng `custom-panic` trong `Cargo.toml` của các chương trình

```toml
[features]
default = ["custom-panic"]
custom-panic = []
```

Sau đó, cung cấp triển khai tùy chỉnh của trình xử lý hoảng loạn:

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    solana_program::msg!("program custom panic enabled");
    solana_program::msg!("{}", info);
}
```

Trong đoạn trích trên, việc triển khai mặc định được hiển thị, nhưng các nhà phát triển có thể thay thế nó bằng một thứ gì đó phù hợp hơn với nhu cầu của họ.

Một trong những tác dụng phụ của việc hỗ trợ tin nhắn hoảng loạn đầy đủ theo mặc định là các chương trình phải chịu chi phí kéo thêm việc triển khai `libstd` của Rust vào đối tượng được chia sẻ của chương trình. Các chương trình điển hình sẽ thu hút một lượng lớn `libstd` và có thể không nhận thấy sự gia tăng nhiều về kích thước đối tượng được chia sẻ. Nhưng các chương trình cố gắng rõ ràng là rất nhỏ bằng cách tránh `libstd` có thể gây ra tác động đáng kể (~ 25kb). Để loại bỏ tác động đó, các chương trình có thể cung cấp trình xử lý hoảng loạn tùy chỉnh của riêng họ với một triển khai trống.

```rust
#[cfg(all(feature = "custom-panic", target_arch = "bpf"))]
#[no_mangle]
fn custom_panic(info: &core::panic::PanicInfo<'_>) {
    // Do nothing to save space
}
```

## Tính toán ngân sách

Sử dụng lệnh gọi hệ thống [`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/program/src/log.rs#L102) để ghi lại một tin nhắn có chứa số lượng đơn vị tính toán còn lại mà chương trình có thể sử dụng trước khi quá trình thực thi bị tạm dừng

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## ELF Dump

Nội bộ đối tượng chia sẻ BPF có thể được kết xuất vào một tệp văn bản để hiểu rõ hơn về thành phần của các chương trình và những gì nó có thể đang làm trong thời gian chạy. Dump sẽ chứa cả thông tin ELF cũng như danh sách tất cả các ký hiệu và hướng dẫn thực hiện chúng. Một số tin nhắn nhật ký lỗi của trình tải BPF sẽ tham chiếu đến các số hướng dẫn cụ thể nơi xảy ra lỗi. Các tham chiếu này có thể được tra cứu trong ELF dump để xác định hướng dẫn vi phạm và ngữ cảnh của nó.

Để tạo tệp dump:

```bash
$ cd <program directory>
$ cargo build-bpf --dump
```

## Ví dụ

[ Kho lưu trữ github của Thư viện Chương trình Solana](https://github.com/solana-labs/solana-program-library/tree/master/examples/rust) chứa một bộ sưu tập các ví dụ về Rust.
