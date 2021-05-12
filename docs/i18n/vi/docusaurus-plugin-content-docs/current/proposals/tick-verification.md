---
title: Đánh dấu xác minh
---

Điều này thiết kế các tiêu chí và xác nhận của các đánh dấu trong một slot. Nó cũng mô tả việc xử lý lỗi và các điều kiện slashing liên quan đến cách hệ thống xử lý các đường truyền không đáp ứng các yêu cầu này.

# Cấu trúc Slot

Mỗi slot phải chứa một số lượng `ticks_per_slot` dự kiến. Phần cuối cùng được chia nhỏ trong một slot chỉ được chứa toàn bộ đánh dấu cuối cùng và không có gì khác. Leader cũng phải tick shred này chứa đánh dấu cuối cùng bằng cờ `LAST_SHRED_IN_SLOT`. Giữa các đánh dấu, phải có `hashes_per_tick` số lượng hàm băm.

# Xử lý đường truyền kém

Việc truyền dữ liệu độc hại `T` được xử lý theo hai cách:

1. Nếu leader có thể tạo ra một số lần truyền sai `T` và một số lần truyền thay thế `T'` cho cùng một slot mà không vi phạm bất kỳ quy tắc slashing nào đối với việc truyền trùng lặp (ví dụ: nếu `T'` là một tập con của `T`), sau đó cụm phải xử lý khả năng cả hai đường truyền đang hoạt động.

Do đó, điều này có nghĩa là chúng tôi không thể đánh dấu việc truyền bị lỗi `T` là đã chết vì cụm có thể đã đạt được sự đồng thuận trên `T'`. Những trường hợp này cần có bằng chứng slashing để trừng trị hành vi xấu này.

2. Nếu không, chúng có thể đánh dấu slot là đã chết và không thể chơi được. Một bằng chứng slashing có thể cần thiết hoặc không tùy thuộc vào tính khả thi.

# Blockstore nhận shred

Khi blockstore nhận được một shred mới `s`, có hai trường hợp:

1. `s` được đánh dấu là `LAST_SHRED_IN_SLOT`, sau đó kiểm tra xem có tồn tại `s'` được shred trong blockstore cho slot đó `s'.index > s.index` Nếu vậy, `s` và `s'` cùng nhau tạo thành một bằng chứng slashing.

2. Blockstore đã nhận được một shred được `s'` đánh dấu là `LAST_SHRED_IN_SLOT` có chỉ mục `i`. Nếu `s.index > i`, sau đó cùng `s` và `s'` tạo thành một bằng chứng slashing. Trong trường hợp này, blockstore cũng sẽ không chèn `s`.

3. Các mẩu tin trùng lặp cho cùng một chỉ mục được bỏ qua. Các shred không trùng lặp cho cùng một chỉ mục là một điều kiện dễ slashing. Chi tiết cho trường hợp này được đề cập trong `Leader Duplicate Block Slashing`.

# Phát lại và xác nhận các đánh dấu

1. Giai đoạn phát lại các mục nhập từ blockstore, theo dõi số lượng đánh dấu mà nó đã nhìn thấy trên mỗi slot và xác minh có `hashes_per_tick` số lần băm giữa các lần đánh dấu. Sau khi đánh dấu từ shred cuối cùng này đã được phát, giai đoạn phát lại sau đó kiểm tra tổng số lần đánh dấu.

Tình huống thất bại 1: Nếu có hai đánh dấu liên tiếp giữa số lần băm là `!= hashes_per_tick`, đánh dấu slot này là đã chết.

Tình huống thất bại 2: Nếu số lượng đánh dấu ! = `ticks_per_slot`, hãy đánh dấu slot này đã chết.

Tình huống thất bại 3: Nếu số lượng đánh dấu đạt đến `ticks_per_slot`, nhưng chúng tôi vẫn chưa thấy `LAST_SHRED_IN_SLOT`, hãy đánh dấu slot này là đã chết.

2. Khi ReplayStage đạt đến một shred được đánh dấu là shred cuối cùng, nó sẽ kiểm tra xem shred cuối cùng này có phải là đánh dấu hay không.

Trường hợp không thành công: Nếu không thể ký kết với cờ `LAST_SHRED_IN_SLOT` được deserialized thành một tick (không thể deserialize hoặc deserializing thành một mục nhập), đánh dấu slot này là đã chết.
