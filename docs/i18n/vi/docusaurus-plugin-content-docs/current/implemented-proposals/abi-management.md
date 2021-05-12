---
title: Quy trình quản lý Solana ABI
---

Tài liệu này đề xuất quy trình quản lý Solana ABI. Quy trình quản lý ABI là một thực hành kỹ thuật và một khung kỹ thuật hỗ trợ để tránh đưa ra những thay đổi ABI không tương thích ngoài ý muốn.

# Sự cố

Solana ABI (giao diện nhị phân với cụm) hiện chỉ được xác định ngầm bởi quá trình triển khai và đòi hỏi một con mắt rất cẩn thận để nhận thấy những thay đổi đột phá. Điều này gây khó khăn cho việc nâng cấp phần mềm trên một cụm hiện có mà không cần khởi động lại sổ cái.

# Yêu cầu và mục tiêu

- Những thay đổi ABI ngoài ý muốn có thể được phát hiện như lỗi CI một cách máy móc.
- Việc triển khai mới hơn phải có khả năng xử lý dữ liệu cũ nhất (kể từ genesis) khi chúng ta sử dụng mainnet.
- Mục tiêu của đề xuất này là bảo vệ ABI trong khi duy trì sự phát triển khá nhanh bằng cách chọn một quy trình máy móc thay vì một quy trình đánh giá do con người điều khiển rất lâu.
- Sau khi được ký bằng mật mã, blob dữ liệu phải giống hệt nhau, do đó, không thể cập nhật định dạng dữ liệu tại chỗ bất kể hệ thống trực tuyến đến và đi. Ngoài ra, xem xét khối lượng giao dịch tuyệt đối mà chúng tôi đang hướng tới để xử lý, tốt nhất là cập nhật tại chỗ hồi cứu là điều không mong muốn.

# Giải pháp

Thay vì sự thẩm định bằng mắt thường của con người, vốn được cho là thường xuyên thất bại, chúng ta cần một sự đảm bảo có hệ thống về việc không phá vỡ cụm khi thay đổi mã nguồn.

Vì mục đích đó, chúng tôi giới thiệu một cơ chế đánh dấu mỗi ABI liên quan trong mã nguồn (`struct`s, `enum`s) với thuộc tính `#[frozen_abi]` mới. Điều này nhận giá trị thông báo được mã hóa cứng bắt nguồn từ các loại trường của nó thông qua `ser::Serialize`. Và thuộc tính này tự động tạo một bài kiểm tra đơn vị để thử để phát hiện bất kỳ thay đổi không hoạt động nào đối với những thứ liên quan đến ABI được đánh dấu.

Tuy nhiên, việc phát hiện không thể hoàn thành; cho dù chúng ta phân tích tĩnh mã nguồn khó đến mức nào, vẫn có khả năng phá vỡ ABI. Ví dụ: điều này bao gồm không- `dẫn xuất` d viết tay `ser::Serialize`, thư viện cơ bản của thay đổi triển khai (ví dụ: `bincode`), sự khác biệt về kiến trúc CPU. Việc phát hiện các điểm không tương thích ABI có thể có này nằm ngoài phạm vi quản lý của ABI này.

# Định nghĩa

Mục/loại ABI: nhiều loại khác nhau được sử dụng để tuần tự hóa, bao gồm chung toàn bộ ABI cho bất kỳ thành phần hệ thống nào. Ví dụ: những kiểu đó bao gồm các `struct` và các `enum`.

Thông báo mục ABI: Một số hàm băm cố định bắt nguồn từ thông tin loại của các trường của mục ABI.

# Ví dụ

```patch
+#[frozen_abi(digest="eXSMM7b89VY72V...")]
 #[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
 pub struct Vote {
     /// A stack of votes starting with the oldest vote
     pub slots: Vec<Slot>,
     /// signature of the bank's state at the last slot
     pub hash: Hash,
 }
```

# Quy trình làm việc của nhà phát triển

Để biết thông báo cho các mục ABI mới, nhà phát triển có thể thêm `frozen_abi` với giá trị thông báo ngẫu nhiên và chạy các bài kiểm tra đơn vị và thay thế nó bằng phân loại chính xác từ tin nhắn lỗi kiểm tra xác nhận.

Nói chung, khi chúng ta thêm `freeze_abi` và thay đổi của nó được xuất bản trong kênh phát hành ổn định, phân loại của nó sẽ không bao giờ thay đổi. Nếu cần thay đổi như vậy, chúng ta nên chọn xác định một `struct` thích mới `FooV1`. Và nên tiếp cận luồng phát hành đặc biệt như hard fork.

# Nhận xét triển khai

Chúng ta sử dụng một số mức độ máy móc vĩ mô để tự động tạo các bài kiểm tra đơn vị và tính toán tổng hợp từ các mục ABI. Điều này có thể thực hiện được bằng cách sử dụng khéo léo `serde::Serialize` (`[1]`) và `any::type_name` (`[2]`). Đối với một tiền lệ cho việc triển khai tương tự, `ink` từ Parity Technologies `[3]` có thể là thông tin.

# Chi tiết triển khai

Mục tiêu của việc triển khai là phát hiện các thay đổi ABI ngoài ý muốn một cách tự động càng nhiều càng tốt. Để đạt được mục tiêu đó, việc tổng hợp thông tin cấu trúc ABI được tính toán với độ chính xác và độ ổn định cao nhất.

Khi kiểm tra thông báo ABI được chạy, nó sẽ tự động tính toán thông báo ABI bằng cách tiêu hóa đệ quy ABI của các trường của mục ABI, bằng cách sử dụng lại `serde` chức năng tuần tự hóa, macro proc và chuyên môn hóa chung. Và sau đó, kiểm tra xem `assert!` giá trị thông báo cuối cùng của nó có giống với giá trị được chỉ định trong thuộc tính`frozen_abi` hay không.

Để nhận ra điều đó, nó tạo ra một ví dụ mẫu của kiểu và một `Serializer` cá thể tùy chỉnh `serde` để duyệt đệ quy các trường của nó như thể tuần tự hóa ví dụ đó thành thực. Traversing này phải được thực hiện thông qua `serde` để thực sự nắm bắt được những loại dữ liệu thực sự sẽ được tuần tự bằng `serde`, thậm chí xem xét tùy chỉnh không `derive` `Serialize` triển khai đặc điểm.

# Quá trình tiêu hóa ABI

Phần này hơi phức tạp. Có ba phần phụ thuộc lẫn nhau: `AbiExample`, `AbiDigester` và `AbiEnumVisitor`.

Đầu tiên, bài kiểm tra được tạo tạo ra một ví dụ về kiểu đã tiêu hóa với một đặc điểm được gọi là `AbiExample`, sẽ được triển khai cho tất cả các kiểu đã tiêu hóa như `Serialize` và trả về `Self` giống như đặc điểm `Default`. Thông thường, nó được cung cấp thông qua chuyên môn hóa đặc điểm chung cho hầu hết các kiểu thông thường. Ngoài ra nó có thể `derive` cho `struct` và `enum` và có thể được viết tay nếu cần thiết.

`Serializer` tùy chỉnh được gọi là `AbiDigester`. Và khi nó được gọi bởi `serde` để tuần tự hóa một số dữ liệu, nó sẽ thu thập một cách đệ quy thông tin ABI nhiều nhất có thể. Trạng thái bên trong của `AbiDigester` cho thông báo ABI được cập nhật khác nhau tùy thuộc vào loại dữ liệu. Logic này được chuyển hướng cụ thể thông qua một đặc điểm được gọi là `AbiEnumVisitor` cho từng kiểu `enum`. Như tên cho thấy, không cần phải triển khai `AbiEnumVisitor` cho các kiểu khác.

Để tóm tắt tác động qua lại này, `serde` xử lý luồng điều khiển tuần tự hóa đệ quy song song với `AbiDigester`. Điểm vào ban đầu trong các bài kiểm tra và `AbiDigester` con sử dụng `AbiExample` đệ quy để tạo một biểu đồ phân cấp đối tượng mẫu. Và `AbiDigester` sử dụng `AbiEnumVisitor` để truy vấn thông tin ABI thực tế bằng cách sử dụng mẫu đã xây dựng.

`Default` không đủ cho `AbiExample`. `::default()` của nhiều bộ sưu tập là trống, tuy nhiên, chúng tôi muốn phân tích chúng bằng các mục thực tế. Và, không thể thực hiện tiêu hóa ABI chỉ với `AbiEnumVisitor`. `AbiExample` là bắt buộc vì cần có phiên bản thực tế của kiểu để thực sự duyệt dữ liệu qua `serde`.

Mặt khác, cũng không thể thực hiện tiêu hóa ABI chỉ với `AbiExample`. `AbiEnumVisitor` là bắt buộc vì không thể duyệt tất cả các biến thể của `enum` chỉ với một biến thể duy nhất của nó làm ví dụ ABI.

Thông tin có thể tiêu hóa:

- tên kiểu rust
- Tên kiểu dữ liệu của `serde`
- tất cả các trường trong `struct`
- tất cả các biến thể trong `enum`
- `struct`: (`struct {...}`) và tuple-style (`struct(...)`) thông thường
- `enum`: các biến thể thông thường và các kiểu `struct`- và `tuple`- styles.
- thuộc tính: `serde(serialize_with=...)` và `serde(skip)`

Thông tin không thể tiêu hóa được:

- Bất kỳ đường dẫn code tuần tự hóa tùy chỉnh nào không được chạm vào mẫu được cung cấp bởi `AbiExample`. (về mặt kỹ thuật là không thể)
- các loại chung (phải là loại cụ thể; sử dụng `frozen_abi` trên bí danh loại cụ thể)

# Người giới thiệu

1. [(De) Serialization với thông tin loại · Sự cố # 1095 · serde-rs/serde](https://github.com/serde-rs/serde/issues/1095#issuecomment-345483479)
2. [`std::any::type_name` - Rust](https://doc.rust-lang.org/std/any/fn.type_name.html)
3. [Mực của Parity để viết các hợp đồng thông minh](https://github.com/paritytech/ink)
