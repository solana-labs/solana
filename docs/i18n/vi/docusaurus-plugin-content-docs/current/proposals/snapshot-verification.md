---
title: Xác minh Ảnh chụp nhanh
---

## Sự cố

Xác minh ảnh chụp nhanh các trạng thái tài khoản được triển khai, nhưng hàm băm ngân hàng của ảnh chụp nhanh được sử dụng để xác minh là có thể giả mạo.

## Giải pháp

Trong khi validator đang xử lý các giao dịch để bắt kịp cụm từ ảnh chụp nhanh, hãy sử dụng các giao dịch bỏ phiếu đến và máy tính cam kết để xác nhận rằng cụm thực sự đang xây dựng trên hàm băm ngân hàng được chụp nhanh. Sau khi đạt đến mức cam kết ngưỡng, hãy chấp nhận ảnh chụp nhanh là hợp lệ và bắt đầu bỏ phiếu.
