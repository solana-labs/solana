---
title: Dữ liệu cụm hệ thống
---

Solana hiển thị nhiều loại dữ liệu trạng thái cụm cho các chương trình thông qua các tài khoản [`sysvar`](terminology.md#sysvar). Các tài khoản này được điền tại các địa chỉ đã biết được xuất bản cùng với bố cục tài khoản trong [`solana-program` thùng](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/index.html) và được nêu bên dưới.

Để đưa dữ liệu hệ thống vào các hoạt động của chương trình, hãy chuyển địa chỉ tài khoản hệ thống vào danh sách tài khoản trong một giao dịch. Tài khoản có thể được đọc trong trình xử lý hướng dẫn của bạn giống như bất kỳ tài khoản nào khác. Access to sysvars accounts is always _readonly_.

## Đồng hồ

Hệ thống đồng hồ chứa dữ liệu về thời gian cụm, bao gồm slot hiện tại, kỷ nguyên và dấu thời gian Unix trên đồng hồ treo tường ước tính. Nó được cập nhật với mọi slot.

- Địa chỉ: `SysvarC1ock11111111111111111111111111111111`
- Bố cục: [Clock](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/clock/struct.Clock.html)
- Trường:

  - `slot`: slot hiện tại
  - `epoch_start_timestamp`: dấu thời gian Unix của slot đầu tiên trong kỷ nguyên này. Trong slot đầu tiên của kỷ nguyên, dấu thời gian này giống với `unix_timestamp` (bên dưới).
  - `epoch: `: kỷ nguyên hiện tại
  - `leader_schedule_epoch`: kỷ nguyên gần đây nhất mà lịch trình của leader được tạo
  - `unix_timestamp`: dấu thời gian Unix của slot này.

  Mỗi slot có thời lượng ước tính dựa trên Proof of History. Nhưng trên thực tế, các slot có thể trôi qua nhanh hơn và chậm hơn so với ước tính này. Kết quả là, dấu thời gian Unix của một slot được tạo dựa trên đầu vào tiên tri từ những validator bỏ phiếu. Dấu thời gian này được tính là giá trị trung bình có tỷ trọng stake của các ước tính dấu thời gian được cung cấp bởi các phiếu bầu, giới hạn bởi thời gian dự kiến ​​đã trôi qua kể từ khi bắt đầu kỷ nguyên.

  Rõ ràng hơn: đối với mỗi slot, dấu thời gian bỏ phiếu gần đây nhất do mỗi validator cung cấp được sử dụng để tạo ước tính dấu thời gian cho slot hiện tại (các slot đã trôi qua vì dấu thời gian bỏ phiếu được giả định là Bank::ns_per_slot). Mỗi ước tính dấu thời gian được liên kết với stake được ủy quyền cho tài khoản biểu quyết đó để tạo phân phối dấu thời gian bởi stake. Dấu thời gian trung bình được sử dụng làm `unix_timestamp`, trừ khi thời gian trôi qua kể từ khi `epoch_start_timestamp` đã sai lệch hơn 25% so với thời gian đã trôi qua dự kiến.

## EpochSchedule

The EpochSchedule sysvar contains epoch scheduling constants that are set in genesis, and enables calculating the number of slots in a given epoch, the epoch for a given slot, etc. (Note: the epoch schedule is distinct from the [`leader schedule`](terminology.md#leader-schedule))

- Địa chỉ: `SysvarEpochSchedu1e111111111111111111111111`
- Bố cục: [EpochSchedule](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/epoch_schedule/struct.EpochSchedule.html)

## Phí

Hệ thống phí chứa công cụ tính phí cho slot hiện tại. Nó được cập nhật ở mọi slot, dựa trên thống đốc tỷ lệ phí.

- Địa chỉ: `SysvarFees111111111111111111111111111111111`
- Bố cục: [Fees](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/fees/struct.Fees.html)

## Hướng dẫn

Hệ thống hướng dẫn chứa các hướng dẫn được tuần tự hóa trong một Tin nhắn trong khi Tin nhắn đó đang được xử lý. Điều này cho phép các hướng dẫn của chương trình tham chiếu đến các hướng dẫn khác trong cùng một giao dịch. Đọc thêm thông tin về [hướng dẫn bên trong](implemented-proposals/instruction_introspection.md).

- Địa chỉ: `Sysvar1nstructions1111111111111111111111111`
- Bố cục: [Instructions](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/instructions/struct.Instructions.html)

## RecentBlockhashes

Hệ thống RecentBlockhashes chứa các blockhashes đang hoạt động gần đây cũng như các công cụ tính phí liên quan của chúng. Nó được cập nhật với mọi slot.

- Địa chỉ: `SysvarRecentB1ockHashes11111111111111111111`
- Bố cục: [RecentBlockhashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/sysvar/recent_blockhashes/struct.RecentBlockhashes.html)

## Thuê

Hệ thống thuê bao chứa giá thuê. Hiện tại, tỷ lệ là tĩnh và được đặt trong genesis. Tỷ lệ phần trăm tiền thuê bị đốt được sửa đổi bằng cách kích hoạt tính năng thủ công.

- Địa chỉ: `SysvarRent111111111111111111111111111111111`
- Bố cục: [Rent](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/rent/struct.Rent.html)

## SlotHashes

Hệ thống SlotHashes chứa các hàm băm gần đây nhất của các ngân hàng mẹ của slot. Nó được cập nhật với mọi slot.

- Địa chỉ: `SysvarS1otHashes111111111111111111111111111`
- Bố cục: [SlotHashes](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_hashes/struct.SlotHashes.html)

## SlotHistory

Hệ thống SlotHistory chứa một bitvector của các slot có mặt trong kỷ nguyên trước. Nó được cập nhật với mọi slot.

- Địa chỉ: `SysvarS1otHistory11111111111111111111111111`
- Bố cục: [SlotHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/slot_history/struct.SlotHistory.html)

## StakeHistory

Hệ thống StakeHistory chứa lịch sử kích hoạt và hủy kích hoạt stake trên toàn bộ mỗi kỷ nguyên. Nó được cập nhật vào đầu mỗi kỷ nguyên.

- Địa chỉ: `SysvarStakeHistory1111111111111111111111111`
- Bố cục: [StakeHistory](https://docs.rs/solana-program/VERSION_FOR_DOCS_RS/solana_program/stake_history/struct.StakeHistory.html)
