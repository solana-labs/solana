---
title: Giải phẫu một Validator
---

![Sơ đồ khối validator](/img/validator.svg)

## Pipelining

Những validator sử dụng rộng rãi phương pháp tối ưu hóa phổ biến trong thiết kế CPU, được gọi là _pipelining_. Pipelining là công cụ phù hợp cho công việc khi có một luồng dữ liệu đầu vào cần được xử lý theo một chuỗi các bước và có các phần cứng khác nhau chịu trách nhiệm cho từng bước. Ví dụ cơ bản là sử dụng máy giặt và máy sấy để giặt/sấy/gấp nhiều đồ giặt. Phải giặt trước khi sấy và làm khô trước khi gấp, mỗi thao tác trong ba thao tác được thực hiện bởi một bộ phận riêng biệt. Để tối đa hóa hiệu quả, người ta tạo ra một hệ thống các _giai đoạn_. Chúng ta sẽ gọi một công đoạn là máy giặt, một công đoạn khác là máy sấy, và một công đoạn thứ ba là gấp. Để chạy đường ống, người ta thêm khối lượng đồ giặt thứ hai vào máy giặt ngay sau khi khối lượng đầu tiên được thêm vào máy sấy. Tương tự như vậy, tải thứ ba được thêm vào máy giặt sau khi tải thứ hai ở trong máy sấy và tải thứ nhất đang được gấp. Bằng cách này, một người có thể thực hiện đồng thời ba khối lượng đồ giặt. Với tải vô hạn, đường ống sẽ liên tục hoàn thành tải ở tốc độ của giai đoạn chậm nhất trong đường ống.

## Pipelining trong Validator

Validator chứa hai quy trình tổng hợp, một quy trình được sử dụng trong chế độ leader được gọi là TPU và một được sử dụng trong chế độ validator được gọi là TVU. Trong cả hai trường hợp, phần cứng được kết nối là giống nhau, đầu vào mạng, GPU card, CPU core, ghi vào ổ đĩa và đầu ra mạng. Những gì nó làm với phần cứng đó là khác nhau. TPU tồn tại để tạo các mục sổ cái trong khi TVU tồn tại để xác thực chúng.
