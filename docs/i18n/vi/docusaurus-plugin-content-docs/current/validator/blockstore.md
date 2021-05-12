---
title: Blockstore
---

Sau khi một khối đạt đến độ chính xác, tất cả các khối từ khối đó trở xuống đến khối genesis tạo thành một chuỗi tuyến tính với tên gọi quen thuộc là blockchain. Tuy nhiên, cho đến thời điểm đó, validator phải duy trì tất cả các chuỗi có khả năng hợp lệ, được gọi là _forks_. Quá trình mà các fork hình thành một cách tự nhiên do sự luân chuyển của leader được mô tả trong [quá trình tạo fork](../cluster/fork-generation.md). _blockstore_ cấu trúc dữ liệu được mô tả ở đây là cách validator đối phó với những fork đó cho đến khi các khối được hoàn thiện.

Blockstore cho phép validator ghi lại mọi shred mà nó quan sát được trên mạng, theo bất kỳ thứ tự nào, miễn là shred đó được ký bởi leader dự kiến ​​cho một slot nhất định.

Các shred được di chuyển đến một không gian khóa có fork với bộ giá trị `leader slot` + `shred index` \(trong slot\). Điều này cho phép cấu trúc danh sách bỏ qua của giao thức Solana được lưu trữ toàn bộ, mà không cần phải lựa chọn trước fork nào sẽ tuân theo, Entries nào sẽ tồn tại hoặc khi nào thì duy trì chúng.

Yêu cầu sửa chữa đối với các shred gần đây được cung cấp hết RAM hoặc các tệp gần đây và hết bộ nhớ sâu hơn đối với các shred gần đây hơn, như được thực hiện bởi cửa hàng hỗ trợ Blockstore.

## Chức năng của Blockstore

1. Tính bền bỉ: Blockstore tồn tại ở phía trước xác minh các node

   đường ống, ngay sau nhận mạng và xác minh chữ ký. Nếu

   shred nhận được phù hợp với lịch trình của leader \(tức là đã được ký bởi

   leader cho slot được chỉ định\), nó được lưu trữ ngay lập tức.

2. Sửa chữa: sửa chữa cũng giống như sửa chữa cửa sổ ở trên, nhưng có thể phục vụ bất kỳ

   shred đã được nhận. Các cửa hàng Blockstore được chia nhỏ với chữ ký,

   bảo tồn chuỗi gốc.

3. Forks: Blockstore hỗ trợ truy cập ngẫu nhiên các shred, vì vậy có thể hỗ trợ

   validator cần phải quay lại và phát lại từ điểm kiểm tra của Ngân hàng.

4. Khởi động lại: với việc pruning/culling thích hợp, Blockstore có thể được phát lại bằng

   liệt kê theo thứ tự các mục từ slot 0. Logic của giai đoạn phát lại

   \(nghĩa là giao dịch với các fork\) phải được sử dụng cho các mục nhập gần đây nhất trong

   các Blockstore.

## Thiết kế Blockstore

1. Các mục nhập trong Blockstore được lưu trữ dưới dạng các cặp khóa-giá trị, trong đó khóa là chỉ mục slot được ghép nối và chỉ mục shred cho một mục nhập và giá trị là dữ liệu mục nhập. Ghi chú các chỉ mục shred dựa trên 0 cho mỗi slot \(tức là chúng tương đối với slot\).
2. Blockstore duy trì siêu dữ liệu cho mỗi slot, trong cấu trúc `SlotMeta` chứa:

   - `slot_index` - Chỉ mục của slot này
   - `num_blocks` - Số khối trong slot \(được sử dụng để liên kết với một slot trước đó \)
   - `consumed` - Chỉ số shred cao nhất `n`, như vậy cho tất cả `m < n`, tồn tại một shred trong slot này với chỉ số shred bằng `n` \(tức là chỉ số shred liên tiếp cao nhất\).
   - `received` - Chỉ số shred nhận được cao nhất cho slot
   - `next_slots` - Danh sách các slot trong tương lai mà slot này có thể kết nối với nhau. Được sử dụng khi xây dựng lại

     sổ cái để tìm các điểm fork.

   - `last_index` - Chỉ mục của shred được gắn cờ là shred cuối cùng cho slot này. Cờ này trên shred sẽ được leader đặt cho một slot khi họ đang truyền shred cuối cùng cho một slot.
   - `is_rooted` - True iff mọi khối từ 0... slot tạo thành một chuỗi đầy đủ mà không có bất kỳ lỗ nào. Chúng ta có thể lấy được is_rooted cho mỗi slot với các quy tắc sau. Đặt slot\(n\) là slot có chỉ mục `n` và slot\(n\).is_full\(\) là true nếu slot có chỉ mục `n` có tất cả các đánh dấu mong đợi cho slot đó. Đặt is_rooted\(n\) để tuyên bố rằng "slot\(n\).is_rooted là đúng". Sau đó:

     is_rooted\(0\) is_rooted\(n+1\) iff \(is_rooted\(n\) và slot\(n\).is_full\(\)

3. Chuỗi - Khi một shred cho một slot mới `x` đến, chúng tôi kiểm tra số khối \(`num_blocks`\) cho slot mới đó \(thông tin này được mã hoá trong shred\). Sau đó, chúng tôi biết rằng chuỗi slot mới này sẽ chuyển sang slot `x - num_blocks`.
4. Đăng ký - Blockstore ghi lại một tập hợp các slot đã được "đăng ký". Điều này có nghĩa là các mục nhập liên kết với các slot này sẽ được gửi trên kênh Blockstore để tiêu thụ bởi ReplayStage. Xem `Blockstore APIs` để biết thêm chi tiết.
5. Cập nhật thông báo - Blockstore thông báo cho người nghe khi slot\(n\).is_rooted được chuyển từ sai thành đúng đối với bất kỳ `n` nào.

## API Blockstore

Blockstore cung cấp một API dựa trên đăng ký mà ReplayStage sử dụng để yêu cầu các mục nhập mà nó quan tâm. Các mục nhập sẽ được gửi trên một kênh do Blockstore hiển thị. Các API đăng ký này như sau: 1. `fn get_slots_since(slot_indexes: &[u64]) -> Vec<SlotMeta>`: Trả về các slot mới kết nối với bất kỳ phần tử nào từ danh sách `slot_indexes`.

1. `fn get_slot_entries(slot_index: u64, entry_start_index: usize, max_entries: Option<u64>) -> Vec<Entry>`: Trả về vector mục nhập cho slot bắt đầu bằng `entry_start_index`, giới hạn kết quả tại `max` nếu `max_entries == Some(max)`, nếu không, không áp dụng giới hạn trên về độ dài của vectơ trả về.

Lưu ý: Tích lũy, điều này có nghĩa là giai đoạn phát lại bây giờ sẽ phải biết khi nào một slot kết thúc và đăng ký slot tiếp theo mà nó quan tâm để nhận tập hợp các mục tiếp theo. Trước đây, gánh nặng của các chuỗi slot đã đổ lên Blockstore.

## Giao tiếp với Ngân hàng

Ngân hàng dự kiến ​​giai đoạn phát lại:

1. `prev_hash`: chuỗi PoH mà nó đang hoạt động như được chỉ ra bởi hàm băm của chuỗi cuối cùng

   mục nhập đó đã được xử lý

2. `tick_height`: các đánh dấu trong chuỗi PoH hiện đang được xác minh bởi điều này

   ngân hàng

3. `votes`: một chồng các bản ghi có chứa: 1. `prev_hashes`: bất cứ thứ gì sau cuộc bỏ phiếu này phải xâu chuỗi vào PoH 2. `tick_height`: chiều cao đánh dấu tại đó phiếu bầu này được bỏ 3. `lockout period`: chuỗi phải được quan sát trong sổ cái để

   có thể được xâu chuỗi bên dưới phiếu bầu này

Giai đoạn phát lại sử dụng các API của Blockstore để tìm chuỗi mục nhập dài nhất mà nó có thể treo bỏ phiếu bầu trước đó. Nếu chuỗi mục nhập đó không bị treo ở phiếu bầu mới nhất, giai đoạn phát lại sẽ quay trở lại ngân hàng về phiếu bầu đó và phát lại chuỗi từ đó.

## Blockstore bị loại bỏ

Khi các mục trong Blockstore đủ cũ, việc đại diện cho tất cả các fork có thể trở nên ít hữu ích hơn, thậm chí có thể có vấn đề khi phát lại khi khởi động lại. Tuy nhiên, khi phiếu bầu của validator đã đạt đến mức khóa tối đa, bất kỳ nội dung Blockstore nào không có trong chuỗi PoH cho phiếu bầu đó đều có thể bị loại bỏ, xóa bỏ.
