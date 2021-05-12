---
title: "Phát triển trên C"
---

Solana hỗ trợ viết các chương trình trên chuỗi sử dụng ngôn ngữ lập trình C và C ++.

## Bố cục dự án

Các dự án C được trình bày như sau:

```
/src/<program name>
/makefile
```

`makefile` phải chứa những thứ sau đây:

```bash
OUT_DIR := <path to place to resulting shared object>
include ~/.local/share/solana/install/active_release/bin/sdk/bpf/c/bpf.mk
```

Bpf-sdk có thể không nằm ở vị trí chính xác được chỉ định ở trên nhưng nếu bạn thiết lập môi trường của mình theo [Cách xây dựng](#how-to-build) thì nó sẽ có.

Hãy xem [helloworld](https://github.com/solana-labs/example-helloworld/tree/master/src/program-c) để biết thêm ví dụ về chương trình C.

## Cách xây dựng

Đầu tiên thiết lập môi trường:

- Cài đặt bản Rust ổn định mới nhất từ https://rustup.rs
- Cài đặt các công cụ dòng lệnh Solana mới nhất từ https://docs.solana.com/cli/install-solana-cli-tools

Sau đó, xây dựng bằng cách sử dụng make:

```bash
make -C <program directory>
```

## Cách kiểm tra

Solana sử dụng khung kiểm tra [Tiêu chuẩn](https://github.com/Snaipe/Criterion) và kiểm tra được thực hiện mỗi khi chương trình được xây dựng [Làm thế nào để Xây dựng](#how-to-build)].

Để thêm các bài kiểm tra, hãy tạo một tệp mới bên cạnh tệp nguồn của bạn có tên `test_<program name>.c` và điền vào nó các trường hợp kiểm tra tiêu chuẩn. Để biết ví dụ, hãy xem [các bài kiểm tra helloworld C](https://github.com/solana-labs/example-helloworld/blob/master/src/program-c/src/helloworld/test_helloworld.c) hoặc [các tài liệu tiêu chuẩn](https://criterion.readthedocs.io/en/master) để biết thông tin về cách viết một trường hợp kiểm tra thử.

## Điểm vào chương trình

Các chương trình xuất một biểu tượng điểm vào đã biết mà thời gian chạy Solana sẽ tra cứu và gọi khi gọi một chương trình. Solana hỗ trợ [nhiều phiên bản của trình tải BPF](overview.md#versions) và các điểm vào có thể khác nhau giữa chúng. Các chương trình phải được viết và triển khai cho cùng một trình tải. Để biết thêm chi tiết, hãy xem [tổng quan](overview#loaders).

Hiện tại, có hai trình tải được hỗ trợ là [Trình tải BPF](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader.rs#L17) và [Trình tải BPF không được dùng nữa](https://github.com/solana-labs/solana/blob/7ddf10e602d2ed87a9e3737aa8c32f1db9f909d8/sdk/program/src/bpf_loader_deprecated.rs#L14)

Cả hai đều có cùng định nghĩa điểm vào thô, sau đây là ký hiệu thô mà thời gian chạy tra cứu và gọi:

```c
extern uint64_t entrypoint(const uint8_t *input)
```

Điểm vào này nhận một mảng byte chung chứa các tham số chương trình được tuần tự hóa (id chương trình, tài khoản, dữ liệu hướng dẫn, v. v.). Để giải mã hóa các thông số, mỗi bộ tải chứa [chức năng trợ giúp](#Serialization) của riêng nó.

Tham khảo [việc sử dụng điểm vào của helloworld](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L37) như một ví dụ về cách mọi thứ khớp với nhau.

### Serialization

Tham khảo [cách sử dụng chức năng deserialization của helloworld](https://github.com/solana-labs/example-helloworld/blob/bc0b25c0ccebeff44df9760ddb97011558b7d234/src/program-c/src/helloworld/helloworld.c#L43).

Mỗi trình tải cung cấp một chức năng trợ giúp giải mã hóa các tham số đầu vào của chương trình thành các loại C:

- [Trình tải BPF deserialization](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L304)
- [Trình tải BPF không được dùng nữa trong quá trình deserialization](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/deserialize_deprecated.h#L25)

Một số chương trình có thể muốn tự thực hiện quá trình deserialization và chúng có thể bằng việc cung cấp cách triển khai [raw entrypoint](#program-entrypoint) của riêng mình. Hãy lưu ý rằng các hàm deserialization được cung cấp giữ lại các tham chiếu trở lại mảng byte tuần tự hóa cho các biến mà chương trình được phép sửa đổi (các lamport, dữ liệu tài khoản). Lý do cho điều này là khi trở lại trình tải sẽ đọc những sửa đổi đó để chúng có thể được cam kết. Nếu một chương trình thực hiện chức năng deserialization của riêng chúng, chúng cần đảm bảo rằng bất kỳ sửa đổi nào mà chương trình muốn cam kết sẽ được ghi lại vào mảng byte đầu vào.

Bạn có thể tìm thấy chi tiết về cách trình tải tuần tự hóa các đầu vào chương trình trong các tài liệu [Tuần Tự Hóa Thông Số Đầu Vào](overview.md#input-parameter-serialization).

## Các loại dữ liệu

Chức năng trợ giúp deserialization của trình tải điền vào cấu trúc [SolParameters](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L276):

```c
/**
 * Structure that the program's entrypoint input data is deserialized into.
 */
typedef struct {
  SolAccountInfo* ka; /** Pointer to an array of SolAccountInfo, must already
                          point to an array of SolAccountInfos */
  uint64_t ka_num; /** Number of SolAccountInfo entries in `ka` */
  const uint8_t *data; /** pointer to the instruction data */
  uint64_t data_len; /** Length in bytes of the instruction data */
  const SolPubkey *program_id; /** program_id of the currently executing program */
} SolParameters;
```

'ka' là một mảng có thứ tự gồm các tài khoản được tham chiếu bởi hướng dẫn và được biểu diễn dưới dạng cấu trúc [SolAccountInfo](https://github.com/solana-labs/solana/blob/8415c22b593f164020adc7afe782e8041d756ddf/sdk/bpf/c/inc/solana_sdk.h#L173). Vị trí của một tài khoản trong mảng biểu thị ý nghĩa của nó, ví dụ, khi chuyển các lamport, một lệnh có thể xác định tài khoản đầu tiên là nguồn và tài khoản thứ hai là đích.

Các thành viên của `AccountInfo` là cấu trúc chỉ-đọc ngoại trừ `các lamport` và `data`. Cả hai đều có thể được chương trình sửa đổi bằng [chính sách thực thi thời gian chạy](developing/programming-model/accounts.md#policy). Khi một lệnh tham chiếu đến cùng một tài khoản nhiều lần, có thể có các mục nhập `SolAccountInfo` trùng lặp trong mảng nhưng cả hai đều trỏ về mảng byte đầu vào ban đầu. Một chương trình nên xử lý những trường hợp này một cách tế nhị để tránh việc đọc/ghi chồng chéo vào cùng một bộ đệm. Nếu một chương trình triển khai chức năng deserialization của riêng họ, thì nên cẩn thận để xử lý các tài khoản trùng lặp một cách thích hợp.

`data` là mảng byte mục đích chung từ [dữ liệu hướng dẫn của lệnh](developing/programming-model/transactions.md#instruction-data) đang được xử lý.

`program_id` là public key của chương trình hiện đang thực thi.

## Heap

Chương trình C có thể cấp phát bộ nhớ thông qua lệnh gọi hệ thống [`calloc`](https://github.com/solana-labs/solana/blob/c3d2d2134c93001566e1e56f691582f379b5ae55/sdk/bpf/c/inc/solana_sdk.h#L245) hoặc triển khai heap của riêng họ trên đầu vùng heap 32KB bắt đầu từ địa chỉ ảo x300000000. Vùng heap cũng được sử dụng bởi `calloc` vì vậy nếu một chương trình triển khai heap của riêng họ, thì sẽ không được gọi ` <code>calloc`.

## Ghi nhật ký

Thời gian chạy cung cấp hai lệnh gọi hệ thống lấy dữ liệu và ghi dữ liệu đó vào nhật ký chương trình.

- [`sol_log(const char*)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L128)
- [`sol_log_64(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)`](https://github.com/solana-labs/solana/blob/d2ee9db2143859fa5dc26b15ee6da9c25cc0429c/sdk/bpf/c/inc/solana_sdk.h#L134)

Phần [gỡ lỗi](debugging.md#logging) có thêm thông tin về cách làm việc với nhật ký chương trình.

## Tính toán ngân sách

Sử dụng lệnh gọi hệ thống [`sol_log_compute_units()`](https://github.com/solana-labs/solana/blob/d3a3a7548c857f26ec2cb10e270da72d373020ec/sdk/bpf/c/inc/solana_sdk.h#L140) để ghi lại một tin nhắn có chứa số lượng đơn vị tính toán còn lại mà chương trình có thể sử dụng trước khi quá trình thực thi bị tạm dừng

See [compute budget](developing/programming-model/runtime.md#compute-budget) for more information.

## ELF Dump

Nội bộ đối tượng chia sẻ BPF có thể được kết xuất vào một tệp văn bản để hiểu rõ hơn về thành phần của các chương trình và những gì nó có thể đang làm trong thời gian chạy. Dump sẽ chứa cả thông tin ELF cũng như danh sách tất cả các ký hiệu và hướng dẫn thực hiện chúng. Một số tin nhắn nhật ký lỗi của trình tải BPF sẽ tham chiếu đến các số hướng dẫn cụ thể nơi xảy ra lỗi. Các tham chiếu này có thể được tra cứu trong ELF dump để xác định hướng dẫn vi phạm và ngữ cảnh của nó.

Để tạo tệp dump:

```bash
$ cd <program directory>
$ make dump_<program name>
```

## Ví dụ

[ Kho lưu trữ github của Thư viện Chương trình Solana](https://github.com/solana-labs/solana-program-library/tree/master/examples/c)chứa một bộ sưu tập các ví dụ về C
