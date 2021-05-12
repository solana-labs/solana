---
title: Dịch Vụ Sửa Chữa
---

## Dịch Vụ Sửa Chữa

RepairService chịu trách nhiệm truy xuất các shred bị thiếu không được giao bằng các giao thức truyền thông chính như Turbine. Nó phụ trách quản lý các giao thức được mô tả bên dưới trong `Repair Protocols` bên dưới.

## Những thách thức:

1. Các validator có thể không nhận được các shred cụ thể do lỗi mạng

2\) Hãy xem xét một tình huống trong đó blockstore chứa tập hợp các slot {1, 3, 5}. Sau đó, Blockstore nhận được các shred cho slot 7, nơi cho mỗi shred b, b.parent == 6, do đó, quan hệ cha-con 6 -&gt; 7 được lưu trữ trong blockstore. Tuy nhiên, không có cách nào để liên kết các slot này với bất kỳ ngân hàng hiện có nào trong Blockstore và do đó giao thức `Shred Repair` sẽ không sửa chữa các slot này. Nếu các slot này tình cờ là một phần của chuỗi chính, điều này sẽ tạm dừng tiến trình phát lại trên node này.

## Liên quan đến sữa chữa ban đầu

Epoch Slots: Mỗi validator quảng cáo riêng biệt trên gossip về các phần khác nhau của một `Epoch Slots`:

- The `stash`: Một tập hợp nén kéo dài một kỷ nguyên của tất cả các slot đã hoàn thành.
- The `cache`: Mã hóa thời gian chạy (RLE) của `N` slot đã hoàn thành mới nhất bắt đầu từ một số slot `M`, trong đó `N` là số lượng slot sẽ vừa với một gói có kích thước MTU.

`Epoch Slots` trong gossip được cập nhật mỗi khi validator nhận được một slot hoàn chỉnh trong kỷ nguyên. Các slot đã hoàn thành được blockstore phát hiện và gửi qua một kênh tới RepairService. Điều quan trọng cần lưu ý là chúng ta biết rằng vào thời điểm hoàn tất một slot `X`, lịch biểu kỷ nguyên phải tồn tại cho kỷ nguyên có chứa slot `X` vì WindowService sẽ từ chối các shred đối với các kỷ nguyên chưa được xác nhận.

Mỗi `N/2` slot đã hoàn thành, các slot `N/2` cũ nhất được chuyển từ `cache` vào `stash`. Giá trị cơ sở `M` cho RLE cũng phải được cập nhật.

## Các giao thức yêu cầu sửa chữa

Giao thức sửa chữa sẽ cố gắng tốt nhất để cải thiện cấu trúc forking của Blockstore.

Các chiến lược giao thức khác nhau để giải quyết những thách thức trên:

1. Sửa chữa Shred \(Giải quyết thách thức \# 1\): Đây là giao thức sửa chữa cơ bản nhất, với mục đích phát hiện và lấp đầy các "lỗ hổng" trên sổ cái. Blockstore theo dõi slot gốc mới nhất. RepairService sau đó sẽ lặp lại định kỳ với mọi fork trong blockstore bắt đầu từ slot gốc, gửi các yêu cầu sửa chữa tới các validator đối với bất kỳ shred nào bị thiếu. Nó sẽ gửi nhiều nhất một số `N` yêu cầu sửa chữa mỗi lần lặp lại. Sửa chữa shred nên ưu tiên sửa chữa các fork dựa trên tỉ trọng fork của leader. Các validator chỉ nên gửi yêu cầu sửa chữa đến những validator đã đánh dấu slot đó là đã hoàn thành trong EpochSlots của họ. Các validator nên ưu tiên sửa chữa các shred trong mỗi slot mà họ chịu trách nhiệm truyền lại qua turbine. Các validator có thể tính toán shred nào mà họ chịu trách nhiệm truyền lại vì hạt giống cho turbine dựa trên id leader, slot và chỉ số shred.

   Lưu ý: Các validator sẽ chỉ chấp nhận các shred trong phạm vi có thể xác minh hiện tại kỷ nguyên \(kỷ nguyên validator có lịch trình leader cho\).

2. Sửa chữa slot dự phòng \(Giải quyết thách thức \#2\): Mục tiêu của giao thức này là khám phá mối quan hệ chuỗi của các slot ""orphan" hiện không liên kết với bất kỳ fork nào đã biết. Sửa chữa shred nên ưu tiên sửa chữa các slot orphan dựa trên tỉ trọng fork của leader.

   - Blockstore sẽ theo dõi tập hợp các slot "orphan" trong một họ cột riêng biệt.
   - RepairService sẽ định kỳ thực hiện `Orphan` yêu cầu cho từng orphan trong blockstore.

     `Orphan(orphan)` yêu cầu - `orphan` là slot orphan mà người yêu cầu muốn biết cha mẹ của `Orphan(orphan)` phản hồi - Phần nhỏ nhất cho mỗi `N` cha mẹ đầu tiên của người được yêu cầu `orphan`

     Khi nhận được các phản hồi `p`, nơi `p` được shred trong một slot cha mẹ, các validator sẽ:

     - Chèn một ô trống `SlotMeta` trong blockstore `p.slot` nếu nó chưa tồn tại.
     - Nếu `p.slot` tồn tại, hãy cập nhật cấp độ gốc của `p` dựa trên `cha mẹ`

     Lưu ý: khi các slot trống này được thêm vào blockstore, `Shred Repair` giao thức sẽ cố gắng lấp đầy các slot đó.

     Lưu ý: Các validator sẽ chỉ chấp nhận các phản hồi có chứa các shred trong kỷ nguyên có thể xác minh hiện tại \(kỷ nguyên validator có lịch trình leader cho\).

Các validator nên cố gắng gửi yêu cầu orphan đến những validator đã đánh dấu orphan đó là đã hoàn thành trong EpochSlots của họ. Nếu không có các validator nào như vậy tồn tại, thì hãy chọn ngẫu nhiên một validator theo cách tính tỷ trọng.

## Giao thức phản hồi sửa chữa

Khi validator nhận được yêu cầu về một `S` được shred, họ sẽ trả lời bằng shred nếu họ có nó.

Khi validator nhận được một shred thông qua phản hồi sửa chữa,, họ sẽ kiểm tra `EpochSlots` </code> xem liệu <= `1/3` của mạng đã đánh dấu slot này là hoàn thành chưa. Nếu vậy, họ gửi lại shred này thông qua liên kết turbine liên quan của nó, nhưng chỉ khi validator này chưa truyền lại shred này trước đó.
