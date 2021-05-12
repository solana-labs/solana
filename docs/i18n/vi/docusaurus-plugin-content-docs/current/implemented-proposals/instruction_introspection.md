---
title: hướng dẫn introspection
---

## Sự cố

Một số chương trình hợp đồng thông minh có thể muốn xác minh rằng một Chỉ lệnh khác có trong một Thông báo nhất định vì Chỉ thị đó có thể đang thực hiện xác minh một số dữ liệu nhất định, trong một chức năng được biên dịch trước. (See secp256k1_instruction for an example).

## Giải pháp

Thêm một sysvar mới Sysvar1nstructions1111111111111111111111111 mà một chương trình có thể tham chiếu và nhận dữ liệu hướng dẫn của Message bên trong, đồng thời cũng là chỉ mục của lệnh hiện tại.

Có thể sử dụng hai hàm trợ giúp để trích xuất dữ liệu này:

```
fn load_current_index(instruction_data: &[u8]) -> u16;
fn load_instruction_at(instruction_index: usize, instruction_data: &[u8]) -> Result<Instruction>;
```

Thời gian chạy sẽ nhận ra lệnh đặc biệt này, tuần tự hóa dữ liệu lệnh Message cho nó và cũng ghi chỉ mục lệnh hiện tại và sau đó chương trình bpf có thể trích xuất thông tin cần thiết từ đó.

Lưu ý: sử dụng tuần tự hóa tùy chỉnh các lệnh vì bincode chậm hơn khoảng 10 lần trong mã gốc và vượt quá giới hạn lệnh BPF hiện tại.
