---
title: "Tổng quát"
---

Các nhà phát triển có thể viết và triển khai các chương trình của riêng họ vào blockchain Solana.

[ví dụ Helloworld](examples.md#helloworld) là một nơi khởi đầu tốt để xem cách một chương trình được viết, xây dựng, triển khai và tương tác trên chuỗi,.

## Bộ lọc gói Berkley (BPF)

Các chương trình trên chuỗi của Solana được biên dịch thông qua [Trình biên dịch LLVM cơ sở hạ tầng](https://llvm.org/) thành [Định dạng có thể thực thi và có thể liên kết (ELF)](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format) có chứa một biến thể của bytecode [Bộ lọc gói Berkley (BPF)](https://en.wikipedia.org/wiki/Berkeley_Packet_Filter).

Bởi vì Solana sử dụng cơ sở hạ tầng trình biên dịch LLVM, một chương trình có thể được viết bằng bất kỳ ngôn ngữ lập trình nào có thể nhắm mục tiêu đến phần phụ trợ BPF của các LLVM. Solana hiện hỗ trợ viết chương trình bằng Rust và C/C++.

BPF cung cấp một [instruction set](https://github.com/iovisor/bpf-docs/blob/master/eBPF.md) có thể được thực thi trong một máy ảo được thông dịch hoặc dưới dạng các lệnh gốc được biên dịch nhanh chóng hiệu quả.

## Bản đồ bộ nhớ

Bản đồ bộ nhớ địa chỉ ảo được sử dụng bởi các chương trình Solana BPF được cố định và bố trí như sau

- Code chương trình bắt đầu từ 0x100000000
- Dữ liệu ngăn xếp bắt đầu ở 0x200000000
- Dữ liệu đống bắt đầu ở 0x300000000
- Các tham số đầu vào chương trình bắt đầu từ 0x400000000

Các địa chỉ ảo trên là địa chỉ bắt đầu các chương trình được cấp quyền truy cập vào một tập con của bản đồ bộ nhớ. Chương trình sẽ hoảng sợ nếu nó cố gắng đọc hoặc ghi vào một địa chỉ ảo mà nó không được cấp quyền truy cập và một `AccessViolation` lỗi sẽ được trả về có chứa địa chỉ và kích thước của vi phạm đã cố gắng.

## Ngăn xếp

BPF sử dụng khung ngăn xếp thay vì một con trỏ ngăn xếp biến đổi. Mỗi khung ngăn xếp có kích thước 4KB.

Nếu một chương trình vi phạm kích thước khung ngăn xếp đó, trình biên dịch sẽ báo cáo việc chạy quá mức như một cảnh báo.

For example: `Error: Function _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E Stack offset of -30728 exceeded max offset of -4096 by 26632 bytes, please minimize large stack variables`

Tin nhắn xác định biểu tượng nào đang vượt quá khung ngăn xếp của nó nhưng tên có thể bị lệch nếu đó là biểu tượng Rust hoặc C ++. Để gỡ rối một biểu tượng Rust, hãy sử dụng [rustfilt](https://github.com/luser/rustfilt). Cảnh báo trên đến từ một chương trình Rust, do đó, tên biểu tượng được gỡ bỏ là:

```bash
$ rustfilt _ZN16curve25519_dalek7edwards21EdwardsBasepointTable6create17h178b3d2411f7f082E
curve25519_dalek::edwards::EdwardsBasepointTable::create
```

Để gỡ rối một biểu tượng C ++, hãy sử dụng `c++filt` từ binutils.

Lý do cảnh báo được báo cáo thay vì lỗi là vì một số thùng phụ thuộc có thể bao gồm chức năng vi phạm các hạn chế khung ngăn xếp ngay cả khi chương trình không sử dụng chức năng đó. Nếu chương trình vi phạm kích thước ngăn xếp trong thời gian chạy, một `AccessViolation` lỗi sẽ được thông báo.

Khung ngăn xếp BPF chiếm một dải địa chỉ ảo bắt đầu từ 0x200000000.

## Độ sâu cuộc gọi

Các chương trình bị ràng buộc phải chạy nhanh và để tạo điều kiện thuận lợi cho việc này, ngăn xếp cuộc gọi của chương trình được giới hạn ở độ sâu tối đa là 64 khung hình.

## Heap

Các chương trình có quyền truy cập vào một đống thời gian chạy trực tiếp trong C hoặc thông qua các API Rust `alloc`. Để tạo điều kiện cho việc phân bổ nhanh chóng, một heap đơn giản 32KB được sử dụng. Heap không hỗ trợ `free` hoặc`realloc` vì vậy hãy sử dụng nó một cách khôn ngoan.

Bên trong, các chương trình có quyền truy cập vào vùng bộ nhớ 32KB bắt đầu từ địa chỉ ảo 0x300000000 và có thể triển khai một heap tùy chỉnh dựa trên nhu cầu cụ thể của chương trình.

- [Sử dụng heap chương trình Rust](developing-rust.md#heap)
- [Sử dụng heap chương trình C](developing-c.md#heap)

## Hỗ trợ Float

Programs support a limited subset of Rust's float operations, if a program attempts to use a float operation that is not supported, the runtime will report an unresolved symbol error.

Float operations are performed via software libraries, specifically LLVM's float builtins. Due to be software emulated they consume more compute units than integer operations. In general, fixed point operations are recommended where possible.

The Solana Program Library math tests will report the performance of some math operations: https://github.com/solana-labs/solana-program-library/tree/master/libraries/math

To run the test, sync the repo, and run:

`$ cargo test-bpf -- --nocapture --test-threads=1`

Recent results show the float operations take more instructions compared to integers equivalents. Fixed point implementations may vary but will also be less then the float equivalents:

```
         u64   f32
Multipy    8   176
Divide     9   219
```

## Dữ liệu ghi tĩnh

Các đối tượng được chia sẻ chương trình không hỗ trợ dữ liệu được chia sẻ có thể ghi. Các chương trình được chia sẻ giữa nhiều lần thực thi song song bằng cách sử dụng cùng một mã và dữ liệu chỉ-đọc được chia sẻ. Điều này có nghĩa là các nhà phát triển không nên đưa bất kỳ biến toàn cục hoặc biến tĩnh nào có thể ghi vào chương trình. Trong tương lai, cơ chế copy-on-write có thể được thêm vào để hỗ trợ dữ liệu có thể ghi.

## Bộ phận đã ký

Tập lệnh BPF không hỗ trợ [signed division](https://www.kernel.org/doc/html/latest/bpf/bpf_design_QA.html#q-why-there-is-no-bpf-sdiv-for-signed-divide-operation). Thêm một hướng dẫn phân chia có chữ ký là một cân nhắc.

## Loaders

Các chương trình được triển khai và thực thi bởi các bộ nạp thời gian chạy, hiện tại có hai bộ nạp được hỗ trợ là [Bộ nạp BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) và [Bộ nạp BPF không dùng nữa](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

Các loader có thể hỗ trợ các giao diện nhị phân ứng dụng khác nhau, do đó các nhà phát triển phải viết chương trình của họ và triển khai chúng cho cùng một loader. Nếu một chương trình được viết cho một loader được triển khai cho một loader khác, kết quả thường là `AccessViolation` lỗi do việc mô tả không khớp các tham số đầu vào của chương trình.

Đối với tất cả các mục đích thực tế, chương trình phải luôn được viết để nhắm mục tiêu loader BPF mới nhất và loader mới nhất là mặc định cho giao diện dòng lệnh và các API javascript.

Để biết thông tin cụ thể về ngôn ngữ về việc triển khai chương trình cho một loader cụ thể, hãy xem:

- [Các điểm nhập chương trình Rust](developing-rust.md#program-entrypoint)
- [Các điểm nhập chương trình C](developing-c.md#program-entrypoint)

### Triển khai

Triển khai chương trình BPF là quá trình tải lên một đối tượng được chia sẻ BPF vào dữ liệu của tài khoản chương trình và đánh dấu tài khoản có thể thực thi được. Một máy khách chia đối tượng chia sẻ BPF thành các phần nhỏ hơn và gửi chúng dưới dạng dữ liệu hướng dẫn của các hướng dẫn [`Write`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L13) đến trình tải nơi trình tải ghi dữ liệu đó vào dữ liệu tài khoản của chương trình. Sau khi nhận được tất cả các phần, máy khách sẽ gửi một lệnh [`Finalize`](https://github.com/solana-labs/solana/blob/bc7133d7526a041d1aaee807b80922baa89b6f90/sdk/program/src/loader_instruction.rs#L30) cho bộ nạp, bộ nạp sau đó xác nhận rằng dữ liệu BPF là hợp lệ và đánh dấu tài khoản chương trình là có thể _executable_ được. Khi tài khoản chương trình được đánh dấu là có thể thực thi, các giao dịch tiếp theo có thể đưa ra hướng dẫn để chương trình đó xử lý.

Khi một lệnh được hướng đến một chương trình BPF có thể thực thi được, trình nạp sẽ cấu hình môi trường thực thi của chương trình, tuần tự hóa các tham số đầu vào của chương trình, gọi điểm vào của chương trình và báo cáo bất kỳ lỗi nào gặp phải.

Để biết thêm thông tin, hãy xem [deploying](deploying.md)

### Thông số đầu vào Serialization

Bộ tải BPF tuần tự hóa các tham số đầu vào của chương trình thành một mảng byte sau đó được chuyển đến điểm nhập của chương trình, nơi chương trình chịu trách nhiệm giải mã hóa nó trên chuỗi. Một trong những thay đổi giữa trình tải không dùng nữa và trình tải hiện tại là các tham số đầu vào được tuần tự hóa theo cách dẫn đến các tham số khác nhau nằm trên các khoảng lệch được căn chỉnh trong mảng byte được căn chỉnh. Điều này cho phép triển khai deserialization trực tiếp tham chiếu mảng byte và cung cấp các con trỏ căn chỉnh cho chương trình.

Để biết thông tin cụ thể về ngôn ngữ về tuần tự hóa, hãy xem:

- [Chương trình Rust thông số deserialization](developing-rust.md#parameter-deserialization)
- [Tham số chương trình C giải phóng mặt bằng](developing-c.md#parameter-deserialization)

Trình tải mới nhất tuần tự hóa các tham số đầu vào của chương trình như sau (tất cả mã hóa ít endian):

- 8 byte số tài khoản không dấu
- Đối với mỗi tài khoản
  - 1 byte cho biết đây có phải là tài khoản trùng lặp hay không, nếu không phải là tài khoản trùng lặp thì giá trị là 0xff, nếu không giá trị là chỉ số của tài khoản mà nó là tài khoản trùng lặp.
  - 7 byte đệm
    - nếu không trùng lặp
      - 1 byte đệm
      - Boolean 1 byte, true nếu tài khoản là người ký
      - Boolean 1 byte, true nếu tài khoản có thể ghi
      - Boolean 1 byte, true nếu tài khoản có thể thực thi được
      - 4 byte đệm
      - 32 byte của public key tài khoản
      - 32 byte public key chủ sở hữu tài khoản
      - 8 byte số lamport chưa được đánh dấu thuộc sở hữu của tài khoản
      - 8 byte số byte dữ liệu tài khoản chưa được đánh dấu
      - x byte dữ liệu tài khoản
      - 10k byte đệm, được sử dụng để phân bổ lại
      - đủ đệm để căn chỉnh độ lệch thành 8 byte.
      - Kỷ nguyên thuê 8 byte
- 8 byte dữ liệu hướng dẫn chưa được đánh dấu
- x byte dữ liệu hướng dẫn
- 32 byte của id chương trình
