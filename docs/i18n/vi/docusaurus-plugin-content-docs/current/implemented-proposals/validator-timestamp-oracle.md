---
title: Validator dấu thời gian tiên tri
---

Người dùng bên thứ ba của Solana đôi khi cần biết thời gian thực mà một khối được tạo ra, nói chung là để đáp ứng các yêu cầu tuân thủ đối với kiểm toán viên bên ngoài hoặc cơ quan thực thi pháp luật. Đề xuất này mô tả một validator dấu thời gian tiên tri sẽ cho phép một cụm Solana đáp ứng nhu cầu này.

Đề cương chung của việc thực hiện được đề xuất như sau:

- Theo khoảng thời gian đều đặn, mỗi validator ghi lại thời gian quan sát của nó cho một slot đã biết trên chuỗi (thông qua một Dấu thời gian được thêm vào một slot Bỏ phiếu)
- Mộ khách hàng có thể yêu cầu thời gian chặn cho một khối gốc bằng `getBlockTime` phương pháp RPC. Khi khách hàng yêu cầu dấu thời gian cho khối N:

  1. Validator xác định dấu thời gian một "cụm" cho slot được đánh dấu thời gian gần đây trước khối N bằng cách quan sát tất cả các hướng dẫn Bỏ phiếu có dấu thời gian được ghi lại trên sổ cái tham chiếu đến slot đó và xác định dấu thời gian trung bình có tỉ trọng stake.

  2. Dấu thời gian trung bình gần đây này sau đó được sử dụng để tính toán dấu thời gian của khối N bằng cách sử dụng thời lượng slot đã thiết lập của cụm

Các yêu cầu:

- Bất kỳ validator phát lại sổ cái trong tương lai phải đưa ra cùng một thời gian cho mỗi khối kể từ genesis
- Thời gian khối ước tính không được trôi quá một giờ hoặc lâu hơn trước khi phân giải thành dữ liệu trong thế giới thực (oracle)
- Thời gian khối không được kiểm soát bởi một nhà tiên tri tập trung duy nhất, mà lý tưởng là dựa trên một hàm sử dụng đầu vào từ tất cả các validator
- Mỗi validator phải duy trì một tiên tri dấu thời gian

Việc triển khai tương tự có thể cung cấp ước tính dấu thời gian cho một khối chưa được root. Tuy nhiên, vì slot có dấu thời gian gần đây nhất có thể chưa được root hoặc có thể chưa được root, nên dấu thời gian này sẽ không ổn định (có khả năng không đạt yêu cầu 1). Việc triển khai ban đầu sẽ nhắm mục tiêu đến các khối đã root, nhưng nếu có một trường hợp sử dụng cho dấu thời gian của khối gần đây, thì việc thêm apis RPC trong tương lai sẽ rất nhỏ.

## Thời gian ghi

Trong các khoảng thời gian đều đặn khi nó đang bỏ phiếu trên một slot cụ thể, mỗi validator ghi lại thời gian quan sát của nó bằng cách bao gồm một dấu thời gian trong việc gửi hướng dẫn Bỏ phiếu của nó. Slot tương ứng cho dấu thời gian là Slot mới nhất trong vectơ Bỏ phiếu (`Vote::slots.iter().max()`). Nó được ký bởi keypair nhận dạng của validator như một Phiếu bầu thông thường. Để bật báo cáo này, cấu trúc Phiếu bầu cần được mở rộng để bao gồm trường dấu thời gian, trường `timestamp: Option<UnixTimestamp>`, này sẽ được đặt thành `None` trong hầu hết các Phiếu bầu.

Kể từ https://github.com/solana-labs/solana/pull/10630, các validator gửi dấu thời gian cho mỗi phiếu bầu. Điều này cho phép triển khai dịch vụ lưu trữ thời gian khối cho phép các node tính toán dấu thời gian ước tính ngay sau khi khối được root và lưu trữ giá trị đó trong Blockstore. Điều này cung cấp dữ liệu liên tục và truy vấn nhanh chóng, trong khi vẫn đáp ứng yêu cầu 1) ở trên.

### Các tài khoản bỏ phiếu

Tài khoản bỏ phiếu của validator sẽ giữ dấu thời gian slot gần đây nhất của nó trong VoteState.

### Chương trình bình chọn

Chương trình Bỏ phiếu trên chuỗi cần được mở rộng để xử lý dấu thời gian được gửi cùng với hướng dẫn Bỏ phiếu từ những validator. Ngoài chức năng process_vote hiện tại của nó (bao gồm tải đúng tài khoản Bỏ phiếu và xác minh rằng người ký giao dịch là validator dự kiến), quy trình này cần so sánh dấu thời gian và slot tương ứng với các giá trị hiện được lưu trữ để xác minh rằng chúng đều đang tăng đơn điệu, và lưu trữ slot và dấu thời gian mới trong tài khoản.

## Tính toán dấu thời gian trung bình có tỉ trọng stake

Để tính toán dấu thời gian ước tính cho một khối cụ thể, trước tiên validator cần xác định slot được đánh dấu thời gian gần đây nhất:

```text
let timestamp_slot = floor(current_slot / timestamp_interval);
```

Sau đó, validator cần thu thập tất cả các giao dịch Bỏ phiếu với Dấu ấn từ sổ cái tham chiếu đến slot đó, bằng cách sử dụng `Blockstore::get_slot_entries()`. Vì các giao dịch này có thể mất một khoảng thời gian để leader tiếp cận và xử lý, validator cần phải quét một số khối đã hoàn thành sau timestamp_slot để có được một bộ Dấu thời gian hợp lý. Số lượng slot chính xác sẽ cần được điều chỉnh: Nhiều slot hơn sẽ cho phép sự tham gia của cụm nhiều hơn và nhiều điểm dữ liệu dấu thời gian hơn; ít slot hơn sẽ tăng tốc độ lọc dấu thời gian mất bao lâu.

Từ tập hợp các giao dịch này, validator tính toán dấu thời gian trung bình có tỉ trọng tiền stake, tham chiếu chéo các stake của kỷ nguyên từ `staking_utils::staked_nodes_at_epoch()`.

Bất kỳ validator nào phát lại sổ cái phải lấy cùng một dấu thời gian trung bình có tỉ trọng tiền stake bằng cách xử lý các giao dịch Dấu thời gian từ cùng một số slot.

## Tính toán thời gian ước tính cho một khối cụ thể

Khi dấu thời gian trung bình cho một slot đã biết được tính toán, việc tính dấu thời gian ước tính cho khối N tiếp theo là điều không cần thiết:

```text
let block_n_timestamp = mean_timestamp + (block_n_slot_offset * slot_duration);
```

trong đó `block_n_slot_offset` là sự khác biệt giữa slot của khối N và timestamp_slot và `slot_duration` có nguồn gốc từ cụm `slot_per_year` được lưu trữ trong mỗi Ngân hàng
