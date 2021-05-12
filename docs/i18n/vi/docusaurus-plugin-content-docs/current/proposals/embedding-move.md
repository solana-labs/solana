---
title: Embedding the Move Language
---

## Sự cố

Solana cho phép các nhà phát triển viết các chương trình trên chuỗi bằng các ngôn ngữ lập trình mục đích chung như C hoặc Rust, những chương trình đó chứa các cơ chế dành riêng cho Solana. Ví dụ, không có một chuỗi nào khác yêu cầu các nhà phát triển tạo một mô-đun Rust với một chức năng `process_instruction(KeyedAccounts)`. Trên thực tế, Solana nên cung cấp cho các nhà phát triển ứng dụng nhiều tùy chọn di động hơn.

Cho đến hiện tại, không có blockchain phổ biến nào cung cấp một ngôn ngữ có thể cho thấy giá trị của [thời gian chạy](../validator/runtime.md) song song khổng lồ của Solana. Ví dụ, hợp đồng solidity không tách các tham chiếu đến dữ liệu được chia sẻ khỏi code hợp đồng và do đó cần phải được thực thi tuần tự để đảm bảo hành vi xác định. Trên thực tế, chúng tôi thấy rằng các blockchain dựa trên EVM được tối ưu hóa mạnh mẽ nhất dường như đạt đỉnh chỉ khoảng 1,200 TPS - một phần nhỏ so với những gì Solana có thể làm. Mặt khác, dự án Libra đã thiết kế một ngôn ngữ lập trình trên chuỗi được gọi là Move phù hợp hơn để thực hiện song song. Giống như thời gian chạy của Solana, các chương trình Move phụ thuộc vào các tài khoản cho tất cả các trạng thái được chia sẻ.

Sự khác biệt lớn nhất về thiết kế giữa thời gian chạy của Solana và Move VM của Libra là cách chúng quản lý các lệnh gọi an toàn giữa các mô-đun. Solana sử dụng phương pháp tiếp cận hệ điều hành và Libra sử dụng phương pháp tiếp cận ngôn ngữ dành riêng cho miền. Trong thời gian chạy, một mô-đun phải bẫy trở lại thời gian chạy để đảm bảo mô-đun của người gọi không ghi vào dữ liệu thuộc sở hữu của callee. Tương tự như vậy, khi callee hoàn thành, nó phải bẫy lại thời gian chạy để đảm bảo callee không ghi vào dữ liệu thuộc sở hữu của người gọi. Mặt khác, Move bao gồm một hệ thống loại nâng cao cho phép các kiểm tra này được chạy bởi trình xác minh bytecode của nó. Bởi vì Move bytecode có thể được xác minh, chi phí xác minh chỉ phải thanh toán một lần, tại thời điểm mô-đun được tải trên chuỗi. Trong thời gian chạy, chi phí được thanh toán mỗi khi một giao dịch chéo giữa các mô-đun. Sự khác biệt về tinh thần cũng tương tự như sự khác biệt giữa một ngôn ngữ kiểu động như Python so với một ngôn ngữ kiểu tĩnh như Java. Thời gian chạy của Solana cho phép các ứng dụng được viết bằng các ngôn ngữ lập trình mục đích chung, nhưng điều đó đi kèm với chi phí kiểm tra thời gian chạy khi nhảy giữa các chương trình.

Đề xuất này cố gắng xác định một cách để nhúng Move VM sao cho:

- các lệnh gọi đa mô-đun trong Move không yêu cầu thời gian chạy

  kiểm tra thời gian chạy chương trình chéo

- Chương trình Move có thể tận dụng chức năng trong các chương trình Solana khác và các chương trình khác

  ngược lại

- Sự song song thời gian chạy của Solana được hiển thị với các đợt Move và không Move

  các giao dịch

## Giải pháp đề xuất

### Move VM dưới dạng trình nạp Solana

Move VM sẽ được nhúng dưới dạng trình nạp Solana dưới mã định danh `MOVE_PROGRAM_ID`, để các mô-đun Move có thể được đánh dấu là `thực thi` với VM là `chủ sở hữu`. Điều này sẽ cho phép các mô-đun tải các phụ thuộc mô-đun, cũng như cho phép thực thi song song các tập lệnh Move.

Tất cả tài khoản dữ liệu do mô-đun Move sở hữu phải đặt chủ sở hữu của chúng thành trình nạp, `MOVE_PROGRAM_ID`. Vì các mô-đun Move đóng gói dữ liệu tài khoản của chúng giống như cách các chương trình Solana đóng gói chúng, nên chủ sở hữu mô-đun Move sẽ được nhúng vào dữ liệu tài khoản. Thời gian chạy sẽ cấp quyền ghi vào Move VM và Move cấp quyền truy cập vào các tài khoản mô-đun.

### Tương tác với các chương trình Solana

Để gọi các hướng dẫn trong các chương trình không phải Move, Solana sẽ cần mở rộng Move VM bằng một lệnh gọi hệ thống `process_instruction()`. Nó sẽ hoạt động giống như các chương trình `process_instruction()` Rust BPF.
